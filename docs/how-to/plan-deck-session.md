# Plan a Deck Session

You have 90 minutes and a Steam Deck (or just an evening). This guide gets
you a shortlist of games that fit the time you have, and -- if you want --
puts it into Steam as a collection so it is right there when you sit down.

Nothing here writes to Steam until you pass `--confirm`.

## 1. Ask for picks that fit your time

```bash
vapourfly recommend --minutes 120
```

The default plans a sitting, not finishing the campaign. A 40-hour RPG can be
recommended for an hour. Complete signed reasons distinguish neutral unknown
session suitability from a small cached short-loop-tag heuristic; neither
promises an exact session duration. Full completion estimates show provenance
and unknown remaining time separately.

To deliberately seek a short game to finish instead:

```bash
vapourfly recommend --minutes 60 --finish-short-game
```

Only positive known full estimates within the window qualify. Unknown estimates
are excluded, not zeroed; neither playtime nor achievements infer remaining hours.
Useful flags:

- `--count 3` -- fewer picks
- `--installed-only` -- only games already on disk
- `--deck` -- weigh Steam Deck compatibility
- `--seed 42` -- reproducible tie ordering
- `--exclude-collection "Favorites"` -- skip curated games (repeatable)

## 2. Or compile the Deck Session template

The `deck-session` dynamic template filters by hard criteria instead of
scoring. Its legacy serialized duration rule still constrains **full completion
time**, not measured session length: installed, not hidden, not junk, ProtonDB Gold or better, full
controller support, and a full completion estimate inside the requested limit.
This compatibility template is deliberately unchanged; use default Recommendations
for long-campaign session picks.

```bash
vapourfly collections dynamic deck-session --minutes 90 --out deck-session.json
```

```text
Compiled dynamic template: Deck Session
  Playlist ID: deck-session
  Name:        Deck Session (90m)
  Rules:       6
  Exported to deck-session.json
```

The compiled playlist is a normal rule-based playlist file:

```json
{
  "vapourfly_schema": "vapourfly.playlist.v1",
  "created_by": "vapourfly",
  "playlist": {
    "id": "deck-session",
    "name": "Deck Session (90m)",
    "description": "Installed Steam Deck-friendly games that fit a 90-minute session",
    "content": {
      "type": "Rules",
      "value": {
        "rules": [
          {
            "op": "Installed"
          },
          {
            "op": "NotHidden"
          },
          {
            "op": "NotJunk"
          },
          {
            "op": "ProtonAtLeast",
            "args": {
              "tier": "Gold"
            }
          }
        ]
      }
    }
  }
}
```

(Excerpt: the compiled file contains all 6 rules -- `ControllerSupportFull`
and an effective completion-time bound follow.)

There is also a `finish-it` template for chipping away at nearly-finished
games, and seven curated Editorial Moods (`vapourfly collections mood`).

## 3. Put the picks into Steam

`--to-collection` writes the recommendations into a temporary Steam
collection named `vapourfly-picks`. Preview the write first:

```bash
vapourfly recommend --minutes 120 --to-collection --dry-run
```

```text
Temporary recommendation collection
===============================
Collection ID: vapourfly-picks
Games:         5

Diff:
  Collection 'vapourfly-picks': created
  App IDs to add: 5
  Unchanged entries: 13

Dry run complete. No changes made.
```

Then execute it (close Steam first; the write is backed up automatically):

```bash
vapourfly recommend --minutes 120 --to-collection --confirm
```

To sync the compiled `deck-session` playlist instead, see
[Share and sync playlists](share-and-sync-playlists.md) -- sync follows the
same dry-run/confirm pattern.

## Next steps

- Clear out the games the recommender correctly skips:
  [Purge junk from your library](purge-junk.md)
- What hydration happens before recommendations are scored:
  [How library hydration works](../explanation/hydration-model.md)
