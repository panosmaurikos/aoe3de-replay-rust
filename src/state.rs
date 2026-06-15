//! State engine v0: per-player aggregation of **command-derived** data.
//!
//! Honesty: a `.age3Yrec` is a command replay (see `docs/replay-format.md`), so
//! we can only report what a player *issued* — shipments/cards sent, techs
//! researched, units trained. Simulation outcomes (losses, active counts,
//! resources) are not in the file; they are listed in `unavailable` rather than
//! guessed.

use std::collections::BTreeMap;

use crate::gamedata::Cost;
use crate::models::{
    DebugCommand, DerivedEvent, PlayerState, PlayerStateCounts, Replay, ResourcesSpent,
    SpentByCategory, StateUnavailable, UnitTally,
};

/// A repeated card send within this window is treated as a double-click of one
/// shipment (verified: such pairs cluster <=200ms apart and flat across windows;
/// legitimate re-sends of infinite cards are seconds apart). Trains are NOT
/// de-duplicated — rapid same-unit trains are real queued units.
const SHIPMENT_DEDUP_MS: i32 = 500;

/// Fields a command-only replay cannot provide (need live capture / re-sim).
const UNAVAILABLE_FIELDS: &[&str] = &[
    "activeUnits",
    "unitsInQueue",
    "unitsLost",
    "militaryLostValue",
    "villagersLost",
    "villagerLostValue",
    "resourceValueLost",
    "idleVillagers",
    "currentResources",
    "scoreOverTime",
    "techAppliedToUnit",
];

const UNAVAILABLE_REASON: &str = "Not present in a command-only replay (no game \
state, only player inputs). Requires live game capture or full re-simulation. \
See docs/replay-format.md.";

#[derive(Default)]
struct Accumulator {
    shipments_sent: Vec<DerivedEvent>,
    techs_researched: Vec<DerivedEvent>,
    units_trained: BTreeMap<i32, (String, usize)>,
    buildings_built: Vec<DerivedEvent>,
    spent: SpentTotals,
    /// Every command this player issued (the basis for APM). A command is one
    /// game action — a move/train/build/research/etc. — so this is an honest
    /// actions count, not an input/click count (the file has no raw inputs).
    command_count: usize,
    first_command_ms: Option<i32>,
    last_command_ms: i32,
}

/// What a spend was for, so cumulative spend can be split over time.
#[derive(Clone, Copy)]
enum Category {
    Military,
    Economy,
    Upgrades,
}

#[derive(Default)]
struct SpentTotals {
    food: f64,
    wood: f64,
    gold: f64,
    influence: f64,
    // Cumulative spend by purpose (totals across resource types).
    military: f64,
    economy: f64,
    upgrades: f64,
    /// (timeMs, cumulative total) recorded at each actual spend.
    series: Vec<(i32, f64)>,
    /// (timeMs, cumulative military, economy, upgrades) at each actual spend.
    category_series: Vec<(i32, f64, f64, f64)>,
}

impl SpentTotals {
    fn record(&mut self, time_ms: i32, cost: Option<Cost>, category: Category) {
        let Some(cost) = cost.filter(|cost| cost.total() > 0.0) else {
            return;
        };
        self.food += cost.food;
        self.wood += cost.wood;
        self.gold += cost.gold;
        self.influence += cost.influence;
        match category {
            Category::Military => self.military += cost.total(),
            Category::Economy => self.economy += cost.total(),
            Category::Upgrades => self.upgrades += cost.total(),
        }
        self.series
            .push((time_ms, self.food + self.wood + self.gold + self.influence));
        self.category_series
            .push((time_ms, self.military, self.economy, self.upgrades));
    }
}

fn round1(value: f64) -> f64 {
    (value * 10.0).round() / 10.0
}

/// Aggregate already-resolved debug commands into per-player state.
pub fn build_player_states(replay: &Replay, commands: &[DebugCommand]) -> Vec<PlayerState> {
    let mut by_slot: BTreeMap<i32, Accumulator> = BTreeMap::new();

    for command in commands {
        let Some(slot_id) = command.actor.slot_id else {
            continue;
        };
        let entry = by_slot.entry(slot_id).or_default();

        // Count every command for APM, and track the player's active span.
        entry.command_count += 1;
        entry.first_command_ms.get_or_insert(command.time_ms);
        entry.last_command_ms = entry.last_command_ms.max(command.time_ms);

        // Card/shipment send: only deck-matched sends with a resolved card.
        if let Some(card) = command
            .deck_match
            .as_ref()
            .filter(|deck_match| deck_match.matched)
            .and_then(|deck_match| deck_match.card.as_ref())
        {
            // Skip a double-click duplicate of the same card by this player.
            let is_dup = entry.shipments_sent.last().is_some_and(|last| {
                last.id == card.card_id && command.time_ms - last.time_ms <= SHIPMENT_DEDUP_MS
            });
            if !is_dup {
                entry.shipments_sent.push(DerivedEvent {
                    time_ms: command.time_ms,
                    id: card.card_id,
                    name: card.display_name.clone(),
                });
            }
        }

        if let Some(tech) = command.tech.as_ref() {
            // Dedup double-click research (one logical research).
            let is_dup = entry.techs_researched.last().is_some_and(|last| {
                last.id == tech.id && command.time_ms - last.time_ms <= SHIPMENT_DEDUP_MS
            });
            if !is_dup {
                entry.techs_researched.push(DerivedEvent {
                    time_ms: command.time_ms,
                    id: tech.id,
                    name: tech.display_name.clone(),
                });
                entry.spent.record(command.time_ms, tech.cost, Category::Upgrades);
            }
        }

        if let Some(unit) = command.unit.as_ref() {
            let tally = entry
                .units_trained
                .entry(unit.id)
                .or_insert_with(|| (unit.display_name.clone(), 0));
            tally.1 += 1;
            // Every train is a real unit (no dedup). Military units feed the
            // military split; villagers/wagons/etc. count as economy.
            let category = if unit.mil {
                Category::Military
            } else {
                Category::Economy
            };
            entry.spent.record(command.time_ms, unit.cost, category);
        }

        if let Some(building) = command.building.as_ref() {
            // Drop a double-click duplicate of placing the same building.
            let is_dup = entry.buildings_built.last().is_some_and(|last| {
                last.id == building.id && command.time_ms - last.time_ms <= SHIPMENT_DEDUP_MS
            });
            if !is_dup {
                entry.buildings_built.push(DerivedEvent {
                    time_ms: command.time_ms,
                    id: building.id,
                    name: building.display_name.clone(),
                });
                entry.spent.record(command.time_ms, building.cost, Category::Economy);
            }
        }
    }

    // Ensure every known player appears, even with no commands.
    let mut states = Vec::new();
    for player in &replay.players {
        let Some(slot_id) = player.slot_id else {
            continue;
        };
        let accumulator = by_slot.remove(&slot_id).unwrap_or_default();
        states.push(make_state(
            slot_id,
            player.player_name.clone(),
            player.civ_info.as_ref().map(|civ| civ.name.to_string()),
            accumulator,
        ));
    }
    // Players that issued commands but were not in the parsed player block.
    for (slot_id, accumulator) in by_slot {
        states.push(make_state(slot_id, None, None, accumulator));
    }

    states.sort_by_key(|state| state.slot_id);
    states
}

fn make_state(
    slot_id: i32,
    name: Option<String>,
    civ: Option<String>,
    accumulator: Accumulator,
) -> PlayerState {
    let mut units_trained: Vec<UnitTally> = accumulator
        .units_trained
        .into_iter()
        .map(|(id, (name, count))| UnitTally { name, id, count })
        .collect();
    units_trained.sort_by(|a, b| b.count.cmp(&a.count).then(a.name.cmp(&b.name)));
    let units_trained_total = units_trained.iter().map(|tally| tally.count).sum();
    let spent = &accumulator.spent;
    let resources_spent = ResourcesSpent {
        food: round1(spent.food),
        wood: round1(spent.wood),
        gold: round1(spent.gold),
        influence: round1(spent.influence),
        total: round1(spent.food + spent.wood + spent.gold + spent.influence),
    };
    let resources_spent_series = spent
        .series
        .iter()
        .map(|(time_ms, total)| (*time_ms, total.round()))
        .collect();
    let spent_by_category_series = spent
        .category_series
        .iter()
        .map(|(time_ms, mil, eco, upg)| (*time_ms, mil.round(), eco.round(), upg.round()))
        .collect();
    let spent_by_category = SpentByCategory {
        military: round1(spent.military),
        economy: round1(spent.economy),
        upgrades: round1(spent.upgrades),
    };
    // APM over the player's active span (first→last command). 0 when there is
    // no measurable span (a single command, or none).
    let span_ms = accumulator
        .first_command_ms
        .map(|first| accumulator.last_command_ms - first)
        .unwrap_or(0);
    let apm = if span_ms > 0 && accumulator.command_count > 1 {
        round1(accumulator.command_count as f64 / (span_ms as f64 / 60_000.0))
    } else {
        0.0
    };

    PlayerState {
        slot_id,
        name,
        civ,
        counts: PlayerStateCounts {
            shipments_sent: accumulator.shipments_sent.len(),
            techs_researched: accumulator.techs_researched.len(),
            units_trained_total,
            buildings_built: accumulator.buildings_built.len(),
        },
        shipments_sent: accumulator.shipments_sent,
        techs_researched: accumulator.techs_researched,
        units_trained,
        buildings_built: accumulator.buildings_built,
        resources_spent,
        spent_by_category,
        resources_spent_series,
        spent_by_category_series,
        commands_total: accumulator.command_count,
        apm,
        unavailable: StateUnavailable {
            reason: UNAVAILABLE_REASON.to_string(),
            fields: UNAVAILABLE_FIELDS.to_vec(),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gamedata::CardRef;
    use crate::models::{
        ActorKind, DebugDeckResolution, DebugRawFields, Replay, TimelineActor,
    };
    use std::collections::BTreeMap;

    fn card_send(slot: i32, time_ms: i32, card_id: i32) -> DebugCommand {
        DebugCommand {
            offset: 0,
            time_ms,
            actor: TimelineActor {
                kind: ActorKind::Player,
                slot_id: Some(slot),
                player_id: Some(slot),
                name: Some(format!("P{slot}")),
            },
            command_id: 2,
            command_name: String::new(),
            decoded: false,
            length: 0,
            hex_preview: String::new(),
            parsed_as: "card_send_candidate".to_string(),
            decoded_fields: BTreeMap::new(),
            raw_fields: DebugRawFields {
                u16le: Vec::new(),
                u32le: Vec::new(),
            },
            deck_matches: Vec::new(),
            deck_match: Some(DebugDeckResolution {
                matched: true,
                slot_id: slot,
                deck_index: 0,
                active_deck_id: Some(0),
                deck_name: None,
                card_id_candidate: Some(card_id),
                source: None,
                confidence: "medium".to_string(),
                reason: None,
                card: Some(CardRef {
                    card_id,
                    display_name: format!("Card{card_id}"),
                    icon_key: "card.generic".to_string(),
                    known: true,
                }),
            }),
            unit: None,
            tech: None,
            building: None,
        }
    }

    fn replay_one_player() -> Replay {
        Replay {
            exe_version: None,
            setting: Default::default(),
            players: Vec::new(),
            teams: Vec::new(),
        }
    }

    #[test]
    fn dedupes_double_click_card_sends_but_keeps_distinct() {
        let commands = vec![
            card_send(1, 1_000, 500),  // first send
            card_send(1, 1_150, 500),  // double-click dup (150ms) -> dropped
            card_send(1, 9_000, 500),  // real re-send much later -> kept
            card_send(1, 9_100, 501),  // different card -> kept
        ];

        let states = build_player_states(&replay_one_player(), &commands);

        assert_eq!(states.len(), 1);
        assert_eq!(states[0].counts.shipments_sent, 3);
        assert_eq!(states[0].shipments_sent[0].time_ms, 1_000);
        assert_eq!(states[0].shipments_sent[1].time_ms, 9_000);
    }
}
