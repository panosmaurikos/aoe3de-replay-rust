//! Mode B offset configuration — pointer chains as *data*, not code.
//!
//! Live game state (current resources, pop, score, ...) lives at addresses that
//! move every patch, so we never hard-code them. Instead each field is described
//! by a static `module-base + pointer-chain`, loaded from
//! `data/offsets/<gameVersion>.json` and resolved at runtime against the running
//! `aoe3de` process (see `mode_b::ProcessMemory`). A patch breaks capture → swap
//! the JSON, not the binary.
//!
//! This module is platform-independent and fully unit-tested without a game.

use std::collections::BTreeMap;
use std::path::Path;

use serde::{Deserialize, Serialize};

/// Numeric type of a captured field, as stored in game memory.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum FieldType {
    F32,
    F64,
    I32,
    U32,
}

impl FieldType {
    /// Width in bytes of this field in memory.
    pub fn width(self) -> usize {
        match self {
            FieldType::F32 | FieldType::I32 | FieldType::U32 => 4,
            FieldType::F64 => 8,
        }
    }

    /// Decode `bytes` (little-endian, length == `self.width()`) to an f64.
    /// All captured values surface as f64 so resources, pop, and score share one
    /// series type in the viewer.
    pub fn decode(self, bytes: &[u8]) -> Result<f64, String> {
        if bytes.len() < self.width() {
            return Err(format!(
                "field decode: need {} bytes, got {}",
                self.width(),
                bytes.len()
            ));
        }
        Ok(match self {
            FieldType::F32 => f32::from_le_bytes(bytes[..4].try_into().unwrap()) as f64,
            FieldType::F64 => f64::from_le_bytes(bytes[..8].try_into().unwrap()),
            FieldType::I32 => i32::from_le_bytes(bytes[..4].try_into().unwrap()) as f64,
            FieldType::U32 => u32::from_le_bytes(bytes[..4].try_into().unwrap()) as f64,
        })
    }
}

/// A pointer chain from the module base to one field of player slot 0.
///
/// Resolution: `addr = moduleBase + base`; then for each `off` in `chain`,
/// `addr = read_ptr(addr) + off`; the final field of player `slot` is read at
/// `addr + slot * playerStride` (stride applied by the sampler, not here).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FieldChain {
    /// Offset from the module base to the first pointer, as a hex string
    /// (`"0x01A2B3C0"`). Hex keeps the JSON readable and matches Cheat Engine.
    pub base: String,
    /// Offsets applied after each dereference. Empty = `base` is the value itself.
    #[serde(default)]
    pub chain: Vec<String>,
    #[serde(rename = "type")]
    pub field_type: FieldType,
}

impl FieldChain {
    /// Parse `base` to a u64 (`0x` prefix optional).
    pub fn base_offset(&self) -> Result<u64, String> {
        parse_hex(&self.base).map_err(|e| format!("field base '{}': {e}", self.base))
    }

    /// Parse the post-dereference offsets to u64s.
    pub fn chain_offsets(&self) -> Result<Vec<u64>, String> {
        self.chain
            .iter()
            .map(|s| parse_hex(s).map_err(|e| format!("chain offset '{s}': {e}")))
            .collect()
    }
}

/// A sanity gate: the sampler refuses to emit if this field for slot 0 is out of
/// `[min, max]`, so a post-patch wrong chain produces an error, not garbage.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SanityCheck {
    pub field: String,
    pub min: f64,
    pub max: f64,
}

/// The full offset config for one game version.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OffsetConfig {
    /// Human-readable game version these offsets were captured against
    /// (AoE3 DE About screen). Stamped onto every sample for auditability.
    #[serde(rename = "gameVersion")]
    pub game_version: String,
    /// Process to attach to, e.g. `"aoe3de.exe"`.
    #[serde(rename = "processName")]
    pub process_name: String,
    /// Module whose base anchors the chains (usually == `processName`).
    #[serde(rename = "moduleName")]
    pub module_name: String,
    /// Number of player slots to read.
    #[serde(rename = "playerCount")]
    pub player_count: u32,
    /// Bytes between consecutive players' copies of the same field.
    /// `0` (the template default) means "not yet discovered" → only slot 0 is
    /// meaningful; the sampler warns.
    #[serde(rename = "playerStride", default)]
    pub player_stride: u64,
    /// Field name → pointer chain. Ordered (BTreeMap) for deterministic output.
    pub fields: BTreeMap<String, FieldChain>,
    /// Optional anti-garbage gate.
    #[serde(default)]
    pub sanity: Option<SanityCheck>,
}

impl OffsetConfig {
    pub fn from_json(text: &str) -> Result<Self, String> {
        serde_json::from_str(text).map_err(|e| format!("invalid offset config JSON: {e}"))
    }

    pub fn load(path: &Path) -> Result<Self, String> {
        let text = std::fs::read_to_string(path)
            .map_err(|e| format!("could not read offset config '{}': {e}", path.display()))?;
        let cfg = Self::from_json(&text)?;
        cfg.validate()?;
        Ok(cfg)
    }

    /// Static checks that don't need a running game: non-empty fields, parseable
    /// hex, a real sanity target, a placeholder guard.
    pub fn validate(&self) -> Result<(), String> {
        if self.fields.is_empty() {
            return Err("offset config has no fields".into());
        }
        if self.player_count == 0 {
            return Err("offset config playerCount must be >= 1".into());
        }
        for (name, fc) in &self.fields {
            fc.base_offset()
                .map_err(|e| format!("field '{name}': {e}"))?;
            fc.chain_offsets()
                .map_err(|e| format!("field '{name}': {e}"))?;
            if fc.base_offset()? == 0 && fc.chain.is_empty() {
                return Err(format!(
                    "field '{name}' is still a template placeholder (base 0x0, no chain) — \
                     discover its pointer chain (see docs/mode-b-live-capture.md) before capturing"
                ));
            }
        }
        if let Some(s) = &self.sanity {
            if !self.fields.contains_key(&s.field) {
                return Err(format!(
                    "sanity check references unknown field '{}'",
                    s.field
                ));
            }
            if s.min > s.max {
                return Err(format!(
                    "sanity check min {} > max {} for field '{}'",
                    s.min, s.max, s.field
                ));
            }
        }
        Ok(())
    }
}

/// Parse a hex string with an optional `0x`/`0X` prefix and optional `_`
/// separators to a u64.
pub fn parse_hex(s: &str) -> Result<u64, String> {
    let t = s.trim();
    let stripped = t
        .strip_prefix("0x")
        .or_else(|| t.strip_prefix("0X"))
        .unwrap_or(t);
    let cleaned: String = stripped.chars().filter(|c| *c != '_').collect();
    if cleaned.is_empty() {
        return Err("empty hex value".into());
    }
    u64::from_str_radix(&cleaned, 16).map_err(|e| format!("not hex: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_hex_with_and_without_prefix() {
        assert_eq!(parse_hex("0x1A").unwrap(), 26);
        assert_eq!(parse_hex("1a").unwrap(), 26);
        assert_eq!(parse_hex("0x01_A2_B3").unwrap(), 0x01A2B3);
        assert!(parse_hex("0xZZ").is_err());
        assert!(parse_hex("").is_err());
    }

    #[test]
    fn field_type_decodes_little_endian() {
        assert_eq!(FieldType::I32.decode(&[1, 0, 0, 0]).unwrap(), 1.0);
        assert_eq!(FieldType::U32.decode(&200u32.to_le_bytes()).unwrap(), 200.0);
        assert_eq!(FieldType::F32.decode(&1.5f32.to_le_bytes()).unwrap(), 1.5);
        assert_eq!(FieldType::F64.decode(&2.5f64.to_le_bytes()).unwrap(), 2.5);
        assert!(FieldType::F64.decode(&[0, 0, 0, 0]).is_err());
    }

    #[test]
    fn round_trips_a_real_config() {
        let json = r#"{
            "gameVersion": "100.15.0",
            "processName": "aoe3de.exe",
            "moduleName": "aoe3de.exe",
            "playerCount": 2,
            "playerStride": 4096,
            "fields": {
                "coin": { "base": "0x01A2B3C0", "chain": ["0x10", "0x8"], "type": "f32" }
            },
            "sanity": { "field": "coin", "min": 0.0, "max": 1000000.0 }
        }"#;
        let cfg = OffsetConfig::from_json(json).unwrap();
        cfg.validate().unwrap();
        assert_eq!(cfg.player_stride, 4096);
        let coin = &cfg.fields["coin"];
        assert_eq!(coin.base_offset().unwrap(), 0x01A2B3C0);
        assert_eq!(coin.chain_offsets().unwrap(), vec![0x10, 0x8]);
        assert_eq!(coin.field_type, FieldType::F32);
    }

    #[test]
    fn rejects_template_placeholder_fields() {
        let json = r#"{
            "gameVersion": "TEMPLATE",
            "processName": "aoe3de.exe",
            "moduleName": "aoe3de.exe",
            "playerCount": 8,
            "fields": { "food": { "base": "0x0", "chain": [], "type": "f32" } }
        }"#;
        let cfg = OffsetConfig::from_json(json).unwrap();
        let err = cfg.validate().unwrap_err();
        assert!(err.contains("template placeholder"), "got: {err}");
    }

    #[test]
    fn rejects_sanity_for_unknown_field() {
        let json = r#"{
            "gameVersion": "x", "processName": "aoe3de.exe", "moduleName": "aoe3de.exe",
            "playerCount": 1,
            "fields": { "coin": { "base": "0x10", "chain": [], "type": "f32" } },
            "sanity": { "field": "ghost", "min": 0.0, "max": 1.0 }
        }"#;
        let cfg = OffsetConfig::from_json(json).unwrap();
        assert!(cfg.validate().unwrap_err().contains("unknown field"));
    }
}
