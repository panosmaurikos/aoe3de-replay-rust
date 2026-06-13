//! Game data layer: resolves numeric ids emitted by the parser into display
//! names and icon keys. The parser never hardcodes names/icons — it emits ids,
//! and this layer maps them. Data lives in `data/*.json`, compiled in via
//! `include_str!` so resolution always works without an external path.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

const CARDS_JSON: &str = include_str!("../data/cards.json");
const ICONS_JSON: &str = include_str!("../data/icons.json");

/// Icon key used when a card has no mapping or its card has no `iconKey`.
pub const GENERIC_CARD_ICON_KEY: &str = "card.generic";

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct CardDefinition {
    pub id: i32,
    #[serde(rename = "internalName", skip_serializing_if = "Option::is_none")]
    pub internal_name: Option<String>,
    #[serde(rename = "displayName")]
    pub display_name: String,
    #[serde(rename = "type", skip_serializing_if = "Option::is_none")]
    pub card_type: Option<String>,
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

#[derive(Debug, Default)]
pub struct GameData {
    cards: BTreeMap<i32, CardDefinition>,
    icons: BTreeMap<String, IconDefinition>,
}

impl GameData {
    /// Load the data compiled into the binary. On a parse error the affected
    /// map is left empty so resolution degrades gracefully instead of crashing.
    pub fn embedded() -> Self {
        let cards = serde_json::from_str::<BTreeMap<String, CardDefinition>>(CARDS_JSON)
            .map(|map| {
                map.into_iter()
                    .filter_map(|(key, value)| key.parse::<i32>().ok().map(|id| (id, value)))
                    .collect()
            })
            .unwrap_or_default();
        let icons = serde_json::from_str::<BTreeMap<String, IconDefinition>>(ICONS_JSON)
            .unwrap_or_default();
        Self { cards, icons }
    }

    pub fn card(&self, card_id: i32) -> Option<&CardDefinition> {
        self.cards.get(&card_id)
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
        // 3438 = HCXPCapitalism in the game data (dbid space). Note this is NOT
        // the replay deck rawId space; see docs/game-data-layer.md.
        let data = GameData::embedded();
        let card = data.resolve_card(3438);
        assert!(card.known);
        assert_eq!(card.display_name, "Capitalism");
        assert_eq!(card.icon_key, "card.Capitalism");
    }

    #[test]
    fn unknown_card_falls_back_gracefully() {
        let data = GameData::embedded();
        let card = data.resolve_card(999_999);
        assert!(!card.known);
        assert_eq!(card.display_name, "Unknown Card #999999");
        assert_eq!(card.icon_key, GENERIC_CARD_ICON_KEY);
    }
}
