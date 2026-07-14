# Feature Reference

This is the current user-facing feature contract for Vapourfly. Keep CLI and
GUI work aligned with this document: when a capability changes, update the
feature row, the command reference, and the relevant GUI smoke test.

## Status Labels

| Label | Meaning |
|---|---|
| Yes | Available in the released CLI or GUI surface |
| Partial | Available, but with a narrower scope than the core model supports |
| Core only | Implemented in library code, but not wired into a user workflow |
| No | Not currently available |

## Feature Matrix

| Capability | CLI | GUI | Current behavior |
|---|---|---|---|
| Steam directory detection | Yes | Yes | Uses platform defaults, `--steam-dir`, `VAPOURFLY_STEAM_DIR`, or the standard config file path. |
| Steam account selection | Yes | Yes | CLI supports `--account` and `accounts list`; GUI has an Account Override setting plus a detected accounts list with a one-click override action. If unset, Vapourfly selects the most recent Steam account or the only account. |
| Setup diagnostics | Yes | Yes | CLI `vapourfly doctor` and GUI Settings setup check report Steam paths, accounts, library folders, cloud storage, cache path, and credential status. |
| Library scan | Yes | Yes | CLI `scan`; GUI scans on startup and via Refresh. Both read installed games, playtime, hidden state, and Steam collections. GUI Library presents scanned games as poster cards with search and three filters: Installed only, Not hidden, Not junk. Summary shows matching / installed / library counts. |
| Enriched scan output | Yes | Yes | CLI `vapourfly scan --enrich` adds external metadata to that command's output and cache. GUI Library hydrates cached metadata and shows Proton/Deck badges and detail on poster cards when cache entries exist. |
| Library hover Recommend | — | Yes | Hovering or selecting a Library poster card reveals a Recommend shortcut that opens Recommendations with that AppID as seed (approved ADR-0006 deviation). |
| Collections list | Yes | Yes | CLI lists Steam collections; GUI shows a read-only card grid with name, game count, optional poster collage, and hidden badge when applicable. No member drill-in editor in v1. |
| Collections export | Yes | Yes | CLI `vapourfly collections export --out <file>` and GUI Collections **Export all** (native save dialog) write the same Steam collection JSON export. |
| Junk preview | Yes | Yes | Evaluates Default, Strict, or Aggressive junk modes after hydrating cached external metadata into scanned game records. GUI opens Junk from the Library toolbar (`Junk…` panel), not a sidebar item (ADR-0006). Panel: mode → Preview → apply/hide. |
| Junk apply to collection | Yes | Yes | Writes junk candidates to a Steam collection after dry-run/confirmation and backup. GUI action lives in the Library Junk panel (**Apply to collection**). |
| Junk hide | Yes | Yes | Adds junk candidates to Steam's hidden collection after dry-run/confirmation and backup. GUI action lives in the Library Junk panel (**Hide**). |
| Recommendations | Yes | Yes | Recommends from hydrated scanned game records by available minutes, count, installed-only, Deck mode, and optional seed AppID. GUI top-level view is labeled **Recommendations**; Preview shows score and human-readable reason codes. |
| Temporary recommendation collection | Yes | Yes | CLI `recommend --to-collection --dry-run|--confirm` and GUI Recommendations **Write to vapourfly-picks** write to `vapourfly-picks` after dry-run confirmation. |
| Playlist import | Yes | Yes | Imports Vapourfly JSON playlist files. CLI stores imported playlists under the app data playlist directory. |
| Playlist export | Yes | Yes | CLI exports a stored playlist by ID. GUI exports the currently loaded playlist to a selected JSON path. |
| Playlist match report | Yes | Yes | Reports owned, missing, played, unplayed, hidden, and junk counts for a playlist. |
| Playlist completion price | Yes | Yes | CLI `playlist match`/`playlist import` and GUI match report show the sum of Steam Store prices for owned, unplayed, non-free games. Requires `steam-store` cache entries; run `vapourfly cache refresh --source steam-store` to populate. The CLI and GUI both display the field with a hint when no price data is cached. |
| Rule-based playlists | Yes | Yes | CLI `playlist create-rules` and GUI Playlists view create rule-based playlists. CLI/GUI workflows hydrate cached metadata before rule evaluation. Rule operators: `ProtonAtLeast`, `HltbMaxMinutes`, `ControllerSupportFull`, `PlaytimeBetween`, `RatingAtLeast`, `HasGenre`, `HasTag`, `Installed`, `NotJunk`, `NotHidden`, `And`, `Or`, `Not`. |
| Playlist sync to Steam collection | Yes | Yes | CLI `vapourfly sync collection <playlist-id>` and GUI Playlists sync resolve a playlist and write a Steam collection with dry-run/confirmation. |
| Data source cache refresh | Yes | Yes | CLI `cache refresh --source <source>` and GUI Data Sources refresh support `igdb`, `rawg`, `protondb`, `pcgw`, `hltb`, `steam-store`, and `all`. |
| Data source status | Yes | Yes | CLI `sources status`; GUI Data Sources table. Shows credential state, cache entries, stale entries, and last success. |
| Offline mode | Yes | Yes | CLI `--offline` blocks network calls and uses cache during enrichment. GUI has an offline toggle that blocks cache refresh network calls while library workflows continue to hydrate cached metadata. |
| Backup list | Yes | Yes | Lists backups next to the Steam cloud storage file. GUI lists backups under **Settings** (not a top-level sidebar item; ADR-0006). |
| Backup restore | Yes | Yes | Restores a selected backup after confirmation. GUI restore lives under Settings → Backups. |
| Diagnostics export | Yes | Yes | CLI `vapourfly diagnostics export --out <file>` and GUI Settings diagnostics export write sanitized support data. |
| Settings | Yes | Yes | CLI `settings show` displays resolved config; `settings set <field> <value>` and `settings unset <field>` edit `config.toml`. GUI edits Steam directory, account, store locale, backup retention, write safety, and hosts backup list/restore. Settable fields: `steam_dir`, `account`, `cc`, `lang`, `backup_retention_count`. |
| GUI navigation (sidebar) | — | Yes | Top-level destinations: Library, Collections, Recommendations, Playlists, Discover, Data Sources, Settings. Default landing view is Library. Dark design-token shell with monochrome line icons (ADR-0006). Junk is a Library panel; backups live under Settings. |
| Playlist creation/editing UI | Yes | Yes | CLI `playlist create`; GUI Playlists view supports load existing, create/edit fields, save, match report with owned preview, export, import file, VF1 share codes, and sync to Steam Collection after dry-run confirmation. No Discover control on this page (ADR-0006). |
| Share codes | Yes | Yes | `VF1:` compact binary playlist codes (ADR-0003) via CLI `playlist share` / `playlist import --code` and GUI copy/import controls. The payload carries content + name + description, DEFLATE-compressed and base64url-encoded. No backward compatibility with the old base64url(JSON) format. |
| Discover / similar-game playlist generation | Yes | Yes | CLI `playlist discover`; GUI **Discover** top-level view (seed AppID, count, Generate) shows on-page result cards with scores and reason codes. Discover owns the entire "similar picks" surface (ADR-0005); Playlists has no Discover control (ADR-0006). On success the GUI writes the stable slot id `discover` and overwrites on regenerate (ADR-0007). Continuation: Open in Playlists (loads the slot for edit/share/sync); optional Sync to Steam Collection remains dry-run confirmed. |
| Dynamic collection templates | Yes | Yes | CLI `collections dynamic <template>`; GUI Playlists **Dynamic** button opens a lightweight chooser for `deck-session` / `finish-it` plus session minutes and count, then generates. Deck Session requires installed, not hidden, not junk, ProtonDB Gold-or-better, PCGW full controller support, and HLTB within the requested session length. On success the GUI writes under stable slot ids `dynamic-deck-session` / `dynamic-finish-it` and overwrites on regenerate (ADR-0007). |
| Editorial Moods | Yes | Yes | CLI `collections mood [name]`; GUI Playlists **Mood** button opens a lightweight chooser for the seven canonical Editorial Moods with opaque criteria (ADR-0004): Today's Biggest Hits, Indie Rising, Friday Party, Deck Guardians, Unopened Treasures, Weekend Marathon, Quick Round. On success the GUI writes under stable slot ids `mood-<id>` (one slot per mood) and overwrites on regenerate (ADR-0007). |

## Current Data Flow

1. `scan` reads local Steam files and builds the library model.
2. `scan --enrich` or cache refresh fetches external metadata and stores API responses in the local cache.
3. User workflows such as Junk, Recommend, Playlist Match, Sync, Discover, Dynamic templates, and Editorial Moods go through `vapourfly_api::workflow::prepare`, which scans the library, hydrates external metadata (lazy network fetch when not offline — ADR-0002), and classifies junk. Fetch failures degrade gracefully: the game is evaluated with whatever data is available.
4. `--offline` forces cache-only hydration. Use `scan --enrich` or `cache refresh` to populate missing cache entries ahead of time when you want to avoid network calls during workflows.

For developers, workflow commands call `vapourfly_api::workflow::prepare` and
then re-classify junk with the desired mode if different from Default.

## Supported Data Sources

| Source | Credential | Used for |
|---|---|---|
| IGDB | `VAPOURFLY_IGDB_CLIENT_ID` and `VAPOURFLY_IGDB_CLIENT_SECRET` | Genres, themes, keywords, ratings, time-to-beat, similar games |
| RAWG | `VAPOURFLY_RAWG_KEY` | Genres, tags, ratings, store availability |
| ProtonDB | None | Linux and Steam Deck compatibility tier |
| PCGamingWiki | None | Controller support, Steam Deck notes, fixes URL |
| HLTB | None, compile-time `hltb_scrape` feature | Completion times from HowLongToBeat |
| Steam Store | None | App details, store metadata, price, platform support |

API credentials must be provided as environment variables before launching the
CLI or GUI. The standard config file is for local Vapourfly settings such as
Steam path, account, locale, and backup retention.

## Write Safety Contract

All Steam file writes go through the same safety model:

- CLI write commands require exactly one of `--dry-run` or `--confirm`.
- GUI write actions show a dry-run diff or confirmation dialog before writing.
- Vapourfly refuses to write while Steam is running unless write safety is explicitly relaxed.
- Every write creates a timestamped backup before modifying the Steam cloud storage file.
- Writes target `userdata/<account>/config/cloudstorage/cloud-storage-namespace-1.json`.

See [STEAM_FILE_SAFETY.md](STEAM_FILE_SAFETY.md) for the full write contract.

## Playlist JSON Contract

Vapourfly playlists use schema `vapourfly.playlist.v1`.

Manual playlist:

```json
{
  "vapourfly_schema": "vapourfly.playlist.v1",
  "created_by": "user",
  "playlist": {
    "id": "deck-shortlist",
    "name": "Deck Shortlist",
    "description": "Games to try on Steam Deck",
    "content": {
      "type": "Manual",
      "value": {
        "app_ids": [292030, 367520]
      }
    }
  }
}
```

Rule playlist:

```json
{
  "vapourfly_schema": "vapourfly.playlist.v1",
  "created_by": "user",
  "playlist": {
    "id": "installed-unplayed",
    "name": "Installed Unplayed",
    "description": "Installed games with no recorded playtime",
    "content": {
      "type": "Rules",
      "value": {
        "rules": [
          { "op": "Installed" },
          { "op": "PlaytimeBetween", "args": { "min": 0, "max": 0 } },
          { "op": "NotHidden" }
        ]
      }
    }
  }
}
```

Available rule operators: `ProtonAtLeast`, `HltbMaxMinutes`,
`ControllerSupportFull`, `PlaytimeBetween`, `RatingAtLeast`, `HasGenre`,
`HasTag`, `Installed`, `NotJunk`, `NotHidden`, `And`, `Or`, and `Not`.
