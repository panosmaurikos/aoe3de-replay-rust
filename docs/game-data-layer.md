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

`data/*.json`, keyed by the game `dbid`. `cards.json` and `icons.json` are
compiled into the binary with `include_str!`; `techs.json` / `units.json` /
`civs.json` are reference files. See `data/README.md` for schemas. Editing a data
file requires `cargo build`.

The data is generated from the MIT-licensed **aoe3-companion** set
(`data/THIRD_PARTY_LICENSES.md`):

```powershell
cargo run -- import-aoe3-companion --input "path\to\aoe3-companion" --out data
```

The importer resolves display names from the in-game English string table and
derives icon keys from icon paths.

## ⚠️ dbid is not the replay rawId

The data layer is keyed by **`dbid`** (the game's internal id, Capitalism =
`3438`). Replay decks and `commandId=2`/`66` use a **different** `rawId` space
(Capitalism shows as `1676` there). The `rawId → dbid` bridge is unsolved (~32%
coincidental overlap on real decks), so:

- `resolve-card <dbid>` resolves **game-data** ids.
- The replay path does **not** feed `rawId` into the data layer — that would
  produce wrong names. Replay shipment/deckMatch output stays numeric until the
  bridge is found. This is deliberate (correctness over a plausible-but-wrong
  name).

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

| Consumer            | What                                                  |
|---------------------|-------------------------------------------------------|
| `resolve-card` CLI  | look up a game `dbid` → name / internal / icon        |

The replay parser is intentionally **not** wired into the data layer yet, because
its card ids are `rawId`s, not `dbid`s (see above). `deckMatch` and
`--experimental-shipments` events carry the numeric `rawId` only. Once the
`rawId → dbid` bridge exists, the same `resolve_card` call will enrich them.

## Roadmap (data layer)

- **Done:** `import-aoe3-companion` — cards/techs/units/civs/icons from the
  aoe3-companion set, names from the English string table.
- **Next (blocker for replay enrichment): solve `rawId → dbid`.** Likely via a
  per-civ home-city card table (the companion `homecities/*.json` decks list card
  `@dbid` + tech name; the replay deck lists `rawId`s in a parallel order) or a
  direct game-file extraction that exposes both ids. Until then no replay card
  gets a name.
- Phase 6: `resolve-tech` / `resolve-unit` / `resolve-building` CLIs over the
  already-imported `techs.json` / `units.json` (and future `buildings.json` /
  `maps.json`).

## Licensing note

No copyrighted game asset is bundled in this repo. Real icons must be extracted
locally by the user. Commercial packaging needs care with Microsoft / AoE
content usage rules — ship custom placeholders or use user-local extracted
assets only.
