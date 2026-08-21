# Share and Sync Playlists

You want to curate a list of games, hand it to a friend as a one-line code,
and push it into your own Steam library as a real collection. Playlists are
portable JSON (schema `vapourfly.playlist.v1`); share codes are compact
`VF1:` strings; sync writes a Steam cloud collection.

The demo playlist in the captures below was created for this guide and
removed afterwards. Only the final `--confirm` step writes to Steam.

## 1. Create a playlist

```bash
vapourfly playlist create --id vf-docs-demo --name "Docs Demo" \
  --description "Temporary demo playlist for documentation captures" \
  --app-ids 10,10150,105450
```

```text
Created playlist: Docs Demo
  ID: vf-docs-demo
```

The playlist is stored locally (shown by its ID in later commands). You can
also write the JSON file by hand -- the format is documented in the
[feature reference](../reference/FEATURES.md#playlist-json-contract) -- and
import it with `vapourfly playlist import my-playlist.json`.

## 2. Match it against your library

Before syncing, see how much of the list you actually own and what it would
cost to complete:

```bash
vapourfly playlist match "$HOME/Library/Application Support/vapourfly/playlists/vf-docs-demo.json"
```

```text
Playlist: Docs Demo
  ID:       vf-docs-demo

Match report:
  Owned:    3
  Missing:  0
  Played:   3
  Unplayed: 0
  Hidden:   0
  Junk:     0
  Completion price: (unavailable — missing entries may be free, unpriced, or not cached)
```

Owned/missing/played counts are self-explanatory. **Completion price** sums
Steam Store prices for missing entries only -- here there was nothing
missing, so no price applies.

## 3. Share it

```bash
vapourfly playlist share vf-docs-demo
```

```text
Share code for 'Docs Demo':
VF1:eJwBUQCu_wEBAwAAAAoAAACmJwAA6psBAAkARG9jcyBEZW1vMgBUZW1wb3JhcnkgZGVtbyBwbGF5bGlzdCBmb3IgZG9jdW1lbnRhdGlvbiBjYXB0dXJlc1ONGW4
```

The code embeds the playlist name, description, and contents -- nothing
personal -- so it is safe to paste anywhere.

Anyone can import it:

```bash
vapourfly playlist import --code 'VF1:...'
```

## 4. Sync it to Steam

Preview the exact change to your Steam cloud storage file:

```bash
vapourfly sync collection vf-docs-demo --dry-run
```

```text
Sync playlist 'Docs Demo' to Steam collection
  Playlist ID:   vf-docs-demo
  Collection ID: vf-docs-demo
  App IDs:       3
  Target:        .../Steam/userdata/<steamid3>/config/cloudstorage/cloud-storage-namespace-1.json

Diff:
  Collection 'vf-docs-demo': created
  App IDs to add: 3
  Unchanged entries: 13

Dry run complete. No changes made.
```

(Path shortened; the real target is inside your Steam directory. What
happens to that file on a confirmed write is specified in
[Steam file safety](../reference/STEAM_FILE_SAFETY.md).)

Close Steam, then execute:

```bash
vapourfly sync collection vf-docs-demo --confirm
```

The collection appears in your Steam client. Rule-based playlists are
resolved against your current library at sync time, so re-syncing refreshes
the membership.

## Next steps

- A confirmed write went wrong? [Back up and restore Steam files](back-up-and-restore.md)
- Generate lists automatically instead:
  [Plan a Deck session](plan-deck-session.md)
- All playlist subcommands: [Command reference](../reference/COMMANDS.md#vapourfly-playlist)
