//! Mode B capture configuration — the AoE3 DE live-memory model as *data*.
//!
//! The model (signature, struct offsets, resource decryption) is reverse-
//! engineered from the open-source AoE3 DE Lua engine
//! (github.com/SystematicSkid/Age-of-Empires-3-DE-Lua). Every version-specific
//! value lives here in JSON so a game patch is fixed by editing
//! `data/offsets/aoe3de.json`, never the binary.
//!
//! Resolution at runtime (see `mode_b`):
//!   1. AOB-scan the module for `game_instance_sig` → patternVA.
//!   2. `globalPtrVA = patternVA + sig_disp_offset + read_i32(patternVA + sig_disp_offset)`
//!      ... actually `patternVA + read_i32(patternVA + sig_disp_offset) + sig_insn_len`
//!      (RIP-relative `mov reg,[rip+disp32]`).
//!   3. `game = read_u64(globalPtrVA)`.
//!   4. `world = read_u64(game + world_offset)`;
//!      `players = read_u64(world + players_offset)`;
//!      `player[i] = read_u64(players + i*player_stride)`;
//!      `reslist = read_u64(player[i] + resource_list_offset)`.
//!   5. For resource index r: `enc = read_u32(reslist + 8*r)`;
//!      `bits = (enc + resource_add) ^ resource_xor`; value = `f32::from_bits(bits)`.
//!
//! This module is platform-independent and unit-tested without a game.

use std::collections::BTreeMap;
use std::path::Path;

use serde::{Deserialize, Serialize};

/// Decryption constants for the obfuscated resource floats.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ResourceCrypt {
    /// Added (mod 2^32) to the raw u32 before XOR. Hex string.
    pub add: String,
    /// XORed with the sum to recover the f32 bit pattern. Hex string.
    pub xor: String,
}

impl ResourceCrypt {
    pub fn add_u32(&self) -> Result<u32, String> {
        parse_hex_u32(&self.add).map_err(|e| format!("resourceCrypt.add '{}': {e}", self.add))
    }
    pub fn xor_u32(&self) -> Result<u32, String> {
        parse_hex_u32(&self.xor).map_err(|e| format!("resourceCrypt.xor '{}': {e}", self.xor))
    }
    /// Decrypt one raw resource word to its float value.
    pub fn decrypt(&self, raw: u32) -> Result<f32, String> {
        let bits = raw.wrapping_add(self.add_u32()?) ^ self.xor_u32()?;
        Ok(f32::from_bits(bits))
    }
    /// Inverse of `decrypt` — used by tests to plant known values in fake memory.
    pub fn encrypt(&self, value: f32) -> Result<u32, String> {
        let bits = value.to_bits() ^ self.xor_u32()?;
        Ok(bits.wrapping_sub(self.add_u32()?))
    }
}

/// Struct-walk offsets from the Game instance down to a player's resource list.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WalkOffsets {
    #[serde(rename = "world")]
    pub world: String,
    #[serde(rename = "players")]
    pub players: String,
    #[serde(rename = "numPlayers")]
    pub num_players: String,
    #[serde(rename = "playerStride")]
    pub player_stride: String,
    #[serde(rename = "resourceList")]
    pub resource_list: String,
    /// Optional: player name pointer (UTF-16) offset, for slot correlation.
    #[serde(rename = "playerName", default)]
    pub player_name: Option<String>,
    /// Optional: player age (int) offset.
    #[serde(rename = "playerAge", default)]
    pub player_age: Option<String>,
}

impl WalkOffsets {
    pub fn world_off(&self) -> Result<u64, String> {
        parse_hex(&self.world)
    }
    pub fn players_off(&self) -> Result<u64, String> {
        parse_hex(&self.players)
    }
    pub fn num_players_off(&self) -> Result<u64, String> {
        parse_hex(&self.num_players)
    }
    pub fn player_stride(&self) -> Result<u64, String> {
        parse_hex(&self.player_stride)
    }
    pub fn resource_list_off(&self) -> Result<u64, String> {
        parse_hex(&self.resource_list)
    }
    pub fn player_name_off(&self) -> Result<Option<u64>, String> {
        self.player_name.as_deref().map(parse_hex).transpose()
    }
    pub fn player_age_off(&self) -> Result<Option<u64>, String> {
        self.player_age.as_deref().map(parse_hex).transpose()
    }
}

/// Anti-garbage gate: a resource whose decrypted value must land in `[min, max]`
/// for slot `slot`, else the sampler errors (stale offsets / no match running).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SanityCheck {
    pub resource: String,
    pub slot: u32,
    pub min: f32,
    pub max: f32,
}

/// Full Mode B capture config for one game version.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CaptureConfig {
    #[serde(rename = "gameVersion")]
    pub game_version: String,
    /// The simulation worker process, e.g. `"AoE3DE_s.exe"`.
    #[serde(rename = "processName")]
    pub process_name: String,
    #[serde(rename = "moduleName")]
    pub module_name: String,
    /// AOB signature (bytes + `??` wildcards) locating the Game-instance pointer.
    #[serde(rename = "gameInstanceSig")]
    pub game_instance_sig: String,
    /// Byte offset of the disp32 within the matched instruction (3 for
    /// `48 8B 0D <disp32>`).
    #[serde(rename = "sigDispOffset", default = "default_disp_offset")]
    pub sig_disp_offset: u64,
    /// Total length of the matched instruction (7 for `mov reg,[rip+disp32]`).
    #[serde(rename = "sigInsnLen", default = "default_insn_len")]
    pub sig_insn_len: u64,
    pub walk: WalkOffsets,
    #[serde(rename = "resourceCrypt")]
    pub resource_crypt: ResourceCrypt,
    /// Resource name → index into the resource list (Gold=0, Wood=1, Food=2, ...).
    pub resources: BTreeMap<String, u32>,
    /// Hard cap on players to read, so a bad NumPlayers can't run away.
    #[serde(rename = "maxPlayers", default = "default_max_players")]
    pub max_players: u32,
    #[serde(default)]
    pub sanity: Option<SanityCheck>,
}

fn default_disp_offset() -> u64 {
    3
}
fn default_insn_len() -> u64 {
    7
}
fn default_max_players() -> u32 {
    16
}

impl CaptureConfig {
    pub fn from_json(text: &str) -> Result<Self, String> {
        serde_json::from_str(text).map_err(|e| format!("invalid capture config JSON: {e}"))
    }

    pub fn load(path: &Path) -> Result<Self, String> {
        let text = std::fs::read_to_string(path)
            .map_err(|e| format!("could not read capture config '{}': {e}", path.display()))?;
        let cfg = Self::from_json(&text)?;
        cfg.validate()?;
        Ok(cfg)
    }

    /// Static checks that don't need a running game.
    pub fn validate(&self) -> Result<(), String> {
        parse_signature(&self.game_instance_sig)
            .map_err(|e| format!("gameInstanceSig: {e}"))
            .and_then(|p| {
                if p.is_empty() {
                    Err("gameInstanceSig is empty".into())
                } else if p.iter().all(Option::is_none) {
                    Err("gameInstanceSig is all wildcards".into())
                } else {
                    Ok(())
                }
            })?;
        self.walk.world_off()?;
        self.walk.players_off()?;
        self.walk.num_players_off()?;
        self.walk.resource_list_off()?;
        let stride = self.walk.player_stride()?;
        if stride == 0 {
            return Err("walk.playerStride must be non-zero".into());
        }
        self.walk.player_name_off()?;
        self.walk.player_age_off()?;
        self.resource_crypt.add_u32()?;
        self.resource_crypt.xor_u32()?;
        if self.resources.is_empty() {
            return Err("config has no resources".into());
        }
        if self.max_players == 0 {
            return Err("maxPlayers must be >= 1".into());
        }
        if let Some(s) = &self.sanity {
            if !self.resources.contains_key(&s.resource) {
                return Err(format!(
                    "sanity references unknown resource '{}'",
                    s.resource
                ));
            }
            if s.min > s.max {
                return Err(format!("sanity min {} > max {}", s.min, s.max));
            }
        }
        Ok(())
    }
}

/// One pattern byte: `Some(b)` matches exactly, `None` is a `??` wildcard.
pub type SigByte = Option<u8>;

/// Parse an AOB signature like `"48 8B 0D ?? ?? ?? ?? 80 3D"` into bytes/wildcards.
/// Accepts `?` or `??` for wildcards; whitespace-separated.
pub fn parse_signature(sig: &str) -> Result<Vec<SigByte>, String> {
    let mut out = Vec::new();
    for tok in sig.split_whitespace() {
        if tok == "?" || tok == "??" {
            out.push(None);
        } else if tok.len() == 2 && tok.chars().all(|c| c.is_ascii_hexdigit()) {
            out.push(Some(u8::from_str_radix(tok, 16).unwrap()));
        } else {
            return Err(format!("bad signature token '{tok}'"));
        }
    }
    Ok(out)
}

/// Parse a hex string with optional `0x` prefix and `_` separators to u64.
pub fn parse_hex(s: &str) -> Result<u64, String> {
    let t = s.trim();
    let stripped = t
        .strip_prefix("0x")
        .or_else(|| t.strip_prefix("0X"))
        .unwrap_or(t);
    let cleaned: String = stripped.chars().filter(|c| *c != '_').collect();
    if cleaned.is_empty() {
        return Err(format!("empty hex value '{s}'"));
    }
    u64::from_str_radix(&cleaned, 16).map_err(|e| format!("'{s}' not hex: {e}"))
}

/// Parse a hex string to u32 (with wraparound semantics for crypto constants).
pub fn parse_hex_u32(s: &str) -> Result<u32, String> {
    let v = parse_hex(s)?;
    if v > u32::MAX as u64 {
        return Err(format!("'{s}' exceeds u32"));
    }
    Ok(v as u32)
}

#[cfg(test)]
mod tests {
    use super::*;

    const REAL: &str = r#"{
        "gameVersion": "from SystematicSkid Lua engine",
        "processName": "AoE3DE_s.exe",
        "moduleName": "AoE3DE_s.exe",
        "gameInstanceSig": "48 8B 0D ?? ?? ?? ?? 80 3D ?? ?? ?? ?? ?? 74 54",
        "walk": {
            "world": "0x148", "players": "0x98", "numPlayers": "0xA0",
            "playerStride": "0x8", "resourceList": "0x338",
            "playerName": "0x8", "playerAge": "0x80"
        },
        "resourceCrypt": { "add": "0x7BA9CCB8", "xor": "0x86A4DFC9" },
        "resources": { "coin": 0, "wood": 1, "food": 2, "xp": 5 },
        "sanity": { "resource": "coin", "slot": 1, "min": 0.0, "max": 1000000.0 }
    }"#;

    #[test]
    fn parses_the_real_config() {
        let cfg = CaptureConfig::from_json(REAL).unwrap();
        cfg.validate().unwrap();
        assert_eq!(cfg.process_name, "AoE3DE_s.exe");
        assert_eq!(cfg.sig_disp_offset, 3); // defaulted
        assert_eq!(cfg.sig_insn_len, 7);
        assert_eq!(cfg.walk.world_off().unwrap(), 0x148);
        assert_eq!(cfg.walk.resource_list_off().unwrap(), 0x338);
        assert_eq!(cfg.resources["food"], 2);
    }

    #[test]
    fn signature_parses_bytes_and_wildcards() {
        let p = parse_signature("48 8B 0D ?? ? 74").unwrap();
        assert_eq!(p[0], Some(0x48));
        assert_eq!(p[2], Some(0x0D));
        assert_eq!(p[3], None);
        assert_eq!(p[4], None);
        assert_eq!(p[5], Some(0x74));
        assert!(parse_signature("zz").is_err());
    }

    #[test]
    fn resource_crypt_round_trips() {
        let cfg = CaptureConfig::from_json(REAL).unwrap();
        let c = &cfg.resource_crypt;
        for v in [0.0f32, 200.0, 12345.5, 999999.0] {
            let enc = c.encrypt(v).unwrap();
            let dec = c.decrypt(enc).unwrap();
            assert_eq!(dec, v, "round-trip failed for {v}");
        }
    }

    #[test]
    fn rejects_all_wildcard_signature() {
        let mut v: serde_json::Value = serde_json::from_str(REAL).unwrap();
        v["gameInstanceSig"] = serde_json::json!("?? ?? ??");
        let cfg = CaptureConfig::from_json(&v.to_string()).unwrap();
        assert!(cfg.validate().unwrap_err().contains("all wildcards"));
    }

    #[test]
    fn rejects_unknown_sanity_resource() {
        let mut v: serde_json::Value = serde_json::from_str(REAL).unwrap();
        v["sanity"]["resource"] = serde_json::json!("ghost");
        let cfg = CaptureConfig::from_json(&v.to_string()).unwrap();
        assert!(cfg.validate().unwrap_err().contains("unknown resource"));
    }

    #[test]
    fn parses_hex_forms() {
        assert_eq!(parse_hex("0x148").unwrap(), 0x148);
        assert_eq!(parse_hex("338").unwrap(), 0x338);
        assert_eq!(parse_hex_u32("0x7BA9CCB8").unwrap(), 0x7BA9CCB8);
        assert!(parse_hex_u32("0x1_0000_0000").is_err());
    }
}
