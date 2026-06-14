# Replay model: command/input log vs per-tick state log

## Hypothesis

`.age3Yrec` is primarily a **deterministic command/input replay**, not a full
per-tick state log. The game reproduces a match by re-simulating recorded player
inputs. Therefore simulation *outcomes* — unit deaths, losses, current resources,
score, unit positions, idle villagers — are **not stored in the file**; they only
exist while the game (or a re-simulation) runs.

If true, a file-only analyzer can recover what players *did* (commands) but not
the live game state. This is the same reason CaptureAge-style overlays attach to
the running game's memory.

## Verification tasks

1. Record a controlled `death_test.age3Yrec` with exactly one known unit death at
   a known time. ← **needs the user** (we cannot synthesize a real replay).
2. Parse with `--debug-commands`.
3. Inspect ±10 s around the known death time.
4. Compare against a no-death control replay.
5. Look for an explicit death/outcome command.
6. If none exists, mark deaths / losses / resource-lost / idle-villagers as
   **unavailable in file-only mode** (already done — `debug.playerStates.unavailable`).

### Controlled-test protocol (for the user to run)

Record two short replays in AoE3 DE:

- `control.age3Yrec` — start a skirmish, build nothing that dies, resign ~30 s in.
  No combat, no deaths.
- `death_test.age3Yrec` — same start, train exactly **one** unit, let exactly
  **one** unit die at a time you note (watch the clock), then resign.

Then:

```powershell
cargo run -- parse "control.age3Yrec"    -o control.debug.json    --debug-commands
cargo run -- parse "death_test.age3Yrec" -o death.debug.json      --debug-commands

# Did any command id appear only in the death replay?
cargo run -- compare-summaries --a control.debug.json --b death.debug.json

# Inspect the ±10 s window around the noted death time T (ms):
cargo run -- inspect-commands death.debug.json --from <T-10000> --to <T+10000>
```

Interpretation:

- If `compare-summaries` reports **no command id only in B**, and the ±10 s window
  contains only ordinary player commands (orders/train/etc.), the death is not
  recorded → hypothesis confirmed.
- If a new command id or a clearly death-shaped payload appears exactly at the
  death time, the hypothesis is wrong and we decode that event.

## Current evidence (existing replays, provisional)

Strong but provisional (pending the controlled test):

1. **Volume.** `malloncheater` is a 17.5-min 4v4 with heavy combat (hundreds–
   thousands of unit deaths) yet has only **6195 total commands** (~355/min).
   Per-death logging would produce one to two orders of magnitude more events.
2. **Attribution.** **Every** one of those 6195 commands has `actor.kind =
   player`. A death/outcome event would be engine-attributed (system / none), not
   player-issued. There are zero non-player commands.
3. **Structure.** The decompressed body (10-byte header, then raw DEFLATE → 53 MB
   for testship, ~44k ticks) is a sequence of: a marker, a **fixed 113-byte
   per-tick header whose payload is high-entropy** (values swing ±billions tick to
   tick — a desync checksum, not slow-moving state), then chat + player commands.
   A fixed-size header cannot hold a variable number of per-tick death records.
4. **Command ids** seen are all input-shaped: 0 = unit order/action (dominant),
   1 = research, 2 = train/card, 3 = proto action, 14 = shipment cancel,
   16 = resign, 66 = deck setup, etc. None scales with combat intensity.

Provisional conclusion: **command/input replay; no in-file death/outcome events.**
The controlled test is the remaining gold-standard confirmation.

## Consequence (enforced in code)

We do **not** emit `unit_died`, `military_lost`, `villager_lost`,
`resource_lost`, or `idle_villager` events anywhere. The state engine lists them
under `debug.playerStates.unavailable` with the reason that a command-only replay
cannot provide them. See `docs/replay-format.md` and `docs/overlay-features.md`.
