//! Mode B — runtime-assisted live capture (see `docs/mode-b-live-capture.md`).
//!
//! Reads live simulation state (current resources, population, ...) out of a
//! running `aoe3de` process **by external read only** (`ReadProcessMemory`, the
//! Cheat-Engine model — no injection, no code execution in the game). The honest
//! use case is observing the deterministic *replay playback* of your own games,
//! then merging the captured state with the Mode A command timeline.
//!
//! Layout:
//! - [`config`] — pointer chains as data (`OffsetConfig`), platform-independent.
//! - this module — the [`ProcessMemory`] trait, the pointer-chain resolver, and
//!   the sampling loop ([`Sampler`]). All platform-independent and testable
//!   against an in-memory fake.
//! - `windows` / `stub` — the OS-specific [`ProcessMemory`] implementation.

pub mod config;

#[cfg(windows)]
mod windows;
#[cfg(not(windows))]
mod stub;

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

pub use config::{FieldChain, FieldType, OffsetConfig, SanityCheck};

#[cfg(windows)]
pub use windows::WindowsProcess as PlatformProcess;
#[cfg(not(windows))]
pub use stub::StubProcess as PlatformProcess;

/// Pointer width of the target. AoE3 DE is x64.
const PTR_SIZE: usize = 8;

/// Read-only access to another process's address space.
///
/// Implemented per-OS; the resolver and sampler are written entirely against
/// this trait so they can be unit-tested with a fake.
pub trait ProcessMemory {
    /// Base address of the configured module in the target process.
    fn module_base(&self) -> u64;
    /// Read exactly `len` bytes at `addr`, or an error if the region is unreadable.
    fn read_bytes(&self, addr: u64, len: usize) -> Result<Vec<u8>, String>;

    /// Read a little-endian pointer (`PTR_SIZE` bytes) at `addr`.
    fn read_ptr(&self, addr: u64) -> Result<u64, String> {
        let b = self.read_bytes(addr, PTR_SIZE)?;
        Ok(u64::from_le_bytes(b[..PTR_SIZE].try_into().unwrap()))
    }
}

/// Resolve a field chain to the final address of player `slot`, applying the
/// player stride at the end. Returns the address to read the value from.
pub fn resolve_address(
    mem: &dyn ProcessMemory,
    chain: &FieldChain,
    slot: u32,
    player_stride: u64,
) -> Result<u64, String> {
    let mut addr = mem
        .module_base()
        .checked_add(chain.base_offset()?)
        .ok_or("address overflow at base")?;
    // Walk pointers: deref, add offset.
    for off in chain.chain_offsets()? {
        addr = mem
            .read_ptr(addr)?
            .checked_add(off)
            .ok_or("address overflow in chain")?;
    }
    // Player slots are a flat array at the resolved field address.
    addr = addr
        .checked_add(slot as u64 * player_stride)
        .ok_or("address overflow at player stride")?;
    Ok(addr)
}

/// Read one field's decoded value for one player slot.
pub fn read_field(
    mem: &dyn ProcessMemory,
    chain: &FieldChain,
    slot: u32,
    player_stride: u64,
) -> Result<f64, String> {
    let addr = resolve_address(mem, chain, slot, player_stride)?;
    let bytes = mem.read_bytes(addr, chain.field_type.width())?;
    chain.field_type.decode(&bytes)
}

/// One player's captured live state at a single sample time.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PlayerState {
    pub slot: u32,
    /// field name → live value (resources/pop/...). Ordered for stable JSON.
    pub fields: BTreeMap<String, f64>,
}

/// One sample across all players.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StateSample {
    /// Milliseconds since capture start.
    #[serde(rename = "tMs")]
    pub t_ms: u64,
    pub players: Vec<PlayerState>,
}

/// A full Mode B capture, ready to merge with Mode A output.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LiveCapture {
    /// Source tag for auditability, e.g. `"memory:aoe3de.exe@100.15.0"`.
    pub source: String,
    #[serde(rename = "gameVersion")]
    pub game_version: String,
    #[serde(rename = "sampleHz")]
    pub sample_hz: u32,
    pub samples: Vec<StateSample>,
}

/// The sampling engine: holds the config and a `ProcessMemory`, produces samples.
pub struct Sampler<'a> {
    cfg: &'a OffsetConfig,
    mem: &'a dyn ProcessMemory,
}

impl<'a> Sampler<'a> {
    pub fn new(cfg: &'a OffsetConfig, mem: &'a dyn ProcessMemory) -> Self {
        Self { cfg, mem }
    }

    /// Verify the sanity field for slot 0 is in range. Call once before sampling
    /// so a wrong post-patch chain errors instead of emitting garbage.
    pub fn check_sanity(&self) -> Result<(), String> {
        let Some(s) = &self.cfg.sanity else {
            return Ok(());
        };
        let chain = self
            .cfg
            .fields
            .get(&s.field)
            .ok_or_else(|| format!("sanity field '{}' missing", s.field))?;
        let v = read_field(self.mem, chain, 0, self.cfg.player_stride)
            .map_err(|e| format!("sanity read failed (is a game/replay running?): {e}"))?;
        if v < s.min || v > s.max {
            return Err(format!(
                "sanity check failed: {} = {v} is outside [{}, {}] — offsets likely stale for this \
                 game version, or no match is in progress",
                s.field, s.min, s.max
            ));
        }
        Ok(())
    }

    /// Capture a single sample at logical time `t_ms`.
    pub fn sample(&self, t_ms: u64) -> Result<StateSample, String> {
        let mut players = Vec::with_capacity(self.cfg.player_count as usize);
        for slot in 0..self.cfg.player_count {
            let mut fields = BTreeMap::new();
            for (name, chain) in &self.cfg.fields {
                let v = read_field(self.mem, chain, slot, self.cfg.player_stride)?;
                fields.insert(name.clone(), v);
            }
            players.push(PlayerState { slot, fields });
        }
        Ok(StateSample { t_ms, players })
    }

    pub fn source_tag(&self) -> String {
        format!(
            "memory:{}@{}",
            self.cfg.process_name, self.cfg.game_version
        )
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
            Self {
                base,
                bytes: vec![0u8; len],
            }
        }
        fn write(&mut self, addr: u64, data: &[u8]) {
            let off = (addr - self.base) as usize;
            self.bytes[off..off + data.len()].copy_from_slice(data);
        }
    }
    impl ProcessMemory for FakeMem {
        fn module_base(&self) -> u64 {
            self.base
        }
        fn read_bytes(&self, addr: u64, len: usize) -> Result<Vec<u8>, String> {
            let off = addr
                .checked_sub(self.base)
                .ok_or("read below base")? as usize;
            self.bytes
                .get(off..off + len)
                .map(|s| s.to_vec())
                .ok_or_else(|| format!("read out of range at {addr:#x}"))
        }
    }

    fn chain(base: &str, offs: &[&str], ty: FieldType) -> FieldChain {
        FieldChain {
            base: base.into(),
            chain: offs.iter().map(|s| s.to_string()).collect(),
            field_type: ty,
        }
    }

    #[test]
    fn resolves_direct_value_no_chain() {
        let base = 0x1000;
        let mut mem = FakeMem::new(base, 0x100);
        mem.write(base + 0x20, &123.0f32.to_le_bytes());
        let fc = chain("0x20", &[], FieldType::F32);
        let v = read_field(&mem, &fc, 0, 0).unwrap();
        assert_eq!(v, 123.0);
    }

    #[test]
    fn follows_pointer_chain() {
        // moduleBase+0x10 -> ptr A; A+0x8 -> value
        let base = 0x10_000;
        let mut mem = FakeMem::new(base, 0x1000);
        let a = base + 0x200;
        mem.write(base + 0x10, &a.to_le_bytes()); // pointer at base+0x10
        mem.write(a + 0x8, &777i32.to_le_bytes()); // value at A+0x8
        let fc = chain("0x10", &["0x8"], FieldType::I32);
        assert_eq!(read_field(&mem, &fc, 0, 0).unwrap(), 777.0);
    }

    #[test]
    fn applies_player_stride() {
        let base = 0x20_000;
        let mut mem = FakeMem::new(base, 0x1000);
        let stride = 0x40u64;
        // slot 0 at base+0x100, slot 1 at base+0x140
        mem.write(base + 0x100, &10.0f32.to_le_bytes());
        mem.write(base + 0x100 + stride, &20.0f32.to_le_bytes());
        let fc = chain("0x100", &[], FieldType::F32);
        assert_eq!(read_field(&mem, &fc, 0, stride).unwrap(), 10.0);
        assert_eq!(read_field(&mem, &fc, 1, stride).unwrap(), 20.0);
    }

    #[test]
    fn sampler_reads_all_players_and_fields() {
        let base = 0x30_000;
        let mut mem = FakeMem::new(base, 0x1000);
        let stride = 0x10u64;
        mem.write(base + 0x40, &100.0f32.to_le_bytes()); // coin slot0
        mem.write(base + 0x40 + stride, &200.0f32.to_le_bytes()); // coin slot1
        let mut fields = BTreeMap::new();
        fields.insert("coin".to_string(), chain("0x40", &[], FieldType::F32));
        let cfg = OffsetConfig {
            game_version: "test".into(),
            process_name: "aoe3de.exe".into(),
            module_name: "aoe3de.exe".into(),
            player_count: 2,
            player_stride: stride,
            fields,
            sanity: Some(SanityCheck {
                field: "coin".into(),
                min: 0.0,
                max: 1_000.0,
            }),
        };
        let sampler = Sampler::new(&cfg, &mem);
        sampler.check_sanity().unwrap();
        let s = sampler.sample(500).unwrap();
        assert_eq!(s.t_ms, 500);
        assert_eq!(s.players.len(), 2);
        assert_eq!(s.players[0].fields["coin"], 100.0);
        assert_eq!(s.players[1].fields["coin"], 200.0);
    }

    #[test]
    fn sanity_check_catches_out_of_range() {
        let base = 0x40_000;
        let mut mem = FakeMem::new(base, 0x100);
        mem.write(base + 0x8, &9_999_999.0f32.to_le_bytes());
        let mut fields = BTreeMap::new();
        fields.insert("coin".to_string(), chain("0x8", &[], FieldType::F32));
        let cfg = OffsetConfig {
            game_version: "test".into(),
            process_name: "aoe3de.exe".into(),
            module_name: "aoe3de.exe".into(),
            player_count: 1,
            player_stride: 0,
            fields,
            sanity: Some(SanityCheck {
                field: "coin".into(),
                min: 0.0,
                max: 1_000.0,
            }),
        };
        let sampler = Sampler::new(&cfg, &mem);
        assert!(sampler.check_sanity().unwrap_err().contains("sanity check failed"));
    }
}
