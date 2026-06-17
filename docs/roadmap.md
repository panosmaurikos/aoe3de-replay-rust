# Roadmap — two modes

The project has two distinct capability modes. They differ in **data source**,
which decides what is even possible (`docs/replay-format.md`).

## Mode A — File-only replay analyzer (feature-complete)

Status: the file-only analyzer is feature-complete. Parser decodes the command
stream at 100% coverage; the viewer has Timeline / Build Order (with per-action
costs) / Economy (Total + military/economy/upgrades metrics) / Snapshot (time
scrubber) tabs, player cards with APM + ages + spend split, JSON + stats-CSV
export, a native Tauri desktop app, and the unofficial-tool disclaimer. Further
work here is polish; new *capabilities* (map, losses, live state) require Mode B.


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

## Mode B — Runtime-assisted (CaptureAge-like) — *started*

Status: functional reader + merge landed (design in `docs/mode-b-live-capture.md`).
**External memory reading** (`ReadProcessMemory`, Cheat-Engine model — no
injection) of the running `AoE3DE_s.exe` during **replay playback** of your own
games. The reader reproduces the real game structures from the open-source AoE3 DE
Lua engine: AOB **signature scan** for the Game instance (ASLR-robust), struct
walk Game→World→Players→Player→ResourceList, and **resource decryption**
(`(u32+0x7BA9CCB8)^0x86A4DFC9` as f32). Every version-specific value is data in
`data/offsets/aoe3de.json` — **no Cheat Engine step needed**, and a patch is a
config edit. `capture` samples per-player resources+age to JSON (sanity gate
rejects stale offsets); `merge-capture` attaches the series to a parsed replay
under `liveState`. Pure logic (signature scan, RIP resolve, decrypt, struct walk,
transpose) is unit-tested against a fake address space; merge is smoke-tested
end-to-end. **Remaining:** validate against a live game (offsets/constants are
from the Lua-engine baseline and may need a refresh for the current build — the
sanity gate flags drift), then add a viewer "Live State" line and extend to
pop/score/units.

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
2. Build order from shipments/resign/chat + playerStates — done
3. Research tech events (normal) — done
4. Train + build events (normal) — done
5. Resources spent / military produced (derived from costs) — done
6. State timeline slider — done (Snapshot tab)
7. AoE3-style UI (Mode A data) — done (Campaign Ledger viewer + Tauri desktop app)
8. APM + stats CSV export + unofficial disclaimer — done
9. Mode B (runtime-assisted) — future (deferred; "Mode A now, Mode B later")

## Commercial / community rules

- No official Microsoft assets bundled unless legally safe; icon keys + local
  asset import; fallback icons.
- Unofficial-project disclaimer in the UI.
- Normal JSON stays honest; debug tooling stays for reverse engineering.
