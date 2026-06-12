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
    /// Emit deck-resolved card send events into the normal timeline.
    /// Experimental: ownership comes from the command actor plus that actor's
    /// own active deck, but the decode is still under reverse engineering.
    pub experimental_shipments: bool,
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
    // The deck resolver needs the commandId=66 stream, so collect debug
    // commands whenever experimental shipments are requested too.
    let needs_debug_stream = options.debug_commands || options.experimental_shipments;
    let timeline = if needs_debug_stream {
        match parse_command_debug(file_bytes) {
            Ok((commands, debug_commands)) => {
                let timeline =
                    build_timeline(&replay, commands, None, Some(&debug_commands), &options);
                raw_debug_commands = debug_commands;
                timeline
            }
            Err(err) => {
                eprintln!("parseCommand failed: {err}");
                build_timeline(&replay, empty_commands(), Some(err), None, &options)
            }
        }
    } else {
        match parse_command(file_bytes) {
            Ok(commands) => build_timeline(&replay, commands, None, None, &options),
            Err(err) => {
                eprintln!("parseCommand failed: {err}");
                build_timeline(&replay, empty_commands(), Some(err), None, &options)
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

fn empty_commands() -> Commands {
    Commands {
        chat: Vec::new(),
        resigns: Vec::new(),
        card_sends: Vec::new(),
    }
}

fn build_timeline(
    replay: &Replay,
    commands: Commands,
    command_parse_error: Option<String>,
    deck_setup_commands: Option<&[models::RawDebugCommand]>,
    options: &ParseOptions,
) -> Timeline {
    use models::{TimelineEvent, TimelineEventType, TimelinePayload};

    let resolved_sends = if options.experimental_shipments {
        let resolver = DeckResolver::from_sources(replay, deck_setup_commands.unwrap_or_default());
        commands
            .card_sends
            .iter()
            .filter_map(|send| {
                let resolution = resolver.resolve(send.slot_id, send.time, send.deck_index);
                resolution.matched.then_some((send.clone(), resolution))
            })
            .collect::<Vec<_>>()
    } else {
        Vec::new()
    };
    let mut events =
        Vec::with_capacity(commands.chat.len() + commands.resigns.len() + resolved_sends.len());
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

    for (send, resolution) in resolved_sends {
        let card_id = resolution.card_id.unwrap_or(-1);
        let source = resolution
            .source
            .unwrap_or("debug_command66_deck_setup")
            .to_string();
        let label = match &resolution.deck_name {
            Some(deck_name) => format!(
                "Sent card {card_id} (deck \"{deck_name}\" slot {})",
                send.deck_index
            ),
            None => format!("Sent card {card_id} (deck slot {})", send.deck_index),
        };

        events.push((
            send.time,
            1u8,
            source_index,
            TimelineEvent {
                id: String::new(),
                event_type: TimelineEventType::Shipment,
                time: send.time,
                time_ms: send.time,
                actor: actor_for_slot(replay, send.slot_id, false),
                label: Some(label),
                payload: TimelinePayload::Shipment {
                    raw_command_id: send.raw_command_id,
                    card_id,
                    deck_index: send.deck_index,
                    resolved_name: None,
                    confidence: "medium".to_string(),
                    status: "candidate".to_string(),
                    source: format!("command_stream+actor_deck_match+{source}"),
                    note: format!(
                        "commandId=2 deck index {} resolved against the actor's own active deck ({source})",
                        send.deck_index
                    ),
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

/// A deck as known to the resolver: either parsed from the replay header or
/// reconstructed from commandId=66 deck-edit commands.
#[derive(Clone, Debug)]
struct ResolverDeck {
    deck_id: i32,
    deck_name: Option<String>,
    is_default: bool,
    card_ids: Vec<i32>,
    source: &'static str,
}

#[derive(Debug, Default)]
struct SlotDecks {
    decks: Vec<ResolverDeck>,
    /// commandId=66 deck selections as (timeMs, deckId), in command order.
    selections: Vec<(i32, i32)>,
}

#[derive(Clone, Debug, Default)]
pub struct CardResolution {
    pub matched: bool,
    pub active_deck_id: Option<i32>,
    pub deck_name: Option<String>,
    pub card_id: Option<i32>,
    pub source: Option<&'static str>,
    pub reason: Option<String>,
}

impl CardResolution {
    fn unresolved(reason: impl Into<String>) -> Self {
        Self {
            reason: Some(reason.into()),
            ..Self::default()
        }
    }
}

/// Resolves commandId=2 card sends (deck index clicks) against the acting
/// player's own decks. Ownership always comes from the command actor slot:
/// a candidate is never matched against another player's deck.
#[derive(Debug, Default)]
pub struct DeckResolver {
    by_slot: HashMap<i32, SlotDecks>,
}

impl DeckResolver {
    pub fn from_sources(replay: &Replay, debug: &[models::RawDebugCommand]) -> Self {
        let mut by_slot: HashMap<i32, SlotDecks> = HashMap::new();

        for player in &replay.players {
            let Some(slot) = player.slot_id else { continue };
            let entry = by_slot.entry(slot).or_default();
            for deck in &player.initial_decks {
                entry.decks.push(ResolverDeck {
                    deck_id: deck.deck_id,
                    deck_name: Some(deck.deck_name.clone()),
                    is_default: deck.is_default,
                    card_ids: deck.tech_ids.clone(),
                    source: "parsed_player_deck",
                });
            }
        }

        for cmd in debug {
            if cmd.command_id != 66 {
                continue;
            }
            let (Some(deck_id), Some(card_id)) = (
                cmd.decoded_fields.get("deckIdCandidate").copied(),
                cmd.decoded_fields.get("cardIdCandidate").copied(),
            ) else {
                continue;
            };
            let entry = by_slot.entry(cmd.player_slot_id).or_default();
            if card_id == -1 {
                entry.selections.push((cmd.time_ms, deck_id));
            } else if let Some(deck) = entry.decks.iter_mut().find(|deck| {
                deck.deck_id == deck_id && deck.source == "debug_command66_deck_setup"
            }) {
                deck.card_ids.push(card_id);
            } else {
                entry.decks.push(ResolverDeck {
                    deck_id,
                    deck_name: None,
                    is_default: false,
                    card_ids: vec![card_id],
                    source: "debug_command66_deck_setup",
                });
            }
        }

        Self { by_slot }
    }

    /// Resolve a deck-index card send for `slot` at `time_ms` to a card id in
    /// that slot's own active deck.
    pub fn resolve(&self, slot: i32, time_ms: i32, deck_index: i32) -> CardResolution {
        let Some(slot_decks) = self.by_slot.get(&slot) else {
            return CardResolution::unresolved("no deck data for actor slot");
        };
        if deck_index < 0 {
            return CardResolution::unresolved("negative deck index");
        }

        let active_deck_id = slot_decks
            .selections
            .iter()
            .take_while(|(selection_time, _)| *selection_time <= time_ms)
            .last()
            .map(|(_, deck_id)| *deck_id);
        let candidates: Vec<&ResolverDeck> = match active_deck_id {
            Some(deck_id) => slot_decks
                .decks
                .iter()
                .filter(|deck| deck.deck_id == deck_id)
                .collect(),
            None => {
                // No selection command seen: only trust an unambiguous fallback.
                if slot_decks.decks.len() == 1 {
                    slot_decks.decks.iter().collect()
                } else {
                    let defaults: Vec<&ResolverDeck> = slot_decks
                        .decks
                        .iter()
                        .filter(|deck| deck.is_default)
                        .collect();
                    if defaults.len() == 1 {
                        defaults
                    } else {
                        return CardResolution::unresolved(
                            "active deck unknown (no deck selection command, no unique default)",
                        );
                    }
                }
            }
        };

        if candidates.is_empty() {
            return CardResolution {
                active_deck_id,
                ..CardResolution::unresolved("active deck id not found in known decks")
            };
        }

        let resolved: Vec<(&ResolverDeck, i32)> = candidates
            .iter()
            .filter_map(|deck| {
                deck.card_ids
                    .get(deck_index as usize)
                    .map(|card_id| (*deck, *card_id))
            })
            .collect();

        if resolved.is_empty() {
            return CardResolution {
                active_deck_id: active_deck_id.or(candidates.first().map(|deck| deck.deck_id)),
                deck_name: candidates.first().and_then(|deck| deck.deck_name.clone()),
                ..CardResolution::unresolved("deck index out of range for active deck")
            };
        }

        let first_card_id = resolved[0].1;
        if resolved
            .iter()
            .any(|(_, card_id)| *card_id != first_card_id)
        {
            return CardResolution {
                active_deck_id,
                ..CardResolution::unresolved(
                    "ambiguous deck contents (multiple decks share the active deck id)",
                )
            };
        }

        // Prefer the parsed header deck for provenance when both agree.
        let (deck, card_id) = resolved
            .iter()
            .find(|(deck, _)| deck.source == "parsed_player_deck")
            .copied()
            .unwrap_or(resolved[0]);

        CardResolution {
            matched: true,
            active_deck_id: Some(deck.deck_id),
            deck_name: deck.deck_name.clone(),
            card_id: Some(card_id),
            source: Some(deck.source),
            reason: None,
        }
    }
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
    let mut card_send_candidate_count = 0usize;
    let resolver = DeckResolver::from_sources(replay, &raw_commands);
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
            if command.parsed_as == "card_send_candidate" {
                card_send_candidate_count += 1;
            }
            let card_id_candidate = command.decoded_fields.get("cardIdCandidate").copied();
            let deck_matches = card_id_candidate
                .map(|card_id| deck_matches_for_card(replay, command.player_slot_id, card_id))
                .unwrap_or_default();

            // Resolve card sends (commandId=2, deck index variant) against the
            // acting slot's own deck. Train-unit variants get no deckMatch.
            let deck_match = if command.parsed_as == "card_send_candidate" {
                let deck_index = command
                    .decoded_fields
                    .get("deckIndexCandidate")
                    .copied()
                    .unwrap_or(-1);
                let resolution =
                    resolver.resolve(command.player_slot_id, command.time_ms, deck_index);
                Some(models::DebugDeckResolution {
                    matched: resolution.matched,
                    slot_id: command.player_slot_id,
                    deck_index,
                    active_deck_id: resolution.active_deck_id,
                    deck_name: resolution.deck_name,
                    card_id_candidate: resolution.card_id,
                    source: resolution.source.map(str::to_string),
                    confidence: if resolution.matched { "medium" } else { "low" }.to_string(),
                    reason: resolution.reason,
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
            shipment_candidate_count: card_send_candidate_count,
        },
    }
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

#[cfg(test)]
mod tests {
    use super::*;
    use models::{Deck, DeckCard, GameSetting, Player, RawDebugCommand};
    use std::collections::BTreeMap;

    fn empty_setting() -> GameSetting {
        GameSetting {
            game_name: None,
            allow_cheats: None,
            blockade: None,
            player_count: None,
            difficulty: None,
            starting_age: None,
            ending_age: None,
            is_treaty: None,
            allow_trade_monopoly: None,
            game_type: None,
            map_crc: None,
            map_name: None,
            map_info: None,
            map_set: None,
            free_for_all: None,
            host_time: None,
            koth: None,
            latency: None,
            map_set_name: None,
            map_resource: None,
            radom_seed: None,
            game_speed: None,
        }
    }

    fn player_with_decks(slot_id: i32, decks: Vec<Deck>) -> Player {
        Player {
            ai_personality: None,
            avatar_id: None,
            civ_id: None,
            civ_info: None,
            civ_is_random: None,
            clan: None,
            color: None,
            explorer_name: None,
            explorer_skin_id: None,
            handicap: None,
            homecity_file_name: None,
            homecity_level: None,
            homecity_name: None,
            slot_id: Some(slot_id),
            player_name: Some(format!("Player {slot_id}")),
            initial_decks: decks,
        }
    }

    fn deck(deck_id: i32, name: &str, is_default: bool, card_ids: &[i32]) -> Deck {
        Deck {
            deck_name: name.to_string(),
            deck_id,
            game_id: 0,
            is_default,
            card_count: card_ids.len() as i32,
            cards: card_ids
                .iter()
                .map(|raw_id| DeckCard { raw_id: *raw_id })
                .collect(),
            tech_ids: card_ids.to_vec(),
        }
    }

    fn replay_with_players(players: Vec<Player>) -> Replay {
        Replay {
            exe_version: None,
            setting: empty_setting(),
            players,
            teams: Vec::new(),
        }
    }

    fn command66(slot_id: i32, time_ms: i32, deck_id: i32, card_id: i32) -> RawDebugCommand {
        let mut decoded_fields = BTreeMap::new();
        decoded_fields.insert("deckIdCandidate".to_string(), deck_id);
        decoded_fields.insert("cardIdCandidate".to_string(), card_id);
        RawDebugCommand {
            offset: 0,
            time_ms,
            player_slot_id: slot_id,
            command_id: 66,
            command_name: "deck_select_or_card_add".to_string(),
            decoded: false,
            length: 83,
            hex_preview: String::new(),
            parsed_as: if card_id == -1 {
                "deck_select_candidate"
            } else {
                "deck_card_add_candidate"
            }
            .to_string(),
            decoded_fields,
            raw_fields: models::DebugRawFields {
                u16le: Vec::new(),
                u32le: Vec::new(),
            },
        }
    }

    #[test]
    fn resolves_card_in_selected_parsed_deck() {
        let replay = replay_with_players(vec![player_with_decks(
            2,
            vec![
                deck(0, "Opening", false, &[100, 101, 102]),
                deck(3, "Main", false, &[200, 201, 202]),
            ],
        )]);
        let selects = vec![command66(2, 1_000, 3, -1)];
        let resolver = DeckResolver::from_sources(&replay, &selects);

        let resolution = resolver.resolve(2, 5_000, 1);

        assert!(resolution.matched);
        assert_eq!(resolution.card_id, Some(201));
        assert_eq!(resolution.active_deck_id, Some(3));
        assert_eq!(resolution.source, Some("parsed_player_deck"));
    }

    #[test]
    fn never_matches_against_other_players_deck() {
        let replay = replay_with_players(vec![
            player_with_decks(1, vec![deck(0, "P1", true, &[100, 101])]),
            player_with_decks(2, vec![deck(0, "P2", true, &[500, 501])]),
        ]);
        let resolver = DeckResolver::from_sources(&replay, &[]);

        let resolution = resolver.resolve(1, 5_000, 1);

        assert!(resolution.matched);
        assert_eq!(resolution.card_id, Some(101));
        assert_ne!(resolution.card_id, Some(501));
    }

    #[test]
    fn unresolved_without_selection_among_multiple_non_default_decks() {
        let replay = replay_with_players(vec![player_with_decks(
            6,
            vec![
                deck(0, "A", false, &[100, 101]),
                deck(1, "B", false, &[200, 201]),
            ],
        )]);
        let resolver = DeckResolver::from_sources(&replay, &[]);

        let resolution = resolver.resolve(6, 5_000, 0);

        assert!(!resolution.matched);
        assert!(resolution.reason.is_some());
    }

    #[test]
    fn falls_back_to_unique_default_deck() {
        let replay = replay_with_players(vec![player_with_decks(
            4,
            vec![
                deck(0, "Default", true, &[100, 101]),
                deck(1, "Other", false, &[200, 201]),
            ],
        )]);
        let resolver = DeckResolver::from_sources(&replay, &[]);

        let resolution = resolver.resolve(4, 5_000, 0);

        assert!(resolution.matched);
        assert_eq!(resolution.card_id, Some(100));
    }

    #[test]
    fn builds_deck_from_command66_adds_and_select() {
        // testship pattern: no parsed players, deck 0 built card-by-card, then selected.
        let replay = replay_with_players(Vec::new());
        let mut commands = vec![
            command66(2, 592, 0, 1676),
            command66(2, 621, 0, 714),
            command66(2, 649, 0, 708),
        ];
        commands.push(command66(2, 1_255, 0, -1));
        let resolver = DeckResolver::from_sources(&replay, &commands);

        let resolution = resolver.resolve(2, 180_514, 0);

        assert!(resolution.matched);
        assert_eq!(resolution.card_id, Some(1676));
        assert_eq!(resolution.source, Some("debug_command66_deck_setup"));
    }

    #[test]
    fn unresolved_when_deck_index_out_of_range() {
        let replay = replay_with_players(vec![player_with_decks(
            1,
            vec![deck(0, "Only", true, &[100, 101])],
        )]);
        let resolver = DeckResolver::from_sources(&replay, &[]);

        let resolution = resolver.resolve(1, 5_000, 7);

        assert!(!resolution.matched);
        assert_eq!(
            resolution.reason.as_deref(),
            Some("deck index out of range for active deck")
        );
    }

    #[test]
    fn selection_after_send_time_is_ignored() {
        let replay = replay_with_players(vec![player_with_decks(
            1,
            vec![
                deck(0, "Early", false, &[100, 101]),
                deck(1, "Late", false, &[200, 201]),
            ],
        )]);
        let commands = vec![command66(1, 10_000, 0, -1), command66(1, 50_000, 1, -1)];
        let resolver = DeckResolver::from_sources(&replay, &commands);

        let resolution = resolver.resolve(1, 20_000, 0);

        assert!(resolution.matched);
        assert_eq!(resolution.card_id, Some(100));
        assert_eq!(resolution.active_deck_id, Some(0));
    }
}
