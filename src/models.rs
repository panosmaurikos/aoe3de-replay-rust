use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Debug, Serialize)]
pub struct ParsedOutput {
    #[serde(rename = "schemaVersion")]
    pub schema_version: u32,
    pub timeline: Timeline,
    pub summary: ParsedSummary,
    pub result: InferredResult,
    /// Per-player command-derived aggregation (state engine). Present with
    /// `--events` or `--debug-commands`.
    #[serde(rename = "playerStates", skip_serializing_if = "Option::is_none")]
    pub player_states: Option<Vec<PlayerState>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub debug: Option<DebugOutput>,
    pub replay: Replay,
}

#[derive(Debug, Serialize)]
pub struct ParsedSummary {
    #[serde(rename = "eventCount")]
    pub event_count: usize,
    #[serde(rename = "chatCount")]
    pub chat_count: usize,
    #[serde(rename = "resignCount")]
    pub resign_count: usize,
    #[serde(rename = "shipmentCount")]
    pub shipment_count: usize,
    #[serde(rename = "shipmentConfirmedCount")]
    pub shipment_confirmed_count: usize,
    #[serde(rename = "shipmentCandidateCount")]
    pub shipment_candidate_count: usize,
    #[serde(rename = "playerCount")]
    pub player_count: usize,
    #[serde(rename = "teamCount")]
    pub team_count: usize,
}

#[derive(Debug, Serialize)]
pub struct Replay {
    #[serde(rename = "exeVersion")]
    pub exe_version: Option<i32>,
    pub setting: GameSetting,
    pub players: Vec<Player>,
    pub teams: Vec<Team>,
}

#[derive(Debug, Default, Serialize)]
pub struct GameSetting {
    #[serde(rename = "gameName")]
    pub game_name: Option<String>,
    #[serde(rename = "allowCheats")]
    pub allow_cheats: Option<bool>,
    pub blockade: Option<bool>,
    #[serde(rename = "playerCount")]
    pub player_count: Option<i32>,
    pub difficulty: Option<i32>,
    #[serde(rename = "startingAge")]
    pub starting_age: Option<i32>,
    #[serde(rename = "endingAge")]
    pub ending_age: Option<i32>,
    #[serde(rename = "isTreaty")]
    pub is_treaty: Option<bool>,
    #[serde(rename = "allowTradeMonopoly")]
    pub allow_trade_monopoly: Option<bool>,
    #[serde(rename = "gameType")]
    pub game_type: Option<i32>,
    #[serde(rename = "mapCRC")]
    pub map_crc: Option<i32>,
    #[serde(rename = "mapName")]
    pub map_name: Option<String>,
    #[serde(rename = "mapInfo")]
    pub map_info: Option<MapInfo>,
    #[serde(rename = "mapSet")]
    pub map_set: Option<String>,
    #[serde(rename = "freeForAll")]
    pub free_for_all: Option<bool>,
    #[serde(rename = "hostTime")]
    pub host_time: Option<i32>,
    pub koth: Option<bool>,
    pub latency: Option<i32>,
    #[serde(rename = "mapSetName")]
    pub map_set_name: Option<String>,
    #[serde(rename = "mapResource")]
    pub map_resource: Option<i32>,
    #[serde(rename = "radomSeed")]
    pub radom_seed: Option<i32>,
    #[serde(rename = "gameSpeed")]
    pub game_speed: Option<i32>,
}

#[derive(Clone, Debug, Serialize)]
pub struct Player {
    #[serde(rename = "aiPersonality")]
    pub ai_personality: Option<String>,
    #[serde(rename = "avatarId")]
    pub avatar_id: Option<String>,
    #[serde(rename = "civId")]
    pub civ_id: Option<i32>,
    #[serde(rename = "civInfo")]
    pub civ_info: Option<CivInfo>,
    #[serde(rename = "civIsRandom")]
    pub civ_is_random: Option<bool>,
    pub clan: Option<String>,
    pub color: Option<i32>,
    #[serde(rename = "explorerName")]
    pub explorer_name: Option<String>,
    #[serde(rename = "explorerSkinId")]
    pub explorer_skin_id: Option<i32>,
    pub handicap: Option<i32>,
    #[serde(rename = "homecityFileName")]
    pub homecity_file_name: Option<String>,
    #[serde(rename = "homecityLevel")]
    pub homecity_level: Option<i32>,
    #[serde(rename = "homecityName")]
    pub homecity_name: Option<String>,
    #[serde(rename = "slotId")]
    pub slot_id: Option<i32>,
    #[serde(rename = "playerName")]
    pub player_name: Option<String>,
    #[serde(rename = "initialDecks")]
    pub initial_decks: Vec<Deck>,
}

#[derive(Clone, Debug, Serialize)]
pub struct CivInfo {
    pub name: &'static str,
    #[serde(rename = "urlCircle")]
    pub url_circle: &'static str,
    #[serde(rename = "urlRectanle")]
    pub url_rectanle: &'static str,
    #[serde(rename = "urlLeft")]
    pub url_left: &'static str,
    #[serde(rename = "idCiv")]
    pub id_civ: i32,
    #[serde(rename = "homecityJson")]
    pub homecity_json: &'static str,
}

#[derive(Clone, Debug, Serialize)]
pub struct Team {
    pub id: i32,
    pub name: String,
    pub members: Vec<i32>,
}

#[derive(Clone, Debug, Serialize)]
pub struct Deck {
    #[serde(rename = "deckName")]
    pub deck_name: String,
    #[serde(rename = "deckId")]
    pub deck_id: i32,
    #[serde(rename = "gameId")]
    pub game_id: i32,
    #[serde(rename = "isDefault")]
    pub is_default: bool,
    #[serde(rename = "cardCount")]
    pub card_count: i32,
    pub cards: Vec<DeckCard>,
    #[serde(rename = "techIds")]
    pub tech_ids: Vec<i32>,
}

#[derive(Clone, Debug, Serialize)]
pub struct DeckCard {
    #[serde(rename = "rawId")]
    pub raw_id: i32,
}

#[derive(Debug, Serialize)]
pub struct Commands {
    pub chat: Vec<Message>,
    pub resigns: Vec<Resign>,
    #[serde(rename = "cardSends")]
    pub card_sends: Vec<CardSendCandidate>,
    /// commandId=1 research candidates (tech id = techtree array index).
    pub research: Vec<ActionCandidate>,
    /// commandId=2 train candidates (proto id = proto array index).
    pub trains: Vec<ActionCandidate>,
    /// commandId=3 build candidates (proto id = proto array index).
    pub builds: Vec<ActionCandidate>,
}

/// A raw player action carrying one game id (research tech / train unit / build).
#[derive(Clone, Debug, Serialize)]
pub struct ActionCandidate {
    #[serde(rename = "slotId")]
    pub slot_id: i32,
    pub time: i32,
    #[serde(rename = "rawId")]
    pub raw_id: i32,
}

#[derive(Debug, Serialize)]
pub struct Timeline {
    pub events: Vec<TimelineEvent>,
    #[serde(rename = "commandParseError", skip_serializing_if = "Option::is_none")]
    pub command_parse_error: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct TimelineEvent {
    pub id: String,
    #[serde(rename = "type")]
    pub event_type: TimelineEventType,
    pub time: i32,
    #[serde(rename = "timeMs")]
    pub time_ms: i32,
    pub actor: TimelineActor,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    pub payload: TimelinePayload,
}

#[derive(Clone, Copy, Debug, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TimelineEventType {
    Chat,
    Resign,
    Shipment,
    Research,
    Train,
    Build,
    AgeUp,
}

#[derive(Debug, Serialize)]
pub struct TimelineActor {
    pub kind: ActorKind,
    #[serde(rename = "slotId")]
    pub slot_id: Option<i32>,
    #[serde(rename = "playerId")]
    pub player_id: Option<i32>,
    pub name: Option<String>,
}

#[derive(Clone, Copy, Debug, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ActorKind {
    Player,
    System,
    Unknown,
}

#[derive(Debug, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum TimelinePayload {
    Chat {
        #[serde(rename = "toId")]
        to_id: i32,
        message: String,
    },
    Resign,
    Shipment {
        #[serde(rename = "rawCommandId")]
        raw_command_id: i32,
        #[serde(rename = "cardId")]
        card_id: i32,
        #[serde(rename = "deckIndex")]
        deck_index: i32,
        #[serde(rename = "cardName", skip_serializing_if = "Option::is_none")]
        card_name: Option<String>,
        #[serde(rename = "iconKey", skip_serializing_if = "Option::is_none")]
        icon_key: Option<String>,
        #[serde(rename = "resolvedName", skip_serializing_if = "Option::is_none")]
        resolved_name: Option<String>,
        confidence: String,
        status: String,
        source: String,
        note: String,
    },
    Research {
        #[serde(rename = "techId")]
        tech_id: i32,
        name: String,
        #[serde(rename = "iconKey", skip_serializing_if = "Option::is_none")]
        icon_key: Option<String>,
        confidence: String,
        source: String,
    },
    Train {
        #[serde(rename = "unitId")]
        unit_id: i32,
        name: String,
        #[serde(rename = "iconKey", skip_serializing_if = "Option::is_none")]
        icon_key: Option<String>,
        confidence: String,
        source: String,
    },
    Build {
        #[serde(rename = "buildingId")]
        building_id: i32,
        name: String,
        #[serde(rename = "iconKey", skip_serializing_if = "Option::is_none")]
        icon_key: Option<String>,
        confidence: String,
        source: String,
    },
    AgeUp {
        #[serde(rename = "techId")]
        tech_id: i32,
        name: String,
        #[serde(rename = "iconKey", skip_serializing_if = "Option::is_none")]
        icon_key: Option<String>,
        confidence: String,
        source: String,
    },
}

#[derive(Debug, Serialize)]
pub struct Message {
    #[serde(rename = "fromId")]
    pub from_id: i32,
    #[serde(rename = "toId")]
    pub to_id: i32,
    pub message: String,
    pub time: i32,
}

#[derive(Debug, Serialize)]
pub struct Resign {
    #[serde(rename = "slotId")]
    pub slot_id: i32,
    pub time: i32,
}

#[derive(Clone, Debug, Serialize)]
pub struct CardSendCandidate {
    #[serde(rename = "slotId")]
    pub slot_id: i32,
    pub time: i32,
    #[serde(rename = "rawCommandId")]
    pub raw_command_id: i32,
    #[serde(rename = "deckIndex")]
    pub deck_index: i32,
}

#[derive(Debug, Serialize)]
pub struct InferredResult {
    pub inferred: bool,
    pub confidence: String,
    #[serde(rename = "winningTeams")]
    pub winning_teams: Vec<i32>,
    #[serde(rename = "losingTeams")]
    pub losing_teams: Vec<i32>,
    pub reason: String,
}

#[derive(Debug, Serialize)]
pub struct DebugOutput {
    pub commands: Vec<DebugCommand>,
    #[serde(rename = "debugSummary")]
    pub debug_summary: DebugSummary,
}

#[derive(Debug, Serialize)]
pub struct PlayerState {
    #[serde(rename = "slotId")]
    pub slot_id: i32,
    pub name: Option<String>,
    pub civ: Option<String>,
    #[serde(rename = "shipmentsSent")]
    pub shipments_sent: Vec<DerivedEvent>,
    #[serde(rename = "techsResearched")]
    pub techs_researched: Vec<DerivedEvent>,
    #[serde(rename = "unitsTrained")]
    pub units_trained: Vec<UnitTally>,
    #[serde(rename = "buildingsBuilt")]
    pub buildings_built: Vec<DerivedEvent>,
    /// Gross eco resources the player *spent* on trains + builds + research
    /// (command-derived; no refunds; shipments excluded — they cost shipment
    /// points, not resources). Not the player's current/net resources.
    #[serde(rename = "resourcesSpent")]
    pub resources_spent: ResourcesSpent,
    /// Gross spend split by purpose: military units / economy (villagers +
    /// buildings) / upgrades (research). Totals across all resource types.
    #[serde(rename = "spentByCategory")]
    pub spent_by_category: SpentByCategory,
    /// Cumulative gross resources spent over time as `[timeMs, total]` points
    /// (one per spend). For an economy-pace chart. Same caveat as resourcesSpent.
    #[serde(rename = "resourcesSpentSeries", skip_serializing_if = "Vec::is_empty")]
    pub resources_spent_series: Vec<(i32, f64)>,
    pub counts: PlayerStateCounts,
    pub unavailable: StateUnavailable,
}

#[derive(Debug, Default, Serialize)]
pub struct ResourcesSpent {
    pub food: f64,
    pub wood: f64,
    pub gold: f64,
    pub influence: f64,
    pub total: f64,
}

#[derive(Debug, Default, Serialize)]
pub struct SpentByCategory {
    pub military: f64,
    pub economy: f64,
    pub upgrades: f64,
}

#[derive(Debug, Serialize)]
pub struct DerivedEvent {
    #[serde(rename = "timeMs")]
    pub time_ms: i32,
    pub id: i32,
    pub name: String,
}

#[derive(Debug, Serialize)]
pub struct UnitTally {
    pub name: String,
    pub id: i32,
    pub count: usize,
}

#[derive(Debug, Serialize)]
pub struct PlayerStateCounts {
    #[serde(rename = "shipmentsSent")]
    pub shipments_sent: usize,
    #[serde(rename = "techsResearched")]
    pub techs_researched: usize,
    #[serde(rename = "unitsTrainedTotal")]
    pub units_trained_total: usize,
    #[serde(rename = "buildingsBuilt")]
    pub buildings_built: usize,
}

#[derive(Debug, Serialize)]
pub struct StateUnavailable {
    pub reason: String,
    pub fields: Vec<&'static str>,
}

#[derive(Debug, Serialize)]
pub struct DebugCommand {
    pub offset: usize,
    #[serde(rename = "timeMs")]
    pub time_ms: i32,
    pub actor: TimelineActor,
    #[serde(rename = "commandId")]
    pub command_id: i32,
    #[serde(rename = "commandName")]
    pub command_name: String,
    pub decoded: bool,
    pub length: usize,
    #[serde(rename = "hexPreview")]
    pub hex_preview: String,
    #[serde(rename = "parsedAs")]
    pub parsed_as: String,
    #[serde(rename = "decodedFields")]
    pub decoded_fields: BTreeMap<String, i32>,
    #[serde(rename = "rawFields")]
    pub raw_fields: DebugRawFields,
    #[serde(rename = "deckMatches", skip_serializing_if = "Vec::is_empty")]
    pub deck_matches: Vec<DebugDeckMatch>,
    #[serde(rename = "deckMatch", skip_serializing_if = "Option::is_none")]
    pub deck_match: Option<DebugDeckResolution>,
    /// Resolved train-unit (commandId=2 train variant), from the game data layer.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub unit: Option<crate::gamedata::NamedRef>,
    /// Resolved research tech (commandId=1), from the game data layer.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tech: Option<crate::gamedata::NamedRef>,
    /// Resolved building (commandId=3 build), from the game data layer.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub building: Option<crate::gamedata::NamedRef>,
}

#[derive(Clone, Debug, Serialize)]
pub struct DebugDeckMatch {
    pub source: String,
    #[serde(rename = "deckId")]
    pub deck_id: i32,
    #[serde(rename = "deckName")]
    pub deck_name: String,
    #[serde(rename = "cardIndex")]
    pub card_index: usize,
    #[serde(rename = "rawId")]
    pub raw_id: i32,
}

#[derive(Debug, Serialize)]
pub struct DebugDeckResolution {
    pub matched: bool,
    #[serde(rename = "slotId")]
    pub slot_id: i32,
    #[serde(rename = "deckIndex")]
    pub deck_index: i32,
    #[serde(rename = "activeDeckId", skip_serializing_if = "Option::is_none")]
    pub active_deck_id: Option<i32>,
    #[serde(rename = "deckName", skip_serializing_if = "Option::is_none")]
    pub deck_name: Option<String>,
    #[serde(rename = "cardIdCandidate", skip_serializing_if = "Option::is_none")]
    pub card_id_candidate: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
    pub confidence: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    /// Resolved card from the game data layer (rawId = techtree index), present
    /// only when matched and the id resolves to a known card.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub card: Option<crate::gamedata::CardRef>,
}

#[derive(Debug, Serialize)]
pub struct DebugRawFields {
    #[serde(rename = "u16le")]
    pub u16le: Vec<DebugU16Field>,
    #[serde(rename = "u32le")]
    pub u32le: Vec<DebugU32Field>,
}

#[derive(Debug, Serialize)]
pub struct DebugU16Field {
    pub offset: usize,
    pub value: u16,
}

#[derive(Debug, Serialize)]
pub struct DebugU32Field {
    pub offset: usize,
    #[serde(rename = "u32")]
    pub value_u32: u32,
    #[serde(rename = "i32")]
    pub value_i32: i32,
}

#[derive(Debug, Serialize)]
pub struct DebugSummary {
    #[serde(rename = "commandIds")]
    pub command_ids: BTreeMap<String, usize>,
    #[serde(rename = "unknownCommandIds")]
    pub unknown_command_ids: BTreeMap<String, usize>,
    #[serde(rename = "shipmentCandidateCount")]
    pub shipment_candidate_count: usize,
}

#[derive(Debug)]
pub struct RawDebugCommand {
    pub offset: usize,
    pub time_ms: i32,
    pub player_slot_id: i32,
    pub command_id: i32,
    pub command_name: String,
    pub decoded: bool,
    pub length: usize,
    pub hex_preview: String,
    pub parsed_as: String,
    pub decoded_fields: BTreeMap<String, i32>,
    pub raw_fields: DebugRawFields,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct MapInfo {
    pub id: i32,
    #[serde(rename = "idStr")]
    pub id_str: String,
    #[serde(rename = "displayNameID")]
    pub display_name_id: String,
    pub details: String,
    pub imagepath: String,
    #[serde(rename = "isLarge", skip_serializing_if = "Option::is_none")]
    pub is_large: Option<bool>,
}
