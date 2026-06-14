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

## One-command analysis (Windows)

Parse a replay and open the result in your default browser in one step:

```powershell
.\analyze.ps1 "D:\AGEOFEMPIRE3TEST\testship.age3Yrec"
```

Or drag a `.age3Yrec` file onto `analyze.cmd`.

### Drop-zone app

Start `AoE3 Analyzer.cmd` (desktop shortcut: **AoE3 Replay Analyzer**). A small window opens; drop one or more `.age3Yrec` files into it (or click **Browse...**) and each replay is parsed and opened in the browser. The checkbox adds `--debug-commands` output to the JSON.

Options: `-DebugCommands` (include the reverse-engineering command stream), `-NoShipments` (skip experimental shipment events), `-NoBrowser` (generate files only). Output goes to `target\analyze\<name>.json` and a self-contained `target\analyze\<name>.html` (viewer with the JSON embedded — no server or file picker needed).

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

To emit verified command-derived gameplay events (shipments, research, train, build, age-up) into the normal timeline:

```powershell
cargo run -- parse "D:\AGEOFEMPIRE3TEST\malloncheater.age3Yrec" -o ".\malloncheater.json" --events
```

Event types: `chat`, `resign`, `shipment`, `research`, `train`, `build`, `age_up`. The drop-zone app and `analyze.ps1` use `--events` by default, so the viewer's **Build Order** tab shows each player's full action timeline. (`--experimental-shipments` still works for shipments only.)

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
cargo run -- inspect-commands ".\out\game.debug.json" --parsed-as card_send_candidate --limit 25
cargo run -- inspect-commands ".\out\game.debug.json" --command-id 2 --full-hex --limit 5
cargo run -- inspect-card-commands ".\out\game.debug.json" --actor-slot 2
cargo run -- compare-commands --a ".\out\a.debug.json" --a-offset 123456 --b ".\out\b.debug.json" --b-offset 456789
cargo run -- compare-summaries --a ".\out\control.debug.json" --b ".\out\death.debug.json"
cargo run -- dump-decks ".\out\game.debug.json" --card-id 1676
cargo run -- player-summary ".\out\game.debug.json"
cargo run -- resolve-card --card-id 1676
cargo run --release -- validate-corpus "D:\AGEOFEMPIRE3TEST"
```

`validate-corpus <dir>` parses and validates every `.age3Yrec` under a directory
(recursively) and reports a QA summary — per file: pass/fail, event count, command
decode coverage, warnings; plus the unclassified command ids across the corpus. It
never panics on a bad file. The current test corpus (Dutch/Ethiopia/Russia/USA/8-civ,
2–8 players, up to a 1-hour game) passes at 100% with ~99.9% command coverage.

`player-summary` prints the state engine's per-player command-derived totals
(shipments sent, techs researched, units trained). It reads `debug.playerStates`,
so parse with `--debug-commands` first.

`inspect-card-commands` groups card-related commands per actor: deck selections (`commandId=66` with `cardId=-1`), card sends (`commandId=2` deck index variant) with their `deckMatch` resolution, the actor's known decks, and the system shipment arrival chats (hints only — they do not prove ownership).

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

The viewer ("Campaign Ledger") is an AoE3-themed UI — wood-framed parchment,
brass fittings, heraldic player cards. It includes:

- overview gauges (map, result, per-type event counts)
- heraldic player cards: civ-color shield, team, home city, and a resource-spent bar
- event type filter (chat / age-up / research / train / build / shipment / resign)
- player filter, chat/player search, confirmed-shipment toggle
- filtered view export
- Build Order tab: per-player age-up / research / train / build / shipment timeline + resources spent
- Economy tab: cumulative resources-spent-over-time chart, one line per player (economy pace)

It is a single offline HTML file (no web fonts or game assets bundled) and is
unofficial / fan-made.

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

Card sends are decoded from the command stream, never guessed from system chat. See `docs/reverse-engineering/shipments.md` for the full evidence.

- `commandId=2` with `unitProtoIdCandidate=-1` and `deckIndexCandidate>=0` is a card send click: the payload carries a deck **index**, not a card id
- `commandId=66` with `cardIdCandidate=-1` selects the actor's active deck (`deckIdCandidate`); with `cardIdCandidate>=0` it appends a card to a deck being edited in-game
- The deck index is resolved against the **acting slot's own** active deck (parsed `replay.players[*].initialDecks` or `commandId=66` reconstruction) — a candidate is never matched against another player's deck
- Arrival can lag the send by minutes (XP shipment queue), so `*** Shipment has arrived` system chat is never used to assign ownership
- By default the normal timeline contains **no** shipment events; pass `--experimental-shipments` to emit deck-resolved sends as `status=candidate`

`debug.commands[*].deckMatch` records the resolution per card send:

```json
{
  "matched": true,
  "slotId": 2,
  "deckIndex": 0,
  "activeDeckId": 0,
  "cardIdCandidate": 1676,
  "source": "debug_command66_deck_setup",
  "confidence": "medium"
}
```

Unresolved example:

```json
{
  "matched": false,
  "slotId": 6,
  "deckIndex": 2,
  "confidence": "low",
  "reason": "active deck unknown (no deck selection command, no unique default)"
}
```

Sources:

- `parsed_player_deck` — replay player block was decoded
- `debug_command66_deck_setup` — deck reconstructed from in-game `commandId=66` deck edit commands

## Game Data Layer

The parser emits numeric ids only. The game data layer (`data/*.json`, compiled
into the binary) resolves a replay card **`rawId`** to a display name / icon key.
Data is imported from the MIT-licensed
[aoe3-companion](https://github.com/VitorRoda/aoe3-companion) set. See
`docs/game-data-layer.md` and `data/README.md`.

```powershell
cargo run -- resolve-card --card-id 1676
cargo run -- resolve-unit --unit-id 928
cargo run -- resolve-tech --tech-id 410
cargo run -- resolve-building --building-id 926
cargo run -- import-aoe3-companion --input "path\to\aoe3-companion" --out data
```

```text
Card 1676
Name: Capitalism
Internal: HCXPCapitalism
Icon: card.capitalism
Icon path: resources/images/icons/techs/native/Capitalism.png
Source: aoe3_companion
Confidence: imported
```

Unknown ids never crash — they resolve to `Unknown Card #<id>` and the generic
icon. A resolved card is attached to `debug.commands[*].deckMatch.card` and (with
`--experimental-shipments`) to `payload.cardName` / `payload.iconKey`.

> **Replay ids are array indices.** All three replay id spaces are 0-based array
> indices into the companion game data, not dbids:
> - card `rawId` / research `techIdCandidate` → `cards.json` (techtree index)
> - train `unitProtoIdCandidate` → `units.json` (proto index)
>
> Verified civ-correct (Capitalism, Janissary→Ottoman, Confucius' Gift→Chinese, …).
> Each entry also carries its `dbid`.

Debug commands are enriched with `deckMatch.card` (commandId=2 sends), `unit`
(train), and `tech` (research). By default (no `--experimental-shipments`) the
normal timeline still contains **no** shipment/train/research events — only the
debug layer is enriched. See `docs/overlay-features.md` for the feature matrix.

## Scope: file-only vs runtime

`.age3Yrec` is a **command/input replay** — it records what players *did*, not the
game's simulation state. So deaths, losses, active unit counts, resources, and
idle villagers are **not in the file** (they are listed `unavailable` in
`debug.playerStates`, never guessed). See `docs/reverse-engineering/replay-model.md`
for the evidence and the controlled death-test protocol, and `docs/roadmap.md` for
the two modes:

- **Mode A — file-only analyzer (current):** shipments, decks/cards, research,
  train/build commands, build order, state from confirmed commands.
- **Mode B — runtime-assisted (later):** live game capture for the simulation
  state a replay file cannot contain (CaptureAge-like).
