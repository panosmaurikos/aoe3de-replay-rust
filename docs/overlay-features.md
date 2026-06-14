# Overlay feature support

Target: a CaptureAge-like AoE3 DE replay viewer. This tracks which target
features are **supported** (correct in normal JSON), **partial** (resolvable in
debug, not yet normal-JSON-safe), or **not decoded** yet.

Honesty rule: normal JSON contains only confirmed events. Anything candidate /
experimental stays in `debug` (with `--debug-commands`) or behind
`--experimental-shipments`.

**Reality check first:** `.age3Yrec` is a command replay — only player inputs +
per-tick checksums, no game state (`docs/replay-format.md`). So a whole class of
target features (losses, active counts, resources, idle vills) is **not derivable
from the file at all** — those need live game capture or full re-simulation. They
are not "not yet decoded"; they are not present. The state engine
(`debug.playerStates`, `player-summary` CLI) aggregates only what the player
*issued*.

| Feature                            | Status      | Where | Notes |
|------------------------------------|-------------|-------|-------|
| Players / civs / teams             | supported   | normal `replay` | from metadata |
| Decks / cards                      | supported   | normal `replay.players[].initialDecks` | rawId resolvable to name |
| Chat                               | supported   | normal timeline | |
| Resign + inferred winner           | supported   | normal timeline + `result` | from resign vs team membership |
| Card / shipment **send**           | supported   | normal `shipment` events (`--events`) | rawId→name, actor-deck matched, deduped |
| Research tech                      | supported   | normal `research` events (`--events`) | commandId=1, name-resolved, deduped |
| Age up                             | supported   | normal `age_up` events (`--events`) | research with the `AgeUpgrade` flag (politician / Chinese wonder) |
| Train unit                         | supported   | normal `train` events (`--events`) | commandId=2 train, prop-filtered; some civs' military still missed |
| Build building                     | supported   | normal `build` events (`--events`) | commandId=3, building-filtered, deduped |
| Units trained (totals)             | supported   | `playerStates.unitsTrained` | prop/building-filtered (`units.json` `kind`) |
| Resources **spent** (gross)        | supported   | `playerStates.resourcesSpent` + `resourcesSpentSeries` | trains+builds+research × `cost`; food/wood/gold/influence; shipments excluded (paid in shipment pts). NOT current/net resources |
| Economy-pace chart                 | supported   | viewer Economy tab | cumulative resources spent over time per player (from `resourcesSpentSeries`) |
| Active unit counts                 | impossible* | — | sim state, not in command replay |
| Units in queue                     | impossible* | — | sim state |
| Unit death / loss                  | impossible* | — | no "death command" exists; deaths are sim results |
| Military lost + resource value     | impossible* | — | needs deaths (sim state) |
| Villagers lost + resource value    | impossible* | — | needs deaths (sim state) |
| Idle villagers                     | impossible* | — | needs live unit state |
| Techs currently applied to a unit  | impossible* | — | live unit state |
| Build order tab                    | supported   | viewer Build Order tab + `debug.playerStates` | per-player age-up/research/train/build/shipment timeline |
| State timeline slider              | partial     | — | per-timestamp aggregation of issued events is feasible; live state is not |

`*` impossible = not present in a command-only replay; needs live game capture or
full re-simulation (`docs/replay-format.md`), not more decoding.

## Id resolution (game data layer)

All three replay id spaces are array indices into the companion game data
(`docs/game-data-layer.md`):

- card `rawId` → `cards.json[index]` (techtree array index) — **verified**
- research `techIdCandidate` (cmd 1) → `cards.json[index]` (techtree space) — **verified**
- train `unitProtoIdCandidate` (cmd 2) → `units.json[index]` (proto index) — **verified**
- build `protoIdCandidate` (cmd 3) → `units.json[index]`, `kind=building` — **verified**

`units.json` `kind` (`unit`/`building`/`other`) routes train vs build and drops
props. CLI: `resolve-card`, `resolve-tech`, `resolve-unit`, `resolve-building`.

Note: some civs train units the cmd2 variant misses (e.g. a villager-heavy
Chinese player whose military arrived as **shipments**, not trains) — that is
correct, not a gap.

## Next decoding targets (to move "partial" → "supported")

1. Done: `unitsTrained` filtered to real trainable units (`units.json` `kind`,
   from `populationcount` + unit types); `shipmentsSent` de-duplicated for
   double-clicks (trains are real, not deduped).
2. Recover the missing military trains for some civs (the cmd2 train variant does
   not catch every train); confirm one command = one event; then emit verified
   `research_tech` / `train_unit` into normal JSON.
3. Derive **cumulative resources spent / military value produced** from issued
   trains+techs+shipments × `protoy`/`techtreey` costs (honest "spent/produced",
   never "current" or "lost").
4. Decode buildings (commandId=3) and age-up; add to the state engine.
5. Per-timestamp state slider over issued events.

Not on this list (out of scope for static parsing): deaths, losses, active
counts, idle vills, current resources — see the `impossible*` rows above.
