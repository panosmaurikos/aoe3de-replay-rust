//! Mode B — runtime-assisted live capture (see `docs/mode-b-live-capture.md`).
//!
//! Reads live simulation state (current resources, age, ...) out of the running
//! `AoE3DE_s.exe` by **external read only** (`ReadProcessMemory`, the Cheat-Engine
//! model — no injection, no code execution in the game). The honest use case is
//! observing the deterministic *replay playback* of your own games and merging
//! the captured state with the Mode A command timeline.
//!
//! The memory model (AOB signature for the Game instance, struct-walk offsets,
//! and the resource decryption) is reverse-engineered from the open-source AoE3
//! DE Lua engine and lives entirely in [`config::CaptureConfig`] (data, not code),
//! so a game patch is a config edit.
//!
//! Layout:
//! - [`config`] — the model as data, platform-independent.
//! - this module — [`ProcessMemory`], the signature scanner, the instance/struct
//!   resolver, the resource decrypt, and the [`Sampler`]; all platform-independent
//!   and tested against an in-memory fake.
//! - `windows` / `stub` — the OS-specific [`ProcessMemory`] implementation.

pub mod config;

#[cfg(windows)]
mod windows;
#[cfg(not(windows))]
mod stub;

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

pub use config::{parse_signature, CaptureConfig, ResourceCrypt, SanityCheck, SigByte, WalkOffsets};

#[cfg(windows)]
pub use windows::WindowsProcess as PlatformProcess;
#[cfg(not(windows))]
pub use stub::StubProcess as PlatformProcess;

const PTR_SIZE: usize = 8;
/// Chunk size for scanning the module image, with overlap so a match can't be
/// split across chunk boundaries.
const SCAN_CHUNK: usize = 256 * 1024;

/// Read-only access to another process's address space.
pub trait ProcessMemory {
    /// Base address of the configured module in the target.
    fn module_base(&self) -> u64;
    /// Size of the module image (for bounding the signature scan).
    fn module_size(&self) -> u64;
    /// Read exactly `len` bytes at `addr`, or an error if unreadable.
    fn read_bytes(&self, addr: u64, len: usize) -> Result<Vec<u8>, String>;

    fn read_u64(&self, addr: u64) -> Result<u64, String> {
        let b = self.read_bytes(addr, PTR_SIZE)?;
        Ok(u64::from_le_bytes(b[..PTR_SIZE].try_into().unwrap()))
    }
    fn read_u32(&self, addr: u64) -> Result<u32, String> {
        let b = self.read_bytes(addr, 4)?;
        Ok(u32::from_le_bytes(b[..4].try_into().unwrap()))
    }
    fn read_i32(&self, addr: u64) -> Result<i32, String> {
        Ok(self.read_u32(addr)? as i32)
    }
}

/// Match `pattern` against `hay` starting at `i`. Wildcards (`None`) always match.
fn matches_at(hay: &[u8], i: usize, pattern: &[SigByte]) -> bool {
    pattern
        .iter()
        .enumerate()
        .all(|(j, p)| p.map_or(true, |b| hay.get(i + j) == Some(&b)))
}

/// Scan the module image `[base, base+size)` for `pattern`, returning the
/// absolute address of the first match. Reads in overlapping chunks and tolerates
/// unreadable regions (guard/unmapped pages) by skipping them.
pub fn scan_signature(
    mem: &dyn ProcessMemory,
    pattern: &[SigByte],
) -> Result<u64, String> {
    if pattern.is_empty() {
        return Err("empty signature".into());
    }
    let base = mem.module_base();
    let size = mem.module_size() as usize;
    let plen = pattern.len();
    let mut off = 0usize;
    while off < size {
        let want = SCAN_CHUNK.min(size - off);
        // Pull a little extra so a match straddling the chunk edge is still found.
        let read_len = (want + plen - 1).min(size - off);
        match mem.read_bytes(base + off as u64, read_len) {
            Ok(buf) => {
                let limit = buf.len().saturating_sub(plen - 1);
                for i in 0..limit {
                    if matches_at(&buf, i, pattern) {
                        return Ok(base + (off + i) as u64);
                    }
                }
            }
            Err(_) => { /* unreadable region — skip this chunk */ }
        }
        off += want;
    }
    Err("signature not found in module — offsets likely stale for this game \
         version, or the game isn't running"
        .into())
}

/// Resolve the Game-instance pointer from the signature: scan, follow the
/// RIP-relative `mov reg,[rip+disp32]` to the global pointer, then dereference.
pub fn resolve_game_instance(
    mem: &dyn ProcessMemory,
    cfg: &CaptureConfig,
) -> Result<u64, String> {
    let pattern = parse_signature(&cfg.game_instance_sig)?;
    let pattern_va = scan_signature(mem, &pattern)?;
    let disp = mem.read_i32(pattern_va + cfg.sig_disp_offset)? as i64;
    let global_ptr_va = (pattern_va as i64 + disp + cfg.sig_insn_len as i64) as u64;
    let game = mem.read_u64(global_ptr_va)?;
    if game == 0 {
        return Err("Game instance pointer is null — no match loaded yet".into());
    }
    Ok(game)
}

/// One player's captured live state at a sample.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PlayerState {
    pub slot: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub age: Option<i32>,
    /// resource name → current amount.
    pub resources: BTreeMap<String, f64>,
}

/// One sample across all players.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StateSample {
    #[serde(rename = "tMs")]
    pub t_ms: u64,
    pub players: Vec<PlayerState>,
}

/// A full Mode B capture, ready to merge with Mode A output.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LiveCapture {
    /// Source tag for auditability, e.g. `"memory:AoE3DE_s.exe@<version>"`.
    pub source: String,
    #[serde(rename = "gameVersion")]
    pub game_version: String,
    #[serde(rename = "sampleHz")]
    pub sample_hz: u32,
    pub samples: Vec<StateSample>,
}

/// One point in a per-player live series (merge-friendly, transposed view).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SeriesPoint {
    #[serde(rename = "tMs")]
    pub t_ms: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub age: Option<i32>,
    pub resources: BTreeMap<String, f64>,
}

/// A single player's live state over time.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PlayerSeries {
    pub slot: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    pub points: Vec<SeriesPoint>,
}

/// The merge artifact: a capture transposed to per-player series, ready to attach
/// to Mode A parsed-replay JSON under a `liveState` key.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LiveState {
    pub source: String,
    #[serde(rename = "gameVersion")]
    pub game_version: String,
    #[serde(rename = "sampleHz")]
    pub sample_hz: u32,
    /// Milliseconds added to each sample's tMs to align capture-start with the
    /// replay's game-clock zero (0 = capture began at playback start).
    #[serde(rename = "offsetMs")]
    pub offset_ms: i64,
    pub players: Vec<PlayerSeries>,
}

impl LiveCapture {
    /// Transpose samples into per-player series, shifting each time by `offset_ms`
    /// (negative results are clamped to 0). Players are ordered by slot; a name is
    /// carried from the last sample that had one.
    pub fn to_live_state(&self, offset_ms: i64) -> LiveState {
        use std::collections::BTreeMap as Map;
        let mut by_slot: Map<u32, PlayerSeries> = Map::new();
        for sample in &self.samples {
            let t = (sample.t_ms as i64 + offset_ms).max(0) as u64;
            for p in &sample.players {
                let entry = by_slot.entry(p.slot).or_insert_with(|| PlayerSeries {
                    slot: p.slot,
                    name: None,
                    points: Vec::new(),
                });
                if p.name.is_some() {
                    entry.name = p.name.clone();
                }
                entry.points.push(SeriesPoint {
                    t_ms: t,
                    age: p.age,
                    resources: p.resources.clone(),
                });
            }
        }
        LiveState {
            source: self.source.clone(),
            game_version: self.game_version.clone(),
            sample_hz: self.sample_hz,
            offset_ms,
            players: by_slot.into_values().collect(),
        }
    }
}

/// The sampling engine: resolves the game instance once, then reads each sample.
pub struct Sampler<'a> {
    cfg: &'a CaptureConfig,
    mem: &'a dyn ProcessMemory,
    game: u64,
}

impl<'a> Sampler<'a> {
    /// Attach the sampler by resolving the Game instance now.
    pub fn resolve(cfg: &'a CaptureConfig, mem: &'a dyn ProcessMemory) -> Result<Self, String> {
        let game = resolve_game_instance(mem, cfg)?;
        Ok(Self { cfg, mem, game })
    }

    /// For tests: build with a known game pointer.
    #[cfg(test)]
    fn with_game(cfg: &'a CaptureConfig, mem: &'a dyn ProcessMemory, game: u64) -> Self {
        Self { cfg, mem, game }
    }

    fn world(&self) -> Result<u64, String> {
        self.mem.read_u64(self.game + self.cfg.walk.world_off()?)
    }

    fn players_array(&self) -> Result<u64, String> {
        self.mem
            .read_u64(self.world()? + self.cfg.walk.players_off()?)
    }

    fn num_players(&self) -> Result<u32, String> {
        let n = self
            .mem
            .read_i32(self.world()? + self.cfg.walk.num_players_off()?)?;
        if n < 0 {
            return Err(format!("NumPlayers read negative ({n})"));
        }
        Ok((n as u32).min(self.cfg.max_players))
    }

    fn player_ptr(&self, slot: u32) -> Result<u64, String> {
        let arr = self.players_array()?;
        self.mem
            .read_u64(arr + slot as u64 * self.cfg.walk.player_stride()?)
    }

    /// Read and decrypt one resource for an already-resolved player pointer.
    fn resource(&self, player: u64, index: u32) -> Result<f32, String> {
        let reslist = self.mem.read_u64(player + self.cfg.walk.resource_list_off()?)?;
        let raw = self.mem.read_u32(reslist + 8 * index as u64)?;
        self.cfg.resource_crypt.decrypt(raw)
    }

    /// Verify the sanity resource for its configured slot is in range.
    pub fn check_sanity(&self) -> Result<(), String> {
        let Some(s) = &self.cfg.sanity else {
            return Ok(());
        };
        let index = *self
            .cfg
            .resources
            .get(&s.resource)
            .ok_or_else(|| format!("sanity resource '{}' missing", s.resource))?;
        let player = self
            .player_ptr(s.slot)
            .map_err(|e| format!("sanity: reading player {} failed: {e}", s.slot))?;
        let v = self
            .resource(player, index)
            .map_err(|e| format!("sanity read failed (is a game/replay running?): {e}"))?;
        if v < s.min || v > s.max {
            return Err(format!(
                "sanity check failed: {} for slot {} = {v} outside [{}, {}] — offsets/constants \
                 likely stale for this game version, or no match is in progress",
                s.resource, s.slot, s.min, s.max
            ));
        }
        Ok(())
    }

    fn read_name(&self, player: u64) -> Option<String> {
        let off = self.cfg.walk.player_name_off().ok().flatten()?;
        let str_ptr = self.mem.read_u64(player + off).ok()?;
        if str_ptr == 0 {
            return None;
        }
        // UTF-16, NUL-terminated; read a bounded window.
        let bytes = self.mem.read_bytes(str_ptr, 64).ok()?;
        let units: Vec<u16> = bytes
            .chunks_exact(2)
            .map(|c| u16::from_le_bytes([c[0], c[1]]))
            .take_while(|&u| u != 0)
            .collect();
        if units.is_empty() {
            return None;
        }
        Some(String::from_utf16_lossy(&units))
    }

    /// Capture a single sample at logical time `t_ms`.
    pub fn sample(&self, t_ms: u64) -> Result<StateSample, String> {
        let n = self.num_players()?;
        let mut players = Vec::with_capacity(n as usize);
        for slot in 0..n {
            let player = match self.player_ptr(slot) {
                Ok(p) if p != 0 => p,
                _ => continue, // empty slot
            };
            let mut resources = BTreeMap::new();
            for (name, &index) in &self.cfg.resources {
                resources.insert(name.clone(), self.resource(player, index)? as f64);
            }
            let age = self
                .cfg
                .walk
                .player_age_off()
                .ok()
                .flatten()
                .and_then(|off| self.mem.read_i32(player + off).ok());
            players.push(PlayerState {
                slot,
                name: self.read_name(player),
                age,
                resources,
            });
        }
        Ok(StateSample { t_ms, players })
    }

    pub fn source_tag(&self) -> String {
        format!("memory:{}@{}", self.cfg.process_name, self.cfg.game_version)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Fake address space: a flat byte vec mapped at `base`.
    struct FakeMem {
        base: u64,
        bytes: Vec<u8>,
    }
    impl FakeMem {
        fn new(base: u64, len: usize) -> Self {
            Self { base, bytes: vec![0u8; len] }
        }
        fn at(&mut self, addr: u64, data: &[u8]) {
            let off = (addr - self.base) as usize;
            self.bytes[off..off + data.len()].copy_from_slice(data);
        }
        fn ptr(&mut self, addr: u64, target: u64) {
            self.at(addr, &target.to_le_bytes());
        }
    }
    impl ProcessMemory for FakeMem {
        fn module_base(&self) -> u64 {
            self.base
        }
        fn module_size(&self) -> u64 {
            self.bytes.len() as u64
        }
        fn read_bytes(&self, addr: u64, len: usize) -> Result<Vec<u8>, String> {
            let off = addr.checked_sub(self.base).ok_or("below base")? as usize;
            self.bytes
                .get(off..off + len)
                .map(|s| s.to_vec())
                .ok_or_else(|| format!("oob read at {addr:#x}"))
        }
    }

    fn cfg() -> CaptureConfig {
        CaptureConfig::from_json(
            r#"{
            "gameVersion": "test",
            "processName": "AoE3DE_s.exe", "moduleName": "AoE3DE_s.exe",
            "gameInstanceSig": "48 8B 0D ?? ?? ?? ?? 80 3D",
            "walk": { "world": "0x148", "players": "0x98", "numPlayers": "0xA0",
                      "playerStride": "0x8", "resourceList": "0x338", "playerAge": "0x80" },
            "resourceCrypt": { "add": "0x7BA9CCB8", "xor": "0x86A4DFC9" },
            "resources": { "coin": 0, "wood": 1, "food": 2, "xp": 5 },
            "sanity": { "resource": "coin", "slot": 0, "min": 0.0, "max": 1000000.0 }
        }"#,
        )
        .unwrap()
    }

    #[test]
    fn scans_signature_across_chunks() {
        let base = 0x140_000_000u64;
        let mut mem = FakeMem::new(base, 0x1000);
        let pat = parse_signature("DE AD ?? EF").unwrap();
        mem.at(base + 0x555, &[0xDE, 0xAD, 0x99, 0xEF]);
        let found = scan_signature(&mem, &pat).unwrap();
        assert_eq!(found, base + 0x555);
    }

    #[test]
    fn resolves_rip_relative_instance() {
        // pattern at base+0x100; disp32 at +3 points to global ptr at base+0x800
        let base = 0x140_000_000u64;
        let mut mem = FakeMem::new(base, 0x2000);
        let pat_va = base + 0x100;
        mem.at(pat_va, &[0x48, 0x8B, 0x0D]); // mov rcx,[rip+disp]
                                             // disp = target - (pat_va + 7); target global at base+0x800
        let global = base + 0x800;
        let disp = (global as i64 - (pat_va as i64 + 7)) as i32;
        mem.at(pat_va + 3, &disp.to_le_bytes());
        mem.at(pat_va + 7, &[0x80, 0x3D]); // tail of signature
        let game = base + 0x1000;
        mem.ptr(global, game); // *global = game instance
        let resolved = resolve_game_instance(&mem, &cfg()).unwrap();
        assert_eq!(resolved, game);
    }

    #[test]
    fn samples_decrypted_resources_for_all_players() {
        let cfg = cfg();
        let crypt = &cfg.resource_crypt;
        let base = 0x10_000u64;
        let mut mem = FakeMem::new(base, 0x4000);
        let game = base + 0x100;
        let world = base + 0x400;
        let players = base + 0x800;
        let p0 = base + 0xA00;
        let p1 = base + 0xC00;
        let rl0 = base + 0xE00;
        let rl1 = base + 0xF00;
        mem.ptr(game + 0x148, world);
        mem.ptr(world + 0x98, players);
        mem.at(world + 0xA0, &2i32.to_le_bytes()); // NumPlayers = 2
        mem.ptr(players + 0x0, p0);
        mem.ptr(players + 0x8, p1);
        mem.ptr(p0 + 0x338, rl0);
        mem.ptr(p1 + 0x338, rl1);
        mem.at(p0 + 0x80, &2i32.to_le_bytes()); // age
        // plant decrypted values: coin=0,wood=1,food=2,xp=5 (8-byte stride)
        for (idx, v) in [(0u32, 500.0f32), (1, 150.0), (2, 300.0), (5, 42.0)] {
            mem.at(rl0 + 8 * idx as u64, &crypt.encrypt(v).unwrap().to_le_bytes());
        }
        for (idx, v) in [(0u32, 999.0f32), (1, 10.0), (2, 20.0), (5, 7.0)] {
            mem.at(rl1 + 8 * idx as u64, &crypt.encrypt(v).unwrap().to_le_bytes());
        }
        let sampler = Sampler::with_game(&cfg, &mem, game);
        sampler.check_sanity().unwrap();
        let s = sampler.sample(1000).unwrap();
        assert_eq!(s.t_ms, 1000);
        assert_eq!(s.players.len(), 2);
        assert_eq!(s.players[0].resources["coin"], 500.0);
        assert_eq!(s.players[0].resources["food"], 300.0);
        assert_eq!(s.players[0].age, Some(2));
        assert_eq!(s.players[1].resources["coin"], 999.0);
        assert_eq!(s.players[1].resources["xp"], 7.0);
    }

    #[test]
    fn transposes_capture_into_per_player_series_with_offset() {
        let mk = |t: u64, slot: u32, name: &str, coin: f64| StateSample {
            t_ms: t,
            players: vec![PlayerState {
                slot,
                name: Some(name.to_string()),
                age: Some(1),
                resources: BTreeMap::from([("coin".to_string(), coin)]),
            }],
        };
        let cap = LiveCapture {
            source: "memory:AoE3DE_s.exe@test".into(),
            game_version: "test".into(),
            sample_hz: 2,
            samples: vec![mk(0, 1, "Alice", 100.0), mk(500, 1, "Alice", 120.0)],
        };
        let ls = cap.to_live_state(2000);
        assert_eq!(ls.offset_ms, 2000);
        assert_eq!(ls.players.len(), 1);
        let p = &ls.players[0];
        assert_eq!(p.slot, 1);
        assert_eq!(p.name.as_deref(), Some("Alice"));
        assert_eq!(p.points.len(), 2);
        assert_eq!(p.points[0].t_ms, 2000); // 0 + offset
        assert_eq!(p.points[1].t_ms, 2500); // 500 + offset
        assert_eq!(p.points[1].resources["coin"], 120.0);
    }

    #[test]
    fn sanity_catches_garbage() {
        let cfg = cfg();
        let base = 0x10_000u64;
        let mut mem = FakeMem::new(base, 0x4000);
        let game = base + 0x100;
        let world = base + 0x400;
        let players = base + 0x800;
        let p0 = base + 0xA00;
        let rl0 = base + 0xE00;
        mem.ptr(game + 0x148, world);
        mem.ptr(world + 0x98, players);
        mem.at(world + 0xA0, &1i32.to_le_bytes());
        mem.ptr(players, p0);
        mem.ptr(p0 + 0x338, rl0);
        // coin decrypts to a wildly out-of-range value
        mem.at(rl0, &cfg.resource_crypt.encrypt(5_000_000.0).unwrap().to_le_bytes());
        let sampler = Sampler::with_game(&cfg, &mem, game);
        assert!(sampler.check_sanity().unwrap_err().contains("sanity check failed"));
    }
}
