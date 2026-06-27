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
| Steam account selection | Yes | Yes | CLI supports `--account`; GUI has an Account Override setting. If unset, Vapourfly selects the most recent Steam account or the only account. |
| Setup diagnostics | Yes | No | `vapourfly doctor` reports Steam paths, accounts, library folders, cloud storage, cache path, and credential status. |
| Library scan | Yes | Yes | CLI `scan`; GUI scans on startup and via Refresh. Both read installed games, playtime, hidden state, and Steam collections. |
| Enriched scan output | Yes | No | `vapourfly scan --enrich` adds external metadata to that command's output and cache. GUI has cache refresh controls but does not display enriched fields in Library. |
| Collections list | Yes | Yes | CLI lists Steam collections; GUI displays collection names, counts, and hidden status after scan. |
| Collections export | Yes | No | `vapourfly collections export --out <file>` writes a Vapourfly playlist-style JSON export. |
| Junk preview | Yes | Yes | Evaluates Default, Strict, or Aggressive junk modes against data present in the current scanned game records. |
| Junk apply to collection | Yes | Yes | Writes junk candidates to a Steam collection after dry-run/confirmation and backup. |
| Junk hide | Yes | Yes | Adds junk candidates to Steam's hidden collection after dry-run/confirmation and backup. |
| Recommendations | Yes | Yes | Recommends from current scanned game records by available minutes, count, installed-only, Deck mode, and optional seed in CLI. |
| Temporary recommendation collection | No | No | Recommendations are displayed only; they are not written to a temporary Steam collection. |
| Playlist import | Yes | Yes | Imports Vapourfly JSON playlist files. CLI stores imported playlists under the app data playlist directory. |
| Playlist export | Yes | No | CLI exports a stored playlist by ID. GUI can import and inspect, but not export. |
| Playlist match report | Yes | Yes | Reports owned, missing, played, unplayed, hidden, and junk counts for a playlist. |
| Playlist completion price | Partial | Partial | Core can calculate missing-game price when `steam_store` data is present. Current CLI/GUI match flows do not auto-enrich before matching, so this field is usually absent. |
| Rule-based playlists | Partial | Partial | Rule files can be imported and evaluated. Rules that require external fields only work when those fields are already present on the game records. |
| Playlist sync to Steam collection | Yes | No | `vapourfly sync collection <playlist-id>` resolves a stored playlist and writes a Steam collection with dry-run/confirmation. |
| Data source cache refresh | Yes | Yes | CLI `cache refresh --source <source>` and GUI Data Sources refresh support `igdb`, `rawg`, `protondb`, `pcgw`, `hltb`, `steam-store`, and `all`. |
| Data source status | Yes | Yes | CLI `sources status`; GUI Data Sources table. Shows credential state, cache entries, stale entries, and last success. |
| Offline mode | Yes | Partial | CLI `--offline` blocks network calls and uses cache during enrichment. GUI has no offline toggle. |
| Backup list | Yes | Yes | Lists backups next to the Steam cloud storage file. |
| Backup restore | Yes | Yes | Restores a selected backup after confirmation. |
| Diagnostics export | Yes | No | `vapourfly diagnostics export --out <file>` writes sanitized support data. |
| Settings | Partial | Yes | CLI uses flags, environment variables, and the standard config file. GUI edits Steam directory, account, store locale, backup retention, and write safety. |
| Playlist creation/editing UI | No | No | Users create playlist JSON files outside Vapourfly, then import them. |
| Share codes | No | No | JSON import/export is the supported sharing format. |
| Discover / similar-game playlist generation | Core only | No | Recommendation scoring has taste-similarity support when metadata is present; no user command generates a Discover playlist. |
| Dynamic collection templates | No | No | Deck session, Finish It, Mood, and Playlist Radio collections are not exposed as commands or GUI flows. |

## Current Data Flow

1. `scan` reads local Steam files and builds the library model.
2. `scan --enrich` or cache refresh fetches external metadata and stores API responses in the local cache.
3. User workflows such as Junk, Recommend, Playlist Match, and Sync currently operate on the scanned game records they create for that command or GUI session.
4. Current CLI/GUI workflows do not automatically hydrate cached external metadata back into Junk, Recommend, or Playlist rule evaluation.

For developers, that means the core model already has fields for ProtonDB,
PCGW, HLTB, RAWG, IGDB, and Steam Store data, but a frontend feature should
explicitly load enrichment before promising behavior based on those fields.

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
`PlaytimeBetween`, `RatingAtLeast`, `HasGenre`, `HasTag`, `Installed`,
`NotJunk`, `NotHidden`, `And`, `Or`, and `Not`.
