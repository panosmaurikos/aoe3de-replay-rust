pub mod binary;
pub mod command;
pub mod constants;
pub mod deck;
pub mod fields;
pub mod models;
pub mod replay;
pub mod team;

use std::collections::{BTreeMap, HashMap, HashSet};

pub type ParseResult<T> = Result<T, String>;

pub use command::{parse_command, parse_command_debug};
pub use models::{Commands, ParsedOutput, Replay, Timeline};
pub use replay::parse_replay;

#[derive(Clone, Copy, Debug, Default)]
pub struct ParseOptions {
    pub debug_commands: bool,
}

pub fn parse_all(file_bytes: &[u8]) -> ParseResult<ParsedOutput> {
    parse_all_with_options(file_bytes, ParseOptions::default())
}

pub fn parse_all_with_options(
    file_bytes: &[u8],
    options: ParseOptions,
) -> ParseResult<ParsedOutput> {
    let replay = parse_replay(file_bytes)?;
    let mut raw_debug_commands = Vec::new();
    let timeline = if options.debug_commands {
        match parse_command_debug(file_bytes) {
            Ok((commands, debug_commands)) => {
                let timeline = build_timeline(&replay, commands, None, Some(&debug_commands));
                raw_debug_commands = debug_commands;
                timeline
            }
            Err(err) => {
                eprintln!("parseCommand failed: {err}");
                build_timeline(
                    &replay,
                    Commands {
                        chat: Vec::new(),
                        resigns: Vec::new(),
                        shipments: Vec::new(),
                    },
                    Some(err),
                    None,
                )
            }
        }
    } else {
        match parse_command(file_bytes) {
            Ok(commands) => build_timeline(&replay, commands, None, None),
            Err(err) => {
                eprintln!("parseCommand failed: {err}");
                build_timeline(
                    &replay,
                    Commands {
                        chat: Vec::new(),
                        resigns: Vec::new(),
                        shipments: Vec::new(),
                    },
                    Some(err),
                    None,
                )
            }
        }
    };

    let summary = build_summary(&replay, &timeline);
    let result = infer_result(&replay, &timeline);
    let debug = options
        .debug_commands
        .then(|| build_debug_output(&replay, raw_debug_commands));

    Ok(ParsedOutput {
        schema_version: 1,
        timeline,
        summary,
        result,
        debug,
        replay,
    })
}

fn build_timeline(
    replay: &Replay,
    commands: Commands,
    command_parse_error: Option<String>,
    deck_setup_commands: Option<&[models::RawDebugCommand]>,
) -> Timeline {
    use models::{TimelineEvent, TimelineEventType, TimelinePayload};

    let deck_index = DeckIndex::from_sources(replay, deck_setup_commands.unwrap_or_default());
    let resolved_shipments = commands
        .shipments
        .iter()
        .filter_map(|shipment| resolve_shipment_candidate(&deck_index, shipment))
        .collect::<Vec<_>>();
    let shipment_arrivals = correlate_shipment_arrivals(&commands.chat, &resolved_shipments);
    let shipment_event_count = resolved_shipments.len();
    let mut events =
        Vec::with_capacity(commands.chat.len() + commands.resigns.len() + shipment_event_count);
    let mut source_index = 0usize;

    for message in commands.chat {
        events.push((
            message.time,
            0u8,
            source_index,
            TimelineEvent {
                id: String::new(),
                event_type: TimelineEventType::Chat,
                time: message.time,
                time_ms: message.time,
                actor: actor_for_slot(replay, message.from_id, true),
                label: None,
                payload: TimelinePayload::Chat {
                    to_id: message.to_id,
                    message: message.message,
                },
            },
        ));
        source_index += 1;
    }

    for resign in commands.resigns {
        events.push((
            resign.time,
            2u8,
            source_index,
            TimelineEvent {
                id: String::new(),
                event_type: TimelineEventType::Resign,
                time: resign.time,
                time_ms: resign.time,
                actor: actor_for_slot(replay, resign.slot_id, false),
                label: None,
                payload: TimelinePayload::Resign,
            },
        ));
        source_index += 1;
    }

    for (shipment, arrival) in resolved_shipments.into_iter().zip(shipment_arrivals) {
        let (label, resolved_name, arrival_chat_time_ms, confidence, status, source, note) =
            match arrival {
                Some(arrival) => (
                    arrival.label,
                    Some(arrival.name),
                    Some(arrival.time_ms),
                    "high".to_string(),
                    "confirmed".to_string(),
                    format!(
                        "command_stream+actor_deck_match+{}+arrival_chat",
                        shipment.deck_match_source
                    ),
                    format!(
                        "Matched commandId=2 card id against actor deck ({}) and nearest shipment arrival chat",
                        shipment.deck_match_source
                    ),
                ),
                None => (
                    format!("Sent shipment/card {}", shipment.card_id),
                    None,
                    None,
                    "medium".to_string(),
                    "candidate".to_string(),
                    format!(
                        "command_stream+actor_deck_match+{}",
                        shipment.deck_match_source
                    ),
                    format!(
                        "Matched commandId=2 candidate id against actor deck ({})",
                        shipment.deck_match_source
                    ),
                ),
            };

        events.push((
            shipment.time,
            1u8,
            source_index,
            TimelineEvent {
                id: String::new(),
                event_type: TimelineEventType::Shipment,
                time: shipment.time,
                time_ms: shipment.time,
                actor: actor_for_slot(replay, shipment.slot_id, false),
                label: Some(label),
                payload: TimelinePayload::Shipment {
                    raw_command_id: shipment.raw_command_id,
                    card_id: shipment.card_id,
                    resolved_name,
                    arrival_chat_time_ms,
                    confidence,
                    status,
                    source,
                    note,
                },
            },
        ));
        source_index += 1;
    }

    events.sort_by_key(|(time, event_order, source_index, _)| (*time, *event_order, *source_index));

    let events = events
        .into_iter()
        .enumerate()
        .map(|(index, (_, _, _, mut event))| {
            event.id = format!("event-{:06}", index + 1);
            event
        })
        .collect();

    Timeline {
        events,
        command_parse_error,
    }
}

#[derive(Clone, Debug)]
struct ResolvedShipment {
    slot_id: i32,
    time: i32,
    raw_command_id: i32,
    card_id: i32,
    deck_match_source: &'static str,
}

#[derive(Debug, Default)]
struct DeckIndex {
    by_slot: HashMap<i32, (HashSet<i32>, &'static str)>,
}

impl DeckIndex {
    fn from_sources(replay: &Replay, debug: &[models::RawDebugCommand]) -> Self {
        let mut by_slot: HashMap<i32, (HashSet<i32>, &'static str)> = HashMap::new();

        for player in &replay.players {
            let Some(slot) = player.slot_id else { continue };
            let mut ids = HashSet::new();
            for deck in &player.initial_decks {
                ids.extend(deck.tech_ids.iter().copied());
            }
            if !ids.is_empty() {
                by_slot.insert(slot, (ids, "parsed_player_deck"));
            }
        }

        for cmd in debug {
            if cmd.command_id != 66 {
                continue;
            }
            let entry = by_slot
                .entry(cmd.player_slot_id)
                .or_insert_with(|| (HashSet::new(), "debug_command66_deck_setup"));
            if let Some(card_id) = cmd.decoded_fields.get("cardIdCandidate").copied() {
                entry.0.insert(card_id);
            }
            for value in &cmd.numeric_candidates {
                entry.0.insert(*value);
            }
        }

        Self { by_slot }
    }

    fn lookup(&self, slot: i32) -> Option<(&HashSet<i32>, &'static str)> {
        self.by_slot.get(&slot).map(|(ids, src)| (ids, *src))
    }

    fn match_card(&self, slot: i32, candidates: &[i32]) -> Option<(i32, &'static str)> {
        let (ids, source) = self.lookup(slot)?;
        candidates
            .iter()
            .copied()
            .find(|id| ids.contains(id))
            .map(|id| (id, source))
    }
}

fn resolve_shipment_candidate(
    deck_index: &DeckIndex,
    shipment: &models::ShipmentCandidate,
) -> Option<ResolvedShipment> {
    let (card_id, source) = deck_index.match_card(shipment.slot_id, &shipment.candidate_ids)?;
    Some(ResolvedShipment {
        slot_id: shipment.slot_id,
        time: shipment.time,
        raw_command_id: shipment.raw_command_id,
        card_id,
        deck_match_source: source,
    })
}

fn actor_for_slot(replay: &Replay, slot_id: i32, zero_is_system: bool) -> models::TimelineActor {
    if zero_is_system && slot_id <= 0 {
        return models::TimelineActor {
            kind: models::ActorKind::System,
            slot_id: None,
            player_id: None,
            name: Some("System".to_string()),
        };
    }

    if let Some(player) = replay
        .players
        .iter()
        .find(|player| player.slot_id == Some(slot_id))
    {
        return models::TimelineActor {
            kind: models::ActorKind::Player,
            slot_id: Some(slot_id),
            player_id: Some(slot_id),
            name: player.player_name.clone(),
        };
    }

    if let Some(name) = participant_name_from_team(replay, slot_id) {
        return models::TimelineActor {
            kind: models::ActorKind::Player,
            slot_id: Some(slot_id),
            player_id: Some(slot_id),
            name: Some(name),
        };
    }

    models::TimelineActor {
        kind: models::ActorKind::Unknown,
        slot_id: Some(slot_id),
        player_id: None,
        name: None,
    }
}

#[derive(Clone, Debug)]
struct ShipmentArrival {
    name: String,
    time_ms: i32,
    label: String,
}

fn correlate_shipment_arrivals(
    chat: &[models::Message],
    shipments: &[ResolvedShipment],
) -> Vec<Option<ShipmentArrival>> {
    const MATCH_WINDOW_MS: i32 = 3_000;

    let mut arrivals = vec![None; shipments.len()];
    let mut seen_system_messages = HashSet::new();

    for message in chat {
        if message.from_id > 0 {
            continue;
        }

        let Some(arrival_match) = shipment_system_message(&message.message) else {
            continue;
        };

        if !seen_system_messages.insert((message.time, arrival_match.name.clone())) {
            continue;
        }

        let candidates = shipments
            .iter()
            .enumerate()
            .filter(|(index, shipment)| {
                arrivals[*index].is_none()
                    && shipment.time <= message.time
                    && message.time - shipment.time <= MATCH_WINDOW_MS
            })
            .collect::<Vec<_>>();
        let actor_slots = candidates
            .iter()
            .map(|(_, shipment)| shipment.slot_id)
            .collect::<HashSet<_>>();

        if actor_slots.len() != 1 {
            continue;
        }

        let Some((shipment_index, _)) = candidates
            .into_iter()
            .max_by_key(|(_, shipment)| shipment.time)
        else {
            continue;
        };

        arrivals[shipment_index] = Some(ShipmentArrival {
            name: arrival_match.name,
            time_ms: message.time,
            label: arrival_match.label,
        });
    }

    arrivals
}

struct ShipmentSystemMessage {
    name: String,
    label: String,
}

fn shipment_system_message(message: &str) -> Option<ShipmentSystemMessage> {
    let trimmed = message.trim();
    for suffix in [" Shipment has arrived.", " Shipment has arrived"] {
        if let Some(name) = trimmed.strip_suffix(suffix) {
            let name = name.trim();
            if !name.is_empty() {
                return Some(ShipmentSystemMessage {
                    name: name.to_string(),
                    label: format!("Sent shipment: {name}"),
                });
            }
        }
    }

    None
}

fn participant_name_from_team(replay: &Replay, slot_id: i32) -> Option<String> {
    replay
        .teams
        .iter()
        .find(|team| team.members.contains(&slot_id))
        .map(|team| {
            team.name
                .strip_prefix("Team ")
                .unwrap_or(&team.name)
                .trim()
                .to_string()
        })
        .filter(|name| !name.is_empty())
}

fn infer_result(replay: &Replay, timeline: &Timeline) -> models::InferredResult {
    let player_slots = player_slots_from_replay(replay);
    let resigned_slots: HashSet<i32> = timeline
        .events
        .iter()
        .filter(|event| matches!(event.event_type, models::TimelineEventType::Resign))
        .filter_map(|event| match event.actor.kind {
            models::ActorKind::Player => event.actor.slot_id,
            _ => None,
        })
        .collect();

    let teams: Vec<(i32, Vec<i32>)> = replay
        .teams
        .iter()
        .map(|team| {
            let members = team
                .members
                .iter()
                .copied()
                .filter(|slot_id| player_slots.contains(slot_id))
                .collect::<Vec<_>>();
            (team.id, members)
        })
        .filter(|(_, members)| !members.is_empty())
        .collect();

    if teams.len() < 2 {
        return result_not_inferred("Could not infer winner without at least two valid teams");
    }

    let mut losing_teams = Vec::new();
    let mut remaining_teams = Vec::new();

    for (team_id, members) in teams {
        if members
            .iter()
            .all(|slot_id| resigned_slots.contains(slot_id))
        {
            losing_teams.push(team_id);
        } else {
            remaining_teams.push(team_id);
        }
    }

    if losing_teams.is_empty() {
        return result_not_inferred("Could not infer winner because no full team resigned");
    }

    if remaining_teams.len() != 1 {
        return result_not_inferred(
            "Could not infer winner because resign events leave zero or multiple teams active",
        );
    }

    models::InferredResult {
        inferred: true,
        confidence: "medium".to_string(),
        winning_teams: remaining_teams,
        losing_teams: losing_teams.clone(),
        reason: format!(
            "All non-observer players from team(s) {} resigned",
            losing_teams
                .iter()
                .map(i32::to_string)
                .collect::<Vec<_>>()
                .join(", ")
        ),
    }
}

fn player_slots_from_replay(replay: &Replay) -> HashSet<i32> {
    let mut player_slots = replay
        .players
        .iter()
        .filter_map(|player| player.slot_id)
        .collect::<HashSet<_>>();

    if player_slots.is_empty() {
        for team in &replay.teams {
            player_slots.extend(team.members.iter().copied());
        }
    }

    player_slots
}

fn result_not_inferred(reason: &str) -> models::InferredResult {
    models::InferredResult {
        inferred: false,
        confidence: "low".to_string(),
        winning_teams: Vec::new(),
        losing_teams: Vec::new(),
        reason: reason.to_string(),
    }
}

fn build_debug_output(
    replay: &Replay,
    raw_commands: Vec<models::RawDebugCommand>,
) -> models::DebugOutput {
    let mut command_ids = BTreeMap::new();
    let mut unknown_command_ids = BTreeMap::new();
    let mut shipment_candidate_count = 0usize;
    let deck_index = DeckIndex::from_sources(replay, &raw_commands);
    let commands = raw_commands
        .into_iter()
        .map(|command| {
            *command_ids
                .entry(command.command_id.to_string())
                .or_insert(0) += 1;
            if is_unknown_parsed_as(&command.parsed_as) {
                *unknown_command_ids
                    .entry(command.command_id.to_string())
                    .or_insert(0) += 1;
            }
            if command.parsed_as == "shipment_candidate" {
                shipment_candidate_count += 1;
            }
            let card_id_candidate = command.decoded_fields.get("cardIdCandidate").copied();
            let deck_matches = card_id_candidate
                .map(|card_id| deck_matches_for_card(replay, command.player_slot_id, card_id))
                .unwrap_or_default();

            let deck_match = if command.command_id == 2 {
                let candidates = command_candidate_ids(&command);
                let resolved = deck_index.match_card(command.player_slot_id, &candidates);
                Some(match resolved {
                    Some((card_id, source)) => models::DebugDeckResolution {
                        matched: true,
                        slot_id: command.player_slot_id,
                        card_id_candidate: Some(card_id),
                        source: Some(source.to_string()),
                        confidence: "medium".to_string(),
                        reason: None,
                    },
                    None => models::DebugDeckResolution {
                        matched: false,
                        slot_id: command.player_slot_id,
                        card_id_candidate: None,
                        source: None,
                        confidence: "low".to_string(),
                        reason: Some("candidate id not found in actor deck".to_string()),
                    },
                })
            } else {
                None
            };

            models::DebugCommand {
                offset: command.offset,
                time_ms: command.time_ms,
                actor: actor_for_slot(replay, command.player_slot_id, false),
                command_id: command.command_id,
                command_name: command.command_name,
                decoded: command.decoded,
                length: command.length,
                hex_preview: command.hex_preview,
                parsed_as: command.parsed_as,
                decoded_fields: command.decoded_fields,
                raw_fields: command.raw_fields,
                deck_matches,
                deck_match,
            }
        })
        .collect();

    models::DebugOutput {
        commands,
        debug_summary: models::DebugSummary {
            command_ids,
            unknown_command_ids,
            shipment_candidate_count,
        },
    }
}

fn command_candidate_ids(command: &models::RawDebugCommand) -> Vec<i32> {
    let mut ids = Vec::new();
    for key in [
        "cardIdCandidate",
        "shipmentIdCandidate",
        "techIdCandidate",
        "protoIdCandidate",
    ] {
        if let Some(value) = command.decoded_fields.get(key).copied() {
            if (100..=20_000).contains(&value) && !ids.contains(&value) {
                ids.push(value);
            }
        }
    }
    for value in &command.numeric_candidates {
        if (100..=20_000).contains(value) && !ids.contains(value) {
            ids.push(*value);
        }
    }
    ids
}

fn deck_matches_for_card(
    replay: &Replay,
    slot_id: i32,
    card_id: i32,
) -> Vec<models::DebugDeckMatch> {
    let Some(player) = replay
        .players
        .iter()
        .find(|player| player.slot_id == Some(slot_id))
    else {
        return Vec::new();
    };

    player
        .initial_decks
        .iter()
        .flat_map(|deck| {
            deck.tech_ids
                .iter()
                .enumerate()
                .filter_map(|(card_index, raw_id)| {
                    (*raw_id == card_id).then(|| models::DebugDeckMatch {
                        source: "parsed_player_deck".to_string(),
                        deck_id: deck.deck_id,
                        deck_name: deck.deck_name.clone(),
                        card_index,
                        raw_id: *raw_id,
                    })
                })
        })
        .collect()
}

fn is_unknown_parsed_as(parsed_as: &str) -> bool {
    parsed_as.starts_with("unknown")
}

fn build_summary(replay: &Replay, timeline: &Timeline) -> models::ParsedSummary {
    let mut chat_count = 0usize;
    let mut resign_count = 0usize;
    let mut shipment_count = 0usize;
    let mut shipment_confirmed_count = 0usize;
    let mut shipment_candidate_count = 0usize;

    for event in &timeline.events {
        match event.event_type {
            models::TimelineEventType::Chat => chat_count += 1,
            models::TimelineEventType::Resign => resign_count += 1,
            models::TimelineEventType::Shipment => {
                shipment_count += 1;
                match &event.payload {
                    models::TimelinePayload::Shipment { status, .. } if status == "confirmed" => {
                        shipment_confirmed_count += 1;
                    }
                    _ => shipment_candidate_count += 1,
                }
            }
        }
    }

    models::ParsedSummary {
        event_count: timeline.events.len(),
        chat_count,
        resign_count,
        shipment_count,
        shipment_confirmed_count,
        shipment_candidate_count,
        player_count: replay.players.len(),
        team_count: replay.teams.len(),
    }
}
