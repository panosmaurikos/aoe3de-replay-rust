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
| Age up                             | supported   | normal `age_up` events (`--events`) + viewer per-player age timings (II/III/IV/V) | research with the `AgeUpgrade` flag (politician / Chinese wonder); ordered per player to label the age |
| Train unit                         | supported   | normal `train` events (`--events`) | commandId=2 train, prop-filtered; some civs' military still missed |
| Build building                     | supported   | normal `build` events (`--events`) | commandId=3, building-filtered, deduped |
| Units trained (totals)             | supported   | `playerStates.unitsTrained` | prop/building-filtered (`units.json` `kind`) |
| Resources **spent** (gross)        | supported   | `playerStates.resourcesSpent` + `resourcesSpentSeries` | trains+builds+research × `cost`; food/wood/gold/influence; shipments excluded (paid in shipment pts). NOT current/net resources |
| Economy-pace chart                 | supported   | viewer Economy tab | cumulative spend over time per player, with a Total / Military / Economy / Upgrades metric toggle (from `resourcesSpentSeries` + `spentByCategorySeries`) |
| Military vs economy spend split     | supported   | `playerStates.spentByCategory` (+ `spentByCategorySeries`) + viewer card bar | military units / economy (villagers+buildings) / upgrades (research); from `units.json` `mil`. Reflects only decoded trains (some civs' military arrives as shipments → shows low) |
| Per-event cost                     | supported   | each `research`/`train`/`build`/`age_up` payload `cost` | eco cost of that single action (`{food?,wood?,gold?,influence?}`); per-event costs sum to gross spend; viewer shows cost badges |
| APM (actions per minute)           | supported   | `playerStates.apm` + `commandsTotal` + viewer card chip | over the player's active span; from the full command stream (honest action count, not raw clicks) |
| Stats export (JSON / CSV)          | supported   | viewer Export View (JSON) + Stats CSV | one CSV row per player: APM, counts, spend per resource + category, age timings |
| Active unit counts                 | impossible* | — | sim state, not in command replay |
| Units in queue                     | impossible* | — | sim state |
| Unit death / loss                  | impossible* | — | no "death command" exists; deaths are sim results |
| Military lost + resource value     | impossible* | — | needs deaths (sim state) |
| Villagers lost + resource value    | impossible* | — | needs deaths (sim state) |
| Idle villagers                     | impossible* | — | needs live unit state |
| Techs currently applied to a unit  | impossible* | — | live unit state |
| Build order tab                    | supported   | viewer Build Order tab + `debug.playerStates` | per-player age-up/research/train/build/shipment timeline, with per-action cost badges |
| State timeline slider              | supported   | viewer Snapshot tab | time scrubber; per-player state *as of* T (age, trained/built/researched/shipments, spend, mil/eco split, recent actions) from issued events. Live sim state is still impossible |

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

## Mode A status

Every Mode-A row above that is *possible* from a command replay is now
**supported**: shipments, research, train, build, age-up, units trained,
resources spent (+ series + per-event cost), military/economy/upgrades split
(+ series), economy chart with metric toggle, APM, build-order tab, the Snapshot
state-timeline scrubber, and JSON/CSV export — plus a native Tauri desktop app.
Command coverage is 100% on the corpus.

Accuracy caveat (not a missing feature, and not a decode gap):

- For some civs, part of the army does not appear in `unitsTrained` because it
  is **not trained at all** — it arrives via **shipments** (e.g. Chinese banner
  armies, some Ottoman/African military cards), which we already capture under
  `shipmentsSent`. So those units are counted, just as shipments rather than
  trains; `unitsTrained` and the military spend split are
  **incomplete-but-never-wrong** (they under-count trained military, never
  mis-attribute). This is a categorization nuance, not lost data.
- `command_id == 37` was once floated as a possible extra train path. **Ruled
  out:** it is the single most frequent command (up to ~56% of the stream), which
  is impossible for training (a game has a few hundred trains, not thousands). Its
  bytes are the unit-order shape (slot-attributed, `-1/-1` target fields, no
  protoId), so it is a **unit movement/order** command. We decode its layout and
  label it `unit_order` but emit no event — there is nothing to train here.

Not decodable from the file (out of scope; need Mode B live capture): deaths,
losses, active counts, idle vills, current resources — see the `impossible*`
rows above.
