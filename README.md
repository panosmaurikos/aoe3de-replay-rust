# AoE3DE Replay Rust Parser

Rust CLI port of the TypeScript `aoe3de-replay-parser`. Parses Age of Empires 3: Definitive Edition `.age3Yrec` replay files into a normalized JSON timeline (chat, resigns, shipments) plus a reverse-engineering debug layer.

## Quick start

Requires Rust 1.75+ (`rustup` from https://rustup.rs).

```bash
git clone https://github.com/<user>/aoe3de-replay-rust
cd aoe3de-replay-rust
cargo build --release
cargo run --release -- parse "path/to/game.age3Yrec" -o game.parsed.json
```

Open `viewer/index.html` in a browser, click **Open JSON**, pick the output.

## Usage

```powershell
cargo run -- parse "D:\AGEOFEMPIRE3TEST\malloncheater.age3Yrec"
```

This writes:

```text
malloncheater.parsed.json
```

To choose the output path:

```powershell
cargo run -- parse "D:\AGEOFEMPIRE3TEST\malloncheater.age3Yrec" -o "D:\AGEOFEMPIRE3TEST\malloncheater.json"
```

To include command-level debug data for reverse engineering:

```powershell
cargo run -- parse "D:\AGEOFEMPIRE3TEST\malloncheater.age3Yrec" -o "D:\AGEOFEMPIRE3TEST\malloncheater.debug.json" --debug-commands
```

The JSON shape is:

```json
{
  "schemaVersion": 1,
  "timeline": {
    "events": [
      {
        "id": "event-000001",
        "type": "chat",
        "time": 42000,
        "timeMs": 42000,
        "actor": {
          "kind": "player",
          "slotId": 1,
          "playerId": 1,
          "name": "Player"
        },
        "payload": {
          "kind": "chat",
          "toId": -1,
          "message": "hello"
        }
      }
    ]
  },
  "summary": {
    "eventCount": 1,
    "chatCount": 1,
    "resignCount": 0,
    "shipmentCount": 0,
    "shipmentConfirmedCount": 0,
    "shipmentCandidateCount": 0,
    "playerCount": 2,
    "teamCount": 1
  },
  "result": {
    "inferred": true,
    "confidence": "medium",
    "winningTeams": [1],
    "losingTeams": [2],
    "reason": "All non-observer players from team(s) 2 resigned"
  },
  "replay": {}
}
```

If command parsing fails for an unknown replay layout, `timeline.events` is written as an empty list and `timeline.commandParseError` contains the parser error while replay metadata is still saved.

To convert an older parsed JSON file with `commands.chat` and `commands.resigns` into the stable timeline format:

```powershell
cargo run -- normalize ".\malloncheater.parsed.json" -o ".\malloncheater.parsed.json"
```

## Validation

Validate any normalized parsed JSON:

```powershell
cargo run -- validate ".\malloncheater.parsed.json"
```

Inspect command-level debug JSON:

```powershell
cargo run -- inspect-commands ".\out\game.debug.json" --from 155000 --to 165000
cargo run -- inspect-commands ".\out\game.debug.json" --command-id 37 --actor-slot 1 --limit 25
cargo run -- inspect-commands ".\out\game.debug.json" --parsed-as shipment_candidate --limit 25
cargo run -- inspect-commands ".\out\game.debug.json" --command-id 2 --full-hex --limit 5
cargo run -- compare-commands --a ".\out\a.debug.json" --a-offset 123456 --b ".\out\b.debug.json" --b-offset 456789
cargo run -- dump-decks ".\out\game.debug.json" --card-id 1676
```

Debug command records include `commandName`, `decodedFields`, `deckMatches`, and `rawFields.u16le/u32le` candidate values. These are the reverse-engineering layer used to turn raw commands into future gameplay events such as `shipment`, `train_unit`, or `research_tech`.

Parsed decks keep both the original `techIds` list and a stable `cards[]` list:

```json
{
  "deckName": "1v1",
  "deckId": 0,
  "cardCount": 25,
  "cards": [
    { "rawId": 1676 }
  ],
  "techIds": [1676]
}
```

Use `dump-decks` to check whether a candidate command field appears in a player's replay deck:

```powershell
cargo run -- dump-decks ".\target\malloncheater.debug.json" --slot 1
cargo run -- dump-decks ".\target\malloncheater.debug.json" --card-id 1676
```

Some replays currently parse team names but not full player metadata. In those files `replay.players` is empty, so parsed deck lookup cannot work until the player block is decoded for that replay layout. If the JSON includes debug commands, `dump-decks` also prints positive `commandId=66` deck setup candidates as a fallback.

The validator checks:

- `schemaVersion`
- `timeline.events` shape
- unique event ids
- ascending event time
- actor model rules: `player`, `system`, or `unknown`
- payload kind/type consistency
- `summary` counts against timeline and replay data
- `result` shape
- optional `debug.commands` and debug command id summaries

Example output:

```text
OK schemaVersion=1
OK events=484
OK chat=480
OK resign=4
OK shipment=1
OK shipmentConfirmed=1
OK shipmentCandidate=0
OK ids unique
OK events sorted by time
OK timeline events have valid actor model
OK no unknown actors
OK result shape valid
Validation passed with 0 warning(s)
```

## Viewer

Open `viewer/index.html` in a browser and choose a normalized JSON file with **Open JSON**.

The viewer includes:

- overview metrics
- players table
- likely winner and confidence
- event type filter
- confirmed shipment toggle
- player filter
- chat/player search
- filtered timeline export
- build order tab for confirmed/high-confidence gameplay events

If you want the **Load Sample** button to work, serve the repo directory with any static server first, for example:

```powershell
python -m http.server 8000
```

Then open:

```text
http://localhost:8000/viewer/
```

## Replay Corpus Loop

Keep a small replay set outside source control or under an ignored `samples/` directory:

```text
samples/
  1v1.age3Yrec
  2v2.age3Yrec
  ffa.age3Yrec
  treaty.age3Yrec
  long-game.age3Yrec
```

Then parse and validate each replay:

```powershell
cargo run -- parse ".\samples\1v1.age3Yrec" -o ".\out\1v1.parsed.json"
cargo run -- validate ".\out\1v1.parsed.json"
```

## Shipment Ownership

Shipments emitted into `timeline.events` are gated by deck matching:

- `commandId=2` is a card/shipment send candidate
- The candidate id must exist in the actor's own deck (parsed `replay.players[*].initialDecks` or `commandId=66` deck setup fallback)
- A nearby `*** has arrived` system chat is used only as a hint for `resolvedName` and to upgrade `confidence` to `high`

`debug.commands[*].deckMatch` records the resolution per `commandId=2`:

```json
{
  "matched": true,
  "slotId": 2,
  "cardIdCandidate": 1676,
  "source": "parsed_player_deck",
  "confidence": "medium"
}
```

Sources:

- `parsed_player_deck` — replay player block was decoded
- `debug_command66_deck_setup` — fallback when player decks were not parsed
