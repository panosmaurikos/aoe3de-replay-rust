# Mode B — runtime-assisted live capture (design)

Mode A is feature-complete: it extracts everything a `.age3Yrec` *command*
replay can honestly provide (`docs/replay-format.md`, `docs/overlay-features.md`).
The remaining CaptureAge-class features — **current resources, population, score,
unit losses (military/villager lost value), idle villagers, active unit counts,
unit positions / minimap** — are *simulation state*, not in the file. They are
marked `impossible*` in `overlay-features.md` **for the file alone**. Mode B adds
the only honest source of that state: the **running game**.

This doc decides *how* we read live state, the legal/robustness tradeoffs, the
bridge back to the Mode A schema, and the first proof-of-concept.

## The honest use case

We are not building a live-ranked advantage tool. The target workflow mirrors
what CaptureAge does for AoE2/4 — **spectate / play back a game and observe its
state**:

1. The user opens one of *their own* `.age3Yrec` replays **inside AoE3 DE's own
   replay playback** (deterministic re-simulation of the recorded commands).
2. Mode B reads the live simulation state out of the running game process as it
   plays.
3. We timestamp each sample and **merge it with the Mode A command timeline** for
   the same replay — commands (what was issued) + state (what actually happened).

Because playback is deterministic, the same replay always produces the same
state, so a capture is reproducible and can be re-run to fill gaps. This keeps
the project's honesty rule intact: Mode A output stays a strict subset of Mode B,
and every Mode B number is sourced from real simulation state, never guessed.

> Note on official access: CaptureAge for AoE2/AoE4 is an **official World's Edge
> partnership** with an in-game "Spectate with CaptureAge" hook. No such API is
> exposed for AoE3 DE, so we must read state ourselves.

## Candidate approaches

### A — In-game scripting telemetry (XS / Lua mod)

Use the game's *own* scripting (XS triggers, and/or the community Lua engine) from
inside a mod or observer map to read state via the documented trigger API and
write it out (file / localhost socket); the analyzer consumes that stream.

- **Pros:** uses the game's own API (most "legitimate"); stable field meanings;
  no offsets to chase.
- **Cons:** XS only exposes what the trigger system surfaces (resources, some
  counts — *not* arbitrary per-unit data or positions); typically needs a
  **custom/observer map** (Aizamk's Observer UI Mod works only on maps edited for
  it), so it does **not** apply to arbitrary ranked replays; writing telemetry out
  of the sandbox each tick is awkward.
- **Verdict:** good for a curated observer-map experience later; too constrained
  to be the general capture path.

### B — External process memory reading  ⟵ recommended first

Read the running `aoe3de` process from a separate program with
`OpenProcess` + `ReadProcessMemory` (the **Cheat-Engine model**: pure external
read, **no DLL injection, no code execution in the game**). Resolve a static
**module-base + pointer-chain → field** per value (resources, pop, score, per-unit
table).

- **Feasibility is proven:** working Cheat Engine tables for *AoE3 DE* exist
  publicly (FearlessRevolution), i.e. the values are reachable and pointer chains
  are discoverable. The game ships **no kernel/EAC anti-cheat**.
- **Pros:** map-agnostic (works on any replay played back); can reach deep state
  (per-unit arrays, positions) that scripting can't; no game modification; Rust
  can do it natively on Windows (`OpenProcess`/`ReadProcessMemory`/module base via
  Toolhelp32).
- **Cons:** **offsets break on game patches** → must live in editable config and
  be re-discovered per version; pointer-chain discovery needs the live game
  (human-in-the-loop with Cheat Engine, see PoC); reading another process is
  Windows-specific and a bit fiddly (chunked reads, bad pointers).
- **Risk:** for **replay playback / single-player** this is benign (same class as
  Cheat Engine reads, which the game tolerates). We will **not** attach during
  live ranked multiplayer; the tool targets replay/spectate only, and the UI will
  say so.

### C — Observer-UI screen capture + OCR/CV

Capture the AoE3 DE window during playback and OCR the resource/score/pop numbers
the game already renders; template-match civ/age icons.

- **Pros:** zero game access (fully ToS-safe); robust to memory layout changes.
- **Cons:** only what's on screen (no positions, no per-unit data, no off-screen
  players); OCR is noisy; needs a fixed UI layout/resolution; fights fog-of-war.
- **Verdict:** useful fallback / cross-check, not the primary depth path.

## Decision

**Start with B (external memory reading), replay-playback only.** It is proven
feasible on this exact game, needs no game modification, is map-agnostic, and is
the only path that scales to the deep CaptureAge-class data. A and C remain on
the table as a curated-observer mode and a ToS-safe fallback respectively.

## Architecture (Mode B slice)

```
 aoe3de.exe (replaying foo.age3Yrec)
        │  ReadProcessMemory (external, read-only)
        ▼
 capture engine (Rust, Windows)
   - attach: find pid, module base
   - offsets.json: { version, pointer chains per field }
   - sampler loop @ N Hz → StateSample { t, players[].{food,wood,coin,xp,pop,...} }
        │  JSON lines / live socket
        ▼
 analyzer merge: align StateSample series to Mode A command timeline (same replay)
        │
        ▼
 viewer: new "Live State" series next to issued-state series
```

- **Offsets as data, not code:** `data/offsets/<gameVersion>.json` holds each
  field's `{ module, base, chain: [..], type }`. Patch breaks → swap the file,
  not the binary. `capture` refuses to run if it can't verify a sanity field
  (e.g. local player's gold within plausible bounds) to avoid emitting garbage
  after a patch.
- **Schema bridge:** live samples reuse Mode A field names where they overlap and
  add a `live: true` marker so the viewer can show *current* vs *issued/spent*
  side by side without ever conflating them.
- **Honesty:** Mode B JSON is a superset; absence of a Mode B capture leaves Mode
  A output byte-identical. Every live field is tagged with its source
  (`memory`/version) for auditability.

## Implementation (the real memory model — no Cheat Engine needed)

Rather than hand-discover fragile static pointer chains, the reader reproduces the
**actual game structures** documented by the open-source AoE3 DE Lua engine
([github.com/SystematicSkid/Age-of-Empires-3-DE-Lua], `Engine/Addresses.h`,
`Engine/Classes/Player/Player.h`). That makes it robust (signature-anchored, not
ASLR-fragile) and removes the manual offset-scan step. Every version-specific
value lives in `data/offsets/aoe3de.json` (`mode_b::config::CaptureConfig`).

Key facts the engine source gave us:

- **Process is `AoE3DE_s.exe`** (the simulation worker), not the `aoe3de.exe`
  launcher.
- The **Game instance** is found by an **AOB signature scan** of the module, then
  a RIP-relative resolve: for `mov rcx,[rip+disp32]`
  (`48 8B 0D ?? ?? ?? ?? ...`), `globalPtr = patternVA + disp32 + 7`, and
  `game = *globalPtr`. Signature scanning survives ASLR and minor patches.
- **Struct walk:** `game+0x148 → World`; `World+0x98 → Players` (Player\*\* array),
  `World+0xA0 → NumPlayers`; `players[i]` → Player; `player+0x338 → ResourceList`.
- **Resources are obfuscated in memory.** A plain Cheat-Engine float scan would
  never find them. Decrypt each as:
  `bits = (read_u32(ResourceList + 8*index) + 0x7BA9CCB8) ^ 0x86A4DFC9`, then
  `value = f32::from_bits(bits)`. Resource index: Gold=0, Wood=1, Food=2, Fame=3,
  SkillPoints=4, XP=5, Ships=6, Trade=7.

The capture loop (`Sampler`) resolves the Game instance once, then each tick walks
to every player and reads + decrypts the configured resources (and age), emitting
a `StateSample`. A **sanity gate** decrypts a known resource for one slot and
refuses to run if it's wildly out of range — so stale offsets after a patch
produce a clear error, never silent garbage.

### How to run it

```
# 1. Launch AoE3 DE and start a skirmish or play back YOUR replay.
# 2. Capture (writes a per-player resource time series):
cargo run --release -- capture --offsets data/offsets/aoe3de.json --hz 2 --duration 600 -o cap.json
# 3. Merge with the parsed replay timeline:
cargo run --release -- merge-capture --replay game.parsed.json --capture cap.json -o game.live.json
```

If `capture` reports "signature not found" or a failed sanity check, the offsets
have **drifted with a game patch**. Fix = edit `data/offsets/aoe3de.json` (no
recompile): re-derive the signature/offsets/decrypt constants from an updated
build of the Lua engine source, or by inspecting the running process. Pop count,
score, and the unit table (positions/losses) are the same pattern — additional
offsets in the same config, read by the same loop.

### Verifying correctness against Mode A

Because replay playback is deterministic, captured drops in a resource should line
up with Mode A's issued spend: at time T the **decrease** in actual resources
(minus gather income) is bounded by `resourcesSpentSeries`. A train/shipment we
already decode should coincide with a dip in the live series — a free cross-check
that the decryption and offsets are right.

## Roadmap within Mode B

1. Resource capture PoC (this doc) — current food/wood/coin/xp over time.
2. Population + score series → derive **military/villager lost value** from score
   deltas and the unit table.
3. Unit table walk → active counts, idle villagers, positions → **minimap**.
4. Observer-map (A) and screen-OCR (C) as alternative/fallback front-ends.
5. Live-spectate (non-replay) only if/when a safe, ToS-clear path exists.
