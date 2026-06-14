# Game Data Layer

Canonical id → name / icon mappings. The parser emits **numeric ids only**; this
layer resolves them to display names and icon keys. Nothing here is hardcoded
inside the parser. See `docs/game-data-layer.md` for the architecture.

`cards.json` and `icons.json` are compiled into the binary via `include_str!`,
so `resolve-card` works with no external path. Editing a file requires a rebuild.

## Card id = techtree array index

`cards.json` is keyed by the replay card **`rawId`**, which is the **0-based index
into the game's `techtree.tech[]` array** — NOT the dbid. Verified: index `1676`
= Capitalism (dbid `3438`), and full replay decks resolve to civ-correct cards
that match the in-game shipment-arrival chat lines across every civ. Each entry
also carries its `dbid` for cross-reference.

So `resolve-card <rawId>` and the replay deck resolver share one key space, and
`commandId=2` deck sends resolve to real card names/icons. Unknown ids degrade to
`Unknown Card #<id>`. See `docs/reverse-engineering/shipments.md`.

> Caveat: the index depends on the techtree order of the game version that
> produced the companion dump. A replay from a very different patch could be
> off; the corpus tested (DE 2023-era) resolves at ~100%.

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

| File             | Maps (key)                      | Entries | In binary |
|------------------|---------------------------------|---------|-----------|
| `cards.json`     | techtree index (= rawId) → def  | 5670    | yes (`include_str!`) |
| `units.json`     | proto unit `dbid` → def         | ~2.4k   | no (reference) |
| `civs.json`      | civ internal name → def         | 126     | no (reference) |
| `icons.json`     | icon key → `{ path }`           | ~3.3k   | yes (`include_str!`) |

`cards.json` holds every techtree row (cards and research techs); `type`
distinguishes `home_city_card` from `tech`. Replay decks reference card indices.
`units.json` entries carry a `kind` field (`unit` = trainable population unit,
`building`, or `other`), derived from `populationcount` + unit types, so the
train-unit decoder can drop buildings/props.

## cards.json schema

Flat object keyed by the stringified techtree index (= replay `rawId`):

```json
{
  "1676": {
    "id": 1676,
    "dbid": 3438,
    "internalName": "HCXPCapitalism",
    "displayName": "Capitalism",
    "type": "home_city_card",
    "iconKey": "card.capitalism",
    "source": "aoe3_companion",
    "confidence": "imported"
  }
}
```

The key and `id` are the techtree index (= replay `rawId`); `dbid` is the game
database id. `civilizations` and `age` are not yet derived by the importer.

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
  "card.capitalism": {
    "path": "resources/images/icons/techs/native/Capitalism.png",
    "source": "aoe3_companion",
    "fallback": false
  }
}
```

Icon keys are lowercased (case-only variants collapse to one); the `path` keeps
its original casing. `path` is the in-game asset path (advisory). No PNG is
bundled; if the file is absent the viewer falls back to a generic card icon.
Never crash on a missing mapping or asset.
