# Game Data Layer

The replay parser emits **numeric ids** (card ids, deck ids, later unit/tech/
building ids). It never knows or hardcodes display names or icons. A separate
game data layer maps those ids to names and icon keys.

```
Replay Parser  ->  ids/events
Game Data      ->  id -> displayName / iconKey
Decoder        ->  actor deck + cards.json verify event
Viewer/UI      ->  render using the data layer (graceful fallback)
```

This separation keeps the parser honest (it reports what the bytes say) and lets
the name/icon database grow independently from a better source later
(`import-game-data` from extracted game files, or a vetted community dataset)
without touching parser logic.

## Where the data lives

`data/*.json`. `cards.json` and `icons.json` are compiled into the binary with
`include_str!`; `units.json` / `civs.json` are reference files. See
`data/README.md` for schemas. Editing a data file requires `cargo build`.

The data is generated from the MIT-licensed **aoe3-companion** set
(`data/THIRD_PARTY_LICENSES.md`):

```powershell
cargo run -- import-aoe3-companion --input "path\to\aoe3-companion" --out data
```

The importer resolves display names from the in-game English string table and
derives icon keys from icon paths.

## Replay ids are array indices (the bridge)

Every replay id space is a **0-based array index** into the companion game data,
NOT the dbid:

| Replay id                       | Index into            | Resolves via   |
|---------------------------------|-----------------------|----------------|
| card `rawId` (decks, cmd 2/66)  | `techtree.tech[]`     | `cards.json`   |
| research `techIdCandidate` (cmd 1) | `techtree.tech[]`  | `cards.json`   |
| train `unitProtoIdCandidate` (cmd 2) | `proto.unit[]`   | `units.json`   |

Verified: `tech[1676]` = Capitalism (dbid `3438`); `proto[284]` = Settler,
`proto[928]` = Villager; `tech[410]` = Placer Mines. Full replay decks and
trained-unit / researched-tech streams resolve to civ-correct names that match
the in-game arrival chats across every civilization (Janissaries/Abus
Gunner→Ottoman, Bersaglieri→Italian, Confucius' Gift→Chinese, Maigadi→Hausa,
Bank of Rotterdam→Dutch, …). Each entry also carries its `dbid`.

Caveat: the index reflects the array order of the game version that produced the
companion dump. A replay from a very different patch could drift; the tested
corpus resolves at ~100% for cards and high for units/techs.

## Code

`src/gamedata.rs`:

- `GameData::embedded()` — load the compiled-in data; on a JSON parse error the
  affected map is left empty so resolution degrades instead of panicking.
- `GameData::resolve_card(id) -> CardRef` — never fails. Returns
  `{ cardId, displayName, iconKey, known }`. Unknown ids become
  `Unknown Card #<id>` with `iconKey = "card.generic"` and `known = false`.
- `GameData::card(id)` / `GameData::icon(key)` — raw definition lookups.

## Fallback rules

- Unknown card id → `Unknown Card #<id>`, generic icon, `known: false`.
- Card has no `iconKey` → generic icon key (`card.generic`).
- Icon key not in `icons.json`, or file missing on disk → viewer uses a generic
  card icon. Never crash on a missing mapping or asset.

## Where it is used today

| Consumer                                   | What                                              |
|--------------------------------------------|---------------------------------------------------|
| `resolve-card` / `resolve-unit` / `resolve-tech` CLI | id → name / internal / dbid / icon      |
| `debug.commands[*].deckMatch.card`         | card send (commandId=2) → resolved card           |
| `debug.commands[*].unit`                   | train unit (commandId=2) → resolved unit          |
| `debug.commands[*].tech`                   | research (commandId=1) → resolved tech            |
| `--experimental-shipments` timeline events | `payload.cardName`, `payload.iconKey`             |

These are present only when the id resolves to a known entry. Normal JSON (no
`--experimental-shipments`) still contains **no** shipment / train / research
events — only the debug layer is enriched by default.

## Roadmap (data layer)

- **Done:** `import-aoe3-companion` — cards/techs/units/civs/icons from the
  aoe3-companion set, names from the English string table.
- **Done:** all three id→entry bridges (card/tech = techtree index, unit = proto
  index), wired into `deckMatch`, `unit`, `tech` debug fields + experimental
  shipments; `resolve-card` / `resolve-unit` / `resolve-tech` CLIs.
- Next: isolate the commandId=2 train variant from non-train rows (some prop ids
  leak in) so train events become normal-JSON-safe; confirm commandId=1 actor /
  timing for `research_tech` events. See `docs/overlay-features.md`.
- Later: `civilizations` / `age` fields on cards; per-unit resource costs from
  `protoy` (for military/vill lost value); version-aware index handling.

## Licensing note

No copyrighted game asset is bundled in this repo. Real icons must be extracted
locally by the user. Commercial packaging needs care with Microsoft / AoE
content usage rules — ship custom placeholders or use user-local extracted
assets only.
