//! Game data layer: resolves numeric ids emitted by the parser into display
//! names and icon keys. The parser never hardcodes names/icons — it emits ids,
//! and this layer maps them. Data lives in `data/*.json`, compiled in via
//! `include_str!` so resolution always works without an external path.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

const CARDS_JSON: &str = include_str!("../data/cards.json");
const UNITS_JSON: &str = include_str!("../data/units.json");
const ICONS_JSON: &str = include_str!("../data/icons.json");

/// Icon key used when a card has no mapping or its card has no `iconKey`.
pub const GENERIC_CARD_ICON_KEY: &str = "card.generic";
/// Icon key used when a unit has no mapping or no `iconKey`.
pub const GENERIC_UNIT_ICON_KEY: &str = "unit.generic";
/// Icon key used when a building has no mapping or no `iconKey`.
pub const GENERIC_BUILDING_ICON_KEY: &str = "building.generic";

fn is_zero(value: &f64) -> bool {
    *value == 0.0
}

/// Eco-resource cost (Ships/XP/Trade excluded). Amounts are game units.
#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize)]
pub struct Cost {
    #[serde(default, skip_serializing_if = "is_zero")]
    pub food: f64,
    #[serde(default, skip_serializing_if = "is_zero")]
    pub wood: f64,
    #[serde(default, skip_serializing_if = "is_zero")]
    pub gold: f64,
    #[serde(default, skip_serializing_if = "is_zero")]
    pub influence: f64,
}

impl Cost {
    pub fn total(&self) -> f64 {
        self.food + self.wood + self.gold + self.influence
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct CardDefinition {
    /// Techtree array index = the replay card `rawId` (the map key).
    pub id: i32,
    /// Game database id, for cross-reference. Not the replay id.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dbid: Option<i32>,
    #[serde(rename = "internalName", skip_serializing_if = "Option::is_none")]
    pub internal_name: Option<String>,
    #[serde(rename = "displayName")]
    pub display_name: String,
    #[serde(rename = "type", skip_serializing_if = "Option::is_none")]
    pub card_type: Option<String>,
    /// For units: `unit` (trainable population unit), `building`, or `other`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kind: Option<String>,
    /// For techs: `true` when this research advances the age (politician / wonder).
    #[serde(rename = "ageUp", default, skip_serializing_if = "Option::is_none")]
    pub age_up: Option<bool>,
    /// For units: `true` for a military unit (vs villager/economic).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mil: Option<bool>,
    /// Eco-resource cost to train/build/research this entry.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cost: Option<Cost>,
    #[serde(default)]
    pub civilizations: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub age: Option<i32>,
    #[serde(rename = "iconKey", skip_serializing_if = "Option::is_none")]
    pub icon_key: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub confidence: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct IconDefinition {
    pub path: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
    #[serde(default)]
    pub fallback: bool,
}

/// A frontend-friendly card reference: always has a display name (falling back
/// to `Unknown Card #id`) and an icon key (falling back to the generic icon).
#[derive(Clone, Debug, Serialize)]
pub struct CardRef {
    #[serde(rename = "cardId")]
    pub card_id: i32,
    #[serde(rename = "displayName")]
    pub display_name: String,
    #[serde(rename = "iconKey")]
    pub icon_key: String,
    /// `true` when the card id had an entry in `cards.json`.
    pub known: bool,
}

/// A frontend-friendly unit/tech reference. Always has a name (falling back to
/// `Unknown <Kind> #id`) and an icon key. `id` is the proto/techtree array index.
#[derive(Clone, Debug, Serialize)]
pub struct NamedRef {
    pub id: i32,
    #[serde(rename = "displayName")]
    pub display_name: String,
    #[serde(rename = "iconKey")]
    pub icon_key: String,
    pub known: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cost: Option<Cost>,
    /// True for a military unit (only meaningful for train-unit refs).
    #[serde(default, skip_serializing_if = "is_false")]
    pub mil: bool,
}

fn is_false(value: &bool) -> bool {
    !*value
}

#[derive(Debug, Default)]
pub struct GameData {
    cards: BTreeMap<i32, CardDefinition>,
    units: BTreeMap<i32, CardDefinition>,
    icons: BTreeMap<String, IconDefinition>,
}

impl GameData {
    /// Load the data compiled into the binary. On a parse error the affected
    /// map is left empty so resolution degrades gracefully instead of crashing.
    pub fn embedded() -> Self {
        Self {
            cards: parse_definitions(CARDS_JSON),
            units: parse_definitions(UNITS_JSON),
            icons: serde_json::from_str(ICONS_JSON).unwrap_or_default(),
        }
    }

    pub fn card(&self, card_id: i32) -> Option<&CardDefinition> {
        self.cards.get(&card_id)
    }

    pub fn unit(&self, unit_id: i32) -> Option<&CardDefinition> {
        self.units.get(&unit_id)
    }

    pub fn unit_count(&self) -> usize {
        self.units.len()
    }

    /// True only for population units (trainable). Buildings, walls and props
    /// resolve to `false` so the train-unit decoder can drop them.
    pub fn is_trainable_unit(&self, unit_id: i32) -> bool {
        self.units
            .get(&unit_id)
            .and_then(|unit| unit.kind.as_deref())
            == Some("unit")
    }

    /// True when the tech id (techtree index) is an age-up research.
    pub fn is_age_up(&self, tech_id: i32) -> bool {
        self.cards
            .get(&tech_id)
            .and_then(|tech| tech.age_up)
            .unwrap_or(false)
    }

    /// True for buildings/walls (proto `kind == "building"`).
    pub fn is_building(&self, proto_id: i32) -> bool {
        self.units
            .get(&proto_id)
            .and_then(|unit| unit.kind.as_deref())
            == Some("building")
    }

    /// Resolve a build proto id (commandId=3) to a building name/icon.
    pub fn resolve_building(&self, proto_id: i32) -> NamedRef {
        named_ref(
            self.units.get(&proto_id),
            proto_id,
            "Building",
            GENERIC_BUILDING_ICON_KEY,
        )
    }

    /// Resolve a train-unit proto id (proto array index) to a name/icon.
    pub fn resolve_unit(&self, unit_id: i32) -> NamedRef {
        named_ref(self.units.get(&unit_id), unit_id, "Unit", GENERIC_UNIT_ICON_KEY)
    }

    /// Resolve a research tech id (techtree array index) to a name/icon. Techs
    /// live in `cards.json` alongside cards (`type` = `tech`).
    pub fn resolve_tech(&self, tech_id: i32) -> NamedRef {
        named_ref(self.cards.get(&tech_id), tech_id, "Tech", GENERIC_CARD_ICON_KEY)
    }

    pub fn icon(&self, icon_key: &str) -> Option<&IconDefinition> {
        self.icons.get(icon_key)
    }

    pub fn card_count(&self) -> usize {
        self.cards.len()
    }

    /// Resolve a card id to a frontend-friendly reference, never failing.
    pub fn resolve_card(&self, card_id: i32) -> CardRef {
        match self.cards.get(&card_id) {
            Some(card) => CardRef {
                card_id,
                display_name: card.display_name.clone(),
                icon_key: card
                    .icon_key
                    .clone()
                    .unwrap_or_else(|| GENERIC_CARD_ICON_KEY.to_string()),
                known: true,
            },
            None => CardRef {
                card_id,
                display_name: format!("Unknown Card #{card_id}"),
                icon_key: GENERIC_CARD_ICON_KEY.to_string(),
                known: false,
            },
        }
    }
}

/// Parse a stringified-int-keyed definition map, dropping non-numeric keys and
/// degrading to empty on a parse error (never panics).
fn parse_definitions(json: &str) -> BTreeMap<i32, CardDefinition> {
    serde_json::from_str::<BTreeMap<String, CardDefinition>>(json)
        .map(|map| {
            map.into_iter()
                .filter_map(|(key, value)| key.parse::<i32>().ok().map(|id| (id, value)))
                .collect()
        })
        .unwrap_or_default()
}

fn named_ref(
    definition: Option<&CardDefinition>,
    id: i32,
    kind: &str,
    generic_icon: &str,
) -> NamedRef {
    match definition {
        Some(def) => NamedRef {
            id,
            display_name: def.display_name.clone(),
            icon_key: def
                .icon_key
                .clone()
                .unwrap_or_else(|| generic_icon.to_string()),
            known: true,
            cost: def.cost,
            mil: def.mil.unwrap_or(false),
        },
        None => NamedRef {
            id,
            display_name: format!("Unknown {kind} #{id}"),
            icon_key: generic_icon.to_string(),
            known: false,
            cost: None,
            mil: false,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn embedded_data_parses() {
        let data = GameData::embedded();
        assert!(
            data.card_count() >= 1,
            "expected at least the seeded card, got {}",
            data.card_count()
        );
    }

    #[test]
    fn resolves_known_card() {
        // 1676 = the replay card rawId for Capitalism, which is the techtree
        // array index (NOT the dbid 3438). See docs/game-data-layer.md.
        let data = GameData::embedded();
        let card = data.resolve_card(1676);
        assert!(card.known);
        assert_eq!(card.display_name, "Capitalism");
        assert_eq!(card.icon_key, "card.capitalism");
    }

    #[test]
    fn unknown_card_falls_back_gracefully() {
        let data = GameData::embedded();
        let card = data.resolve_card(999_999);
        assert!(!card.known);
        assert_eq!(card.display_name, "Unknown Card #999999");
        assert_eq!(card.icon_key, GENERIC_CARD_ICON_KEY);
    }

    #[test]
    fn resolves_known_unit_by_proto_index() {
        // proto[284] = Settler, proto[928] = Villager (proto array index = the
        // replay train-unit id, NOT the dbid).
        let data = GameData::embedded();
        assert!(data.unit_count() > 1000);
        assert_eq!(data.resolve_unit(284).display_name, "Settler");
        assert_eq!(data.resolve_unit(928).display_name, "Villager");
    }

    #[test]
    fn resolves_known_tech_by_techtree_index() {
        // techtree[410] = Placer Mines (a market economy tech).
        let data = GameData::embedded();
        assert_eq!(data.resolve_tech(410).display_name, "Placer Mines");
    }

    #[test]
    fn detects_age_up_techs() {
        // 522 = The Governor (an age-up politician); 410 = Placer Mines (not).
        let data = GameData::embedded();
        assert!(data.is_age_up(522));
        assert!(!data.is_age_up(410));
    }

    #[test]
    fn trainable_unit_filter_excludes_buildings_and_props() {
        let data = GameData::embedded();
        assert!(data.is_trainable_unit(284)); // Settler
        assert!(data.is_trainable_unit(928)); // Villager
        assert!(!data.is_trainable_unit(2055)); // PROP SPC EU Wall (building)
        assert!(!data.is_trainable_unit(294)); // TownCenter (building)
    }

    #[test]
    fn unknown_unit_falls_back_gracefully() {
        let data = GameData::embedded();
        let unit = data.resolve_unit(999_999);
        assert!(!unit.known);
        assert_eq!(unit.display_name, "Unknown Unit #999999");
        assert_eq!(unit.icon_key, GENERIC_UNIT_ICON_KEY);
    }
}
