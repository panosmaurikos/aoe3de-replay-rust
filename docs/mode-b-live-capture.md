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

## Proof-of-concept (first milestone)

Vertical slice that proves the pipeline end to end on **current resources**
(the simplest deep `impossible*` metric):

1. **`capture` subcommand / `mode_b` module (Rust, Windows-gated):** attach to
   `aoe3de` by process name, resolve module base, read a configured pointer chain,
   print a timestamped `StateSample` for each player at ~2 Hz. Pure external read.
2. **`data/offsets/<version>.json`:** pointer chains for `food/wood/coin/xp` per
   player slot. Bootstrapped from a public Cheat Engine table where possible, then
   verified live; if not, discovered with the guided Cheat-Engine procedure below.
3. **Verify:** play back a known replay, confirm captured resources move sensibly
   (rise with gather, drop on a known train/shipment) and cross-check the drops
   against Mode A's `resourcesSpentSeries` for the same replay at the same T.
4. **Merge + view:** emit the series in the analyzer JSON under a `liveState` key
   and add a viewer line so issued-spend and actual-resources show together.

### Offset discovery procedure (human-in-the-loop, one-time per patch)

The capture *harness* is fully buildable now; the pointer chains need the live
game once:

1. Launch AoE3 DE, start a skirmish or play back a replay.
2. In Cheat Engine: attach to `aoe3de.exe`, scan for a known resource value
   (e.g. starting gold), spend/gather to narrow, find the address.
3. Pointer-scan that address to get a **static module-base + offset chain** that
   survives a restart.
4. Repeat for wood/food/xp and the per-player stride (slots are usually a fixed
   array → one chain + a slot stride).
5. Drop the chains into `data/offsets/<version>.json`; the Rust harness reads them.

Once resources work, the same method extends to **pop, score (→ losses), idle
villagers, and the unit table (positions/counts)** — each is just another chain
in the same config, captured by the same loop.

## Roadmap within Mode B

1. Resource capture PoC (this doc) — current food/wood/coin/xp over time.
2. Population + score series → derive **military/villager lost value** from score
   deltas and the unit table.
3. Unit table walk → active counts, idle villagers, positions → **minimap**.
4. Observer-map (A) and screen-OCR (C) as alternative/fallback front-ends.
5. Live-spectate (non-replay) only if/when a safe, ToS-clear path exists.
