# Game Data Layer

Canonical id → name / icon mappings. The parser emits **numeric ids only**; this
layer resolves them to display names and icon keys. Nothing here is hardcoded
inside the parser. See `docs/game-data-layer.md` for the architecture.

`cards.json` and `icons.json` are compiled into the binary via `include_str!`,
so `resolve-card` works with no external path. Editing a file requires a rebuild.

## ⚠️ Two different id spaces

- **`dbid` (game data id)** — what these files are keyed by. The game's internal
  database id, e.g. Capitalism = `3438`.
- **`rawId` (replay deck/card id)** — what appears in replay decks and
  `commandId=2`/`66`. A **different** space: e.g. the replay shows `1676` for what
  the player experiences as Capitalism, which has dbid `3438`.

The `rawId → dbid` bridge is **not solved yet** (only ~32% coincidental overlap on
real decks). So these files resolve game-data queries (`resolve-card <dbid>`) but
do **not** yet turn a replay card `rawId` into a name. Until the bridge exists,
replay shipment events stay numeric (`card #<rawId>`) — no guessed names. See
`docs/reverse-engineering/shipments.md`.

## Source & license

Imported from **aoe3-companion** (https://github.com/VitorRoda/aoe3-companion),
MIT licensed — see `data/THIRD_PARTY_LICENSES.md`. Regenerate with:

```powershell
cargo run -- import-aoe3-companion --input "path\to\aoe3-companion" --out data
```

The importer reads `techtreey.xml.json` (cards/techs), `protoy.xml.json` (units),
`civs.xml.json`, and `localization/stringtabley_en.json` (names), and writes the
compact files below. Display names are the in-game English strings.

Underlying card/unit names and stats are Age of Empires III game content
(© Microsoft). Only factual text/ids are stored here; **no copyrighted image
asset is bundled** — `iconKey`/`path` are references the user must satisfy with a
local extraction (Phase 2 / Phase 8).

## Files

| File             | Maps (key)                 | Entries | In binary |
|------------------|----------------------------|---------|-----------|
| `cards.json`     | home-city card `dbid` → def | ~2.5k   | yes (`include_str!`) |
| `techs.json`     | non-card tech `dbid` → def  | ~3.1k   | no (reference) |
| `units.json`     | proto unit `dbid` → def     | ~2.4k   | no (reference) |
| `civs.json`      | civ internal name → def     | 126     | no (reference) |
| `icons.json`     | icon key → `{ path }`       | ~3.3k   | yes (`include_str!`) |

## cards.json schema

Flat object keyed by the stringified `dbid`:

```json
{
  "3438": {
    "id": 3438,
    "internalName": "HCXPCapitalism",
    "displayName": "Capitalism",
    "type": "home_city_card",
    "iconKey": "card.Capitalism",
    "source": "aoe3_companion",
    "confidence": "imported"
  }
}
```

`id` and `displayName` are always present. `civilizations` (default `[]`) and
`age` (default `null`) are not yet derived by the importer.

### `confidence`

- `verified` — id → displayName confirmed by hand against in-game evidence
- `imported` — taken from the aoe3-companion data set (authoritative game files)
- `inferred` — correlated only (e.g. arrival-chat timing); do not treat as truth

### `source`

- `aoe3_companion` — produced by `import-aoe3-companion`
- `manual_verified` — added by hand
- `game_data` — produced by a future direct game-file extractor

## icons.json schema

```json
{
  "card.Capitalism": {
    "path": "resources/images/icons/techs/native/Capitalism.png",
    "source": "aoe3_companion",
    "fallback": false
  }
}
```

`path` is the in-game asset path (advisory). No PNG is bundled; if the file is
absent the viewer falls back to a generic card icon. Never crash on a missing
mapping or asset.
