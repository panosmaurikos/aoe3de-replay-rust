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
- `shipment` (only with `--experimental-shipments`, always `status: "candidate"`):

```json
{
  "kind": "shipment",
  "rawCommandId": 2,
  "cardId": 1676,
  "deckIndex": 0,
  "confidence": "medium",
  "status": "candidate",
  "source": "command_stream+actor_deck_match+debug_command66_deck_setup",
  "note": "..."
}
```

`cardId` is the replay **`rawId`** (not the game-data `dbid`), so no card name is
attached yet — see `docs/game-data-layer.md`. `status` is `candidate` or
`confirmed`; `confidence` is `low` | `medium` | `high`. By default (no flag) there
are **no** shipment events.

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
`deckMatches[]`, optional `deckMatch`.

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
  "confidence": "medium"
}
```

`cardIdCandidate` is the replay `rawId`. Unmatched:

```json
{ "matched": false, "slotId": 6, "deckIndex": 2, "confidence": "low", "reason": "..." }
```

See `docs/reverse-engineering/shipments.md` for how `deckIndex` is resolved
against the actor's own active deck.

`debug.debugSummary`: `commandIds`, `unknownCommandIds`, `shipmentCandidateCount`
(counts `card_send_candidate` rows; the key keeps its legacy name).
