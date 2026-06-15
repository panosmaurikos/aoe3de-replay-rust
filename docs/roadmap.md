# Roadmap — two modes

The project has two distinct capability modes. They differ in **data source**,
which decides what is even possible (`docs/replay-format.md`).

## Mode A — File-only replay analyzer (current focus)

Input: a `.age3Yrec` file. Source: recorded **player commands** only.

Scope (what a command log can honestly provide):

- Players / civs / teams / decks (metadata) — done
- Card / shipment **sends**, deck-matched + name-resolved — done (debug; normal
  behind `--experimental-shipments`)
- Research techs (commandId=1) — resolved, debug
- Train commands (commandId=2 train) — resolved, debug
- Build commands (commandId=3) — candidate, not yet confirmed
- Age-up — likely a research subtype, todo
- Resign, chat, inferred winner — done (normal)
- **Build order** + per-player command-derived state (`debug.playerStates`) — v0 done
- Cumulative **resources spent / military value produced** from issued commands ×
  unit/tech costs — todo (honest "spent/produced", never "current/lost")

Out of scope in this mode (not in the file — see `replay-model.md`):
deaths, losses, military/vill lost value, active unit counts, idle villagers,
current resources, score-over-time, unit positions.

Honesty rule: normal JSON carries only confirmed events; candidates stay in
`debug` or behind experimental flags.

### Near-term file-only tasks

1. Filter `unitsTrained` to real trainable units (drop props/buildings via
   `protoy` unit types) and de-duplicate double-click trains. — done
2. Promote research/train to verified normal events once 1-command = 1-event is
   confirmed (incl. the controlled test for ordering). — done; `--events` is now
   the **default** (opt out with `--no-events`).
3. Resources-spent / military-value-produced from costs. — done, incl. a
   military/economy/upgrades split and **per-event `cost`** in each
   research/train/build/age-up payload.
4. Buildings (commandId=3) + age-up. — done.
5. Build-order tab + per-timestamp state slider over issued events. — done
   (Build Order tab + Snapshot tab with a time scrubber showing each player's
   issued state as of T; Economy tab has a Total/Military/Economy/Upgrades
   metric toggle backed by `spentByCategorySeries`).
6. Command coverage: cmd79 layout resolved (no event emitted; verb unconfirmed),
   corpus validator at 100% decode coverage. — done.
7. Native desktop app (Tauri) wrapping the parser + viewer. — scaffolded and
   compiling (`src-tauri/`, `desktop.ps1`); installer bundling via `cargo tauri
   build` once the Tauri CLI is installed.

## Mode B — Runtime-assisted (CaptureAge-like, later)

Input: the **running game** (spectator / memory reading), optionally alongside the
replay file. Source: live simulation state.

Only this mode can provide the outcomes that the file lacks:

- Active unit counts, units in queue
- Unit deaths / losses, military lost + resource value, villagers lost + value
- Idle villagers
- Current resources, score-over-time
- Map / minimap, unit positions
- Tech currently applied to a unit

This is a separate, larger effort (a memory-reading or spectator-API layer) and is
explicitly **not** part of file-only parsing. The two modes share the game data
layer (`data/*.json`) and the event/state schema, so Mode A output is a strict
subset of Mode B.

## Milestones (file-only)

1. Correct shipments with names/icons — done
2. Build order from shipments/resign/chat + playerStates — in progress
3. Research tech events (normal) — next
4. Train + build events (normal) — next
5. Resources spent / military produced (derived from costs)
6. State timeline slider
7. AoE3-style UI (Mode A data)
8. Mode B (runtime-assisted) — future

## Commercial / community rules

- No official Microsoft assets bundled unless legally safe; icon keys + local
  asset import; fallback icons.
- Unofficial-project disclaimer in the UI.
- Normal JSON stays honest; debug tooling stays for reverse engineering.
