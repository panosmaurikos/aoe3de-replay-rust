# Shipments / Card Sends — Reverse Engineering Notes

Status: decoded to **medium** confidence (structure verified on two replays; card name mapping still missing).

Evidence replays:

- `testship.age3Yrec` — 2 players, 62 `commandId=2` rows, 23 `commandId=66` rows, 1 arrival chat
- `malloncheater.age3Yrec` — 8 players, 594 `commandId=2` rows, 7 `commandId=66` rows, 44 arrival chats (32 unique times)

## commandId=2: train unit OR card send

All `commandId=2` payloads share one 91-byte layout. After the common header
(actor slot, selected units, variable blocks, unknown byte block `a1 00 01 00`),
the tail contains two i32 fields, read by the parser as:

| decodedFields key      | bytes (typical) | meaning |
|------------------------|-----------------|---------|
| `unitProtoIdCandidate` | 75-78           | proto unit id when training, `-1` otherwise |
| `deckIndexCandidate`   | 79-82           | deck card index when sending a card, `-1` otherwise |

Two mutually exclusive variants were observed (62/62 and 594/594 rows fit):

1. **Train unit** (`parsedAs=train_unit_candidate`): `unitProtoIdCandidate >= 0`, `deckIndexCandidate == -1`.
   Examples: 284 for European Settlers (all Euro civs in malloncheater), 928 for Chinese villagers, 1710/1711/1739 for Hausa units. Appears in rapid bursts ~130 ms apart (queue clicks).
2. **Card send** (`parsedAs=card_send_candidate`): `unitProtoIdCandidate == -1`, `deckIndexCandidate >= 0`.
   The index is 0-based into the actor's **active deck** (0..24 — DE decks hold max 25 cards). Double-clicks produce duplicate rows ~100-200 ms apart.

The old `shipmentIdCandidate` field read bytes 79-82 and was renamed to
`deckIndexCandidate`; it is a deck **index**, not a card or shipment id. The
card id is **not present** in the payload at all.

## commandId=66: deck select OR deck card add

Same 83-byte layout, same two tail fields:

| decodedFields key | meaning |
|-------------------|---------|
| `deckIdCandidate` | deck id within the player's saved decks |
| `cardIdCandidate` | `-1` for a deck **selection**, otherwise the card raw id being **added** |

1. **Deck select** (`parsedAs=deck_select_candidate`): `cardIdCandidate == -1`.
   malloncheater: 7 selects, e.g. slot 2 selected `deckId=11` ("team 33") at 01:41, slot 4 selected `deckId=5`. Slot 6 never selected — their active deck is unknown.
2. **Deck card add** (`parsedAs=deck_card_add_candidate`): `cardIdCandidate >= 0`.
   testship: George Washington (slot 2) built deck 0 in-game with 21 add rows at ~00:00.6 (1676, 714, 708, ...), then selected it (`cardId=-1` row).

## Card send resolution (deckMatch)

A card send resolves to a card raw id via the **acting slot's own** decks:

1. Take the actor slot from the `commandId=2` row itself.
2. Determine the active deck at the send time: last `commandId=66` selection
   with `timeMs <= send time`. If no selection exists, fall back only to an
   unambiguous option (single known deck, or a unique `isDefault` parsed deck).
3. Deck content comes from `replay.players[*].initialDecks` (`source=parsed_player_deck`)
   or from `commandId=66` add rows (`source=debug_command66_deck_setup`).
4. `cardId = deck.cards[deckIndexCandidate].rawId`. Out-of-range index, ambiguous
   deck id, or unknown active deck ⇒ `matched=false` with a `reason`.

The matched value is a replay **`rawId`**, which is NOT the game data `dbid`
space the card database is keyed by (see `docs/game-data-layer.md`). The
arrival-chat correlation that suggested `1676 → Capitalism` is contradicted by the
authoritative data: Capitalism's `dbid` is `3438`, and `1676` is not a tech
`dbid` at all. So the resolver reports the numeric `rawId` only and assigns **no
card name** until the `rawId → dbid` bridge is solved.

Examples (rawId, name NOT asserted):

- testship slot 2: select deck 0 (built via cmd66), send `deckIndex=0` at 03:00.514 ⇒ rawId 1676; "Capitalism Shipment has arrived" at 03:40.510 (chat hint only — 40 s lag, not proof of the name↔id link).
- malloncheater slot 2 (Italians, deck "team 33"): send `deckIndex=0` at 01:47.009 ⇒ card 735; "TEAM Marco Polo Voyages Shipment has arrived" at 01:51.990 (only Italian player in game).
- malloncheater slot 6 (Dutch, 6 decks, no selection, no default) ⇒ all sends honestly `matched=false`.

## Why system chat must NOT assign ownership

- "X Shipment has arrived" prints when the shipment **arrives**, and the DE
  shipment queue is XP-gated: testship's Capitalism send happened at 03:00.514,
  the arrival chat at 03:40.510 — a 40 s lag. Any fixed time-window correlation
  misattributes cards.
- TEAM card arrivals print once per team member (4 duplicate lines in malloncheater).
- The chat line carries no player identity.

Arrival chats may be used only as *name hints* once ownership is already
established from the command actor + deck index, and even then per-player
arrival ordering (FIFO queue) has not been verified yet.

## Open questions

- **Card `rawId` → `dbid` mapping (the key blocker).** The full game database is
  now imported (`data/cards.json`, dbid-keyed, ~2.5k cards) but the replay
  `rawId` space does not match it (Capitalism: rawId 1676 vs dbid 3438; ~32%
  coincidental overlap on real decks). Solving this bridge — likely via the
  companion `homecities/*.json` per-civ card order, or a direct game-file
  extraction exposing both ids — is what unlocks real card names on shipments.
- Active deck when no `commandId=66` selection exists and several decks are saved (malloncheater slot 6).
- Whether an in-game deck edit (cmd66 adds) can modify a deck that was also parsed in the header (currently parsed content wins when both agree; conflicting contents are reported as ambiguous).
- Double-click duplicate sends: collapse window not yet decided; duplicates are currently kept (visible in debug; both rows resolve to the same card).
- Rows with `unitProtoIdCandidate == -1 && deckIndexCandidate == -1` (or both set) would be flagged `parsedAs=command2_unclassified`; none observed in the two evidence replays so far.
