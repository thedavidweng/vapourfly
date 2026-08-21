# Plan a Deck Session

You have 90 minutes and a Steam Deck (or just an evening). This guide gets
you a shortlist of games that fit the time you have, and -- if you want --
puts it into Steam as a collection so it is right there when you sit down.

Nothing here writes to Steam until you pass `--confirm`.

## 1. Ask for picks that fit your time

```bash
vapourfly recommend --minutes 120
```

```text
AppID      Name                                        Score  Reasons
--------------------------------------------------------------------------------------
8650       RACE 07: Andy Priaulx Crowne Plaza Ra…       3.50  low_playtime, time_match
234140     Mad Max                                      3.50  low_playtime, time_match
267940     Glacier 3: The Meltdown                      3.50  low_playtime, time_match
283330     Desert Thunder                               3.50  low_playtime, time_match
301690     Cobi Treasure Deluxe                         3.50  low_playtime, time_match

5 recommendations (865 games scanned)
```

Each pick carries its reasons (`time_match` means the game's estimated
completion time fits your window). Useful flags:

- `--count 3` -- fewer picks
- `--installed-only` -- only games already on disk
- `--deck` -- weigh Steam Deck compatibility
- `--seed 42` -- reproducible shuffle
- `--exclude-collection "Favorites"` -- skip games you have already
  curated (repeatable)

## 2. Or compile the Deck Session template

The `deck-session` dynamic template filters by hard criteria instead of
scoring: installed, not hidden, not junk, ProtonDB Gold or better, full
controller support, and completion time inside your session length.

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
and an HLTB session-length bound follow.)

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
