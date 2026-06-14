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
    StateUnavailable, UnitTally,
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
}

#[derive(Default)]
struct SpentTotals {
    food: f64,
    wood: f64,
    gold: f64,
    influence: f64,
}

impl SpentTotals {
    fn add(&mut self, cost: Option<Cost>) {
        if let Some(cost) = cost {
            self.food += cost.food;
            self.wood += cost.wood;
            self.gold += cost.gold;
            self.influence += cost.influence;
        }
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
                entry.spent.add(tech.cost);
            }
        }

        if let Some(unit) = command.unit.as_ref() {
            let tally = entry
                .units_trained
                .entry(unit.id)
                .or_insert_with(|| (unit.display_name.clone(), 0));
            tally.1 += 1;
            entry.spent.add(unit.cost); // every train is a real unit
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
                entry.spent.add(building.cost);
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
