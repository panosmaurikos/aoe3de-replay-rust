# Normalized JSON — schema v1

`schemaVersion: 1`. Top-level keys: `schemaVersion`, `timeline`, `summary`,
`result`, `replay`, and optional `debug` (only with `--debug-commands`).

The validator (`cargo run -- validate <json>`) enforces this shape.

## timeline.events[]

Common fields:

| field    | type   | notes |
|----------|--------|-------|
| `id`     | string | unique, `event-000001` style |
| `type`   | string | `chat` \| `resign` \| `shipment` |
| `time`   | int    | ms |
| `timeMs` | int    | ms (same value; events are sorted ascending) |
| `actor`  | object | see Actor model |
| `label`  | string | optional human label |
| `payload`| object | `kind` matches `type` |

Events are sorted by `(timeMs, typeOrder, sourceIndex)`.

### Actor model

```json
{ "kind": "player", "slotId": 2, "playerId": 2, "name": "George Washington" }
```

`kind` is `player` | `system` | `unknown`. System actors have null `slotId`.
Player actors must resolve to a known slot (validator checks this).

### Payloads

- `chat`: `{ "kind": "chat", "toId": int, "message": string }`
- `resign`: `{ "kind": "resign" }`
- gameplay events (only with `--events`): `research` / `age_up` (`{ kind, techId, name, iconKey?, confidence, source }`), `train` (`unitId`), `build` (`buildingId`). All carry `name` + `iconKey` resolved via the game data layer. `age_up` = a research whose tech has the `AgeUpgrade` flag.
- `shipment` (only with `--experimental-shipments`, always `status: "candidate"`):

```json
{
  "kind": "shipment",
  "rawCommandId": 2,
  "cardId": 1676,
  "deckIndex": 0,
  "cardName": "Capitalism",
  "iconKey": "card.capitalism",
  "confidence": "medium",
  "status": "candidate",
  "source": "command_stream+actor_deck_match+debug_command66_deck_setup",
  "note": "..."
}
```

`cardId` is the replay `rawId` (the techtree array index). `cardName` / `iconKey`
are present when it resolves to a known card (`docs/game-data-layer.md`). `status`
is `candidate` or `confirmed`; `confidence` is `low` | `medium` | `high`. By
default (no `--experimental-shipments` flag) there are **no** shipment events.

## summary

Counts mirrored from the timeline + replay: `eventCount`, `chatCount`,
`resignCount`, `shipmentCount`, `shipmentConfirmedCount`,
`shipmentCandidateCount`, `playerCount`, `teamCount`. The validator recomputes
and compares.

## result

Inferred winner: `{ inferred: bool, confidence, winningTeams: [], losingTeams: [], reason }`.
Inferred only from resign events vs team membership — never from chat.

## replay

`exeVersion`, `setting`, `players[]` (with `initialDecks[].cards[].rawId`),
`teams[]`. Players may be empty for replay layouts whose player block is not yet
decoded.

## debug (with `--debug-commands`)

`debug.commands[]` is the reverse-engineering layer. Each row: `offset`,
`timeMs`, `actor`, `commandId`, `commandName`, `decoded`, `length`,
`hexPreview`, `parsedAs`, `decodedFields`, `rawFields.{u16le,u32le}`, optional
`deckMatches[]`, optional `deckMatch`, optional `unit` (train, commandId=2) and
`tech` (research, commandId=1). `tech` (research, commandId=1), and `building` (build, commandId=3). `unit` /
`tech` / `building` are `{ id, displayName, iconKey, known }` resolved via the
game data layer (proto / techtree array index). These are debug-only; they never
enter the normal timeline.

### deckMatch (commandId=2 card sends)

```json
{
  "matched": true,
  "slotId": 2,
  "deckIndex": 0,
  "activeDeckId": 0,
  "deckName": "team 33",
  "cardIdCandidate": 1676,
  "source": "parsed_player_deck",
  "confidence": "medium",
  "card": { "cardId": 1676, "displayName": "Capitalism", "iconKey": "card.capitalism", "known": true }
}
```

`cardIdCandidate` is the replay `rawId`; `card` is present when it resolves to a
known card. Unmatched:

```json
{ "matched": false, "slotId": 6, "deckIndex": 2, "confidence": "low", "reason": "..." }
```

See `docs/reverse-engineering/shipments.md` for how `deckIndex` is resolved
against the actor's own active deck.

`debug.debugSummary`: `commandIds`, `unknownCommandIds`, `shipmentCandidateCount`
(counts `card_send_candidate` rows; the key keeps its legacy name).

## playerStates (state engine)

Top-level `playerStates[]` (with `--events` or `--debug-commands`). Per-player
aggregation of **command-derived** data only — what each player issued:

```json
{
  "slotId": 1, "name": "...", "civ": "Ottoman",
  "shipmentsSent":   [{ "timeMs": 91632, "id": 1676, "name": "Capitalism" }],
  "techsResearched": [{ "timeMs": 120000, "id": 410, "name": "Placer Mines" }],
  "unitsTrained":    [{ "name": "Janissary", "id": 1234, "count": 13 }],
  "buildingsBuilt":  [{ "timeMs": 200000, "id": 296, "name": "Barracks" }],
  "resourcesSpent":  { "food": 5795, "wood": 2650, "gold": 3700, "influence": 0, "total": 12145 },
  "spentByCategory": { "military": 2395, "economy": 1950, "upgrades": 7800 },
  "resourcesSpentSeries": [[31914, 150], [33000, 250], "..."],
  "counts": { "shipmentsSent": 9, "techsResearched": 12, "unitsTrainedTotal": 19, "buildingsBuilt": 14 },
  "unavailable": {
    "reason": "Not present in a command-only replay ...",
    "fields": ["activeUnits","unitsLost","militaryLostValue","resourceValueLost", "..."]
  }
}
```

`resourcesSpent` is **gross** eco spent on trains + builds + research
(`units.json`/`cards.json` `cost`); no refunds; shipments excluded (paid in
shipment points, not resources). It is NOT current/net resources. `unitsTrained`
is filtered to real
trainable units (buildings/props dropped via `units.json` `kind`) and
`shipmentsSent` is de-duplicated for double-clicks; some civs' military trains are
still missed (see `docs/overlay-features.md`). The `unavailable` fields (losses /
active counts / resources) are **not derivable** from a command replay
(`docs/replay-format.md`) and are honestly listed rather than guessed.
