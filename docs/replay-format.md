# Replay format reality (what a .age3Yrec can and cannot give us)

`.age3Yrec` is a **command replay**: a deterministic log of player inputs plus
per-tick desync checksums. The game reproduces a match by re-running its
simulation over these inputs. Verified by decompressing the body (10-byte header,
then raw DEFLATE):

- A 10-min 1v1 → ~53 MB decompressed, ~44k ticks.
- Each tick: a marker, a ~113-byte header whose payload is a **high-entropy
  checksum** (values swing ±billions tick to tick — a hash, not state), then chat
  and the player commands issued that tick.

So the file contains **player intent**, not **game state**.

## Derivable from the static replay (player commands)

- Shipments / cards sent (commandId=2 card variant + deck match)
- Techs researched (commandId=1) — includes age-ups
- Units trained / queued (commandId=2 train variant)
- Buildings placed (commandId=3, candidate — not yet confirmed)
- Resign, chat, players/civs/teams/decks, inferred winner
- → **build order**, action timeline, "what each player did and when"
- With unit/tech costs (from `protoy`/`techtreey`) we can also derive **cumulative
  resources spent** and **military value produced** — clearly "spent/produced",
  never "current" or "lost".

## NOT derivable from the static replay (needs live game memory or full re-sim)

These are simulation outcomes, absent from a command log:

- Active unit counts, units in queue vs already produced
- Unit deaths / losses — **there is no "death command"**; deaths are sim results
- Military lost + resource value, villagers lost + resource value
- Idle villagers
- Current/net resources, score-over-time, map/minimap, unit positions
- Which tech is currently applied to a specific unit (live state)

This is exactly why CaptureAge and similar overlays attach to the **running
game** (spectator / memory) rather than parsing a replay file. Our project
deliberately does not do live capture yet, so these features are out of scope for
static parsing.

See `docs/overlay-features.md` for the per-feature status table. The honesty rule
stands: we never fabricate the un-derivable values — the state engine marks them
`unavailable` with this reason.
