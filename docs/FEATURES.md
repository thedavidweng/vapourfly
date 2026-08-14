# Feature Reference

This is the current user-facing feature contract for Vapourfly. Keep CLI and
GUI work aligned with this document: when a capability changes, update the
feature row, the command reference, and the relevant GUI smoke test. The
shipped desktop GUI is a GPUI application using gpui-component.

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
| Library scan | Yes | Yes | CLI `scan`; GUI scans on startup and via Refresh. Both read installed games, playtime, hidden state, and Steam collections. GUI Library presents a search field and primary scope control (All, Installed, Unplayed, Hidden), labelled Deck/Controller/playtime/genre/tag/sort filters, editorial category chips (Cozy, Story-rich, Great on Deck, Short sessions), advanced ProtonDB/HLTB/exclusion controls, a responsive four-column card grid with deterministic illustrated offline covers, pagination (48/page with a **Load more** button), card actions (Discover similar, Copy AppID, Open Steam Store), and a fixed-width insights rail (totals, installed, playtime, junk excluded, backlog progress, recent activity, Hidden, Avg HLTB). While the library snapshot is being prepared in the background, a skeleton loader is shown and action buttons are disabled. Short sessions filters by HLTB main_story_seconds ≤ 120 min (falls back to IGDB time_to_beat when HLTB is absent). |
| Enriched scan output | Yes | Yes | CLI `vapourfly scan --enrich` adds external metadata to that command's output and cache. GUI Library hydrates cached metadata and shows Proton/Deck badges and detail on poster cards when cache entries exist. |
| Library card actions | — | Yes | Every Library card exposes compact Discover similar, Copy AppID, and Open Steam Store actions. Discover similar opens Discover seeded with that AppID (Discover owns the seed-based similar-picks surface — ADR-0005). |
| Collections list | Yes | Yes | CLI lists Steam collections; GUI shows a read-only card grid with name, game count, optional poster collage, and hidden badge when applicable. No member drill-in editor in v1. |
| Collections export | Yes | Yes | CLI `vapourfly collections export --out <file>` and GUI Collections **Export all** (native save dialog) write the same Steam collection JSON export. |
| Junk preview | Yes | Yes | Evaluates Default, Strict, or Aggressive junk modes after hydrating cached external metadata into scanned game records. GUI opens Junk from the Library toolbar (`Junk…` panel), not a sidebar item (ADR-0006). Panel: mode → Preview → dense table with per-row selection checkboxes, cover thumbnail, playtime, HLTB, rating, Proton/Deck, matched/missing signals, hidden status, and library-share percentage; bulk "Select all junk"/"Clear"; summary rail (Evaluated, Selected, Threshold, HLTB coverage, Playtime selected). Apply and hide operate on the selected subset only. |
| Junk apply to collection | Yes | Yes | Writes the selected junk candidates to a Steam collection after dry-run/confirmation and backup. GUI action lives in the Library Junk panel (**Apply N selected**). |
| Junk hide | Yes | Yes | Adds the selected junk candidates to Steam's hidden collection after dry-run/confirmation and backup. GUI action lives in the Library Junk panel (**Hide N selected**). |
| Recommendations | Yes | Yes | Recommends from hydrated scanned game records by available minutes, count, installed-only, Deck mode, optional deterministic shuffle seed, and excluded Steam collections (CLI `--exclude-collection`, repeatable; GUI Exclude-collections picker). GUI top-level view is labeled **Recommendations**; Session Planner card with shuffle-seed field, top-3 highlight cards with 220×124 cover images, rank badges, match-percent badges, and compact metadata (HLTB, rating, ProtonDB, playtime), explanation rail (Results, Avg score, Top score, methodology), and full results list with match percent and human reason labels. Seed-by-game similarity lives in Discover (ADR-0005), not here. |
| Temporary recommendation collection | Yes | Yes | CLI `recommend --to-collection --dry-run|--confirm` and GUI Recommendations **Save as Steam collection** write to `vapourfly-picks` after dry-run confirmation. |
| Playlist import | Yes | Yes | Imports Vapourfly JSON playlist files. CLI stores imported playlists under the app data playlist directory. |
| Playlist export | Yes | Yes | CLI exports a stored playlist by ID. GUI exports the currently loaded playlist to a selected JSON path. |
| Playlist match report | Yes | Yes | Reports owned, missing, played, unplayed, hidden, and junk counts for a playlist. |
| Playlist completion price | Yes | Yes | CLI `playlist match`/`playlist import` and GUI match report show the sum of Steam Store final prices for **missing, non-free** Playlist entries with available price data. Owned/unplayed games are never included. Rule Playlists have no missing entries, so their completion price is absent. Single-currency totals return one Money value; mixed currencies return per-currency grouped totals. Price coverage (priced/missing-non-free) is shown when partial. In online mode, missing AppIDs without cache entries are fetched on-demand via the Steam Store API; in offline mode, only cached prices are used. When no price is available, the CLI reports "unavailable — missing entries may be free, unpriced, or not cached". |
| Rule-based playlists | Yes | Yes | CLI `playlist create-rules` and GUI Playlists view create rule-based playlists. CLI/GUI workflows hydrate cached metadata before rule evaluation. Rule operators: `ProtonAtLeast`, `HltbMaxMinutes`, `ControllerSupportFull`, `PlaytimeBetween`, `RatingAtLeast`, `HasGenre`, `HasTag`, `Installed`, `NotJunk`, `NotHidden`, `And`, `Or`, `Not`. |
| Playlist sync to Steam collection | Yes | Yes | CLI `vapourfly sync collection <playlist-id>` and GUI Playlists sync resolve a playlist and write a Steam collection with dry-run/confirmation. |
| Data source cache refresh | Yes | Yes | CLI `cache refresh --source <source>` and GUI Data Sources **Refresh All** / per-row Refresh support `igdb`, `rawg`, `protondb`, `pcgw`, `hltb`, `steam-store`, and `all`. Refresh is disabled while offline or when required credentials are missing. |
| Data source status | Yes | Yes | CLI `sources status`; GUI Data Sources unified table (IGDB, RAWG, ProtonDB, PCGW, HLTB, Steam Store) with credential, entries, stale, last success, and refresh actions. Cache Health panel shows a health-percentage gauge (green/amber/red) with total entries, stale, and source counts. |
| Offline mode | Yes | Yes | CLI `--offline` blocks all network calls, including the bounded Steam Web API name-map request and on-demand Playlist prices. GUI offline toggle lives on **Data Sources**; when enabled it blocks cache refresh and forces cache-only hydration (ADR-0009). |
| Backup list | Yes | Yes | Lists backups next to the Steam cloud storage file. GUI lists backups under **Settings → Backups** (not a top-level sidebar item; ADR-0006). |
| Backup restore | Yes | Yes | Restores a selected backup after confirmation. GUI restore lives under Settings → Backups and remains write-safe. |
| Diagnostics export | Yes | Yes | CLI `vapourfly diagnostics export --out <file>` and GUI Settings diagnostics export write sanitized support data. |
| Settings | Yes | Yes | CLI `settings show` displays resolved config; `settings set <field> <value>` and `settings unset <field>` edit `config.toml`. GUI Settings is the maintenance home: Appearance (Light/Dark theme toggle, persisted in a GUI-only `gui-theme` file), Configuration (Steam directory, account override, store locale, backup retention), detected accounts, write safety, setup diagnostics, backup list/restore, diagnostics export, and About. Summary rail shows Theme, Accounts, Steam dir, Retention, Write safety, and Version. Settable fields: `steam_dir`, `account`, `cc`, `lang`, `backup_retention_count`, `steam_api_key` (user-created Steam Web API key for instant name resolution; plain-text input in GUI Settings with a link to steamcommunity.com/dev/apikey; status echoes are masked, status shown under Data Sources). |
| GUI navigation (sidebar) | — | Yes | Top-level destinations render as Discover, Library, Recommendations, Playlists, Collections, followed by Data Sources and Settings at the bottom. Default landing view is Library. Dual-theme shell (light neutral canvas / dark cool surfaces) uses the same layout with orchid or violet accents and monochrome line icons; theme toggle lives in Settings Appearance and the top chrome (ADR-0006). Junk is a Library panel; backups live under Settings. |
| Playlist creation/editing UI | Yes | Yes | CLI `playlist create`; GUI Playlists view supports load existing, create/edit fields, save, match report with owned preview, export, import file, VF1 share codes, and sync to Steam Collection after dry-run confirmation. The right workspace shows a hero card with cover, content-type badge, creator, local-store label, owned/unplayed stats, and average HLTB; a two-column layout below has Games/Rules/Match tabs on the left and a metadata rail (description, content type, game count, generator badge) on the right. Games tab: search-based Add/Remove from library as primary editor, with comma-separated AppID text input. Rules tab: visual rule tree with per-rule remove, recursive And/Or/Not nesting display, quick-add buttons (Installed, NotHidden, NotJunk, ControllerSupportFull), parameterized rule adders (HasGenre, HasTag, HltbMaxMinutes, ProtonAtLeast, PlaytimeBetween with min≤max validation, RatingAtLeast with 0.0–5.0 validation), and Advanced JSON toggle. No Discover control on this page (ADR-0006). |
| Share codes | Yes | Yes | `VF1:` compact binary playlist codes (ADR-0003) via CLI `playlist share` / `playlist import --code` and GUI copy/import controls. The payload carries content + name + description, DEFLATE-compressed and base64url-encoded. No backward compatibility with the old base64url(JSON) format. |
| Discover / similar-game playlist generation | Yes | Yes | CLI `playlist discover`; GUI **Discover** top-level view (optional game name or AppID seed, count, Generate) shows on-page result cards with scores and human reason labels. Discover owns the entire "similar picks" surface (ADR-0005); Playlists has no Discover control (ADR-0006). On success the GUI writes the stable slot id `discover` and overwrites on regenerate (ADR-0007). Continuation: Open in Playlists (loads the slot for edit/share/sync); optional Sync to Steam Collection remains dry-run confirmed. |
| Dynamic collection templates | Yes | Yes | CLI `collections dynamic <template>`; GUI Playlists **Dynamic** button opens a lightweight chooser for `deck-session` / `finish-it` plus session minutes and count, then generates. Deck Session requires installed, not hidden, not junk, ProtonDB Gold-or-better, PCGW full controller support, and HLTB within the requested session length. On success the GUI writes under stable slot ids `dynamic-deck-session` / `dynamic-finish-it` and overwrites on regenerate (ADR-0007). |
| Editorial Moods | Yes | Yes | CLI `collections mood [name]`; GUI Playlists **Mood** button opens a lightweight chooser for the seven canonical Editorial Moods with opaque criteria (ADR-0004): Today's Biggest Hits, Indie Rising, Friday Party, Deck Guardians, Unopened Treasures, Weekend Marathon, Quick Round. On success the GUI writes under stable slot ids `mood-<id>` (one slot per mood) and overwrites on regenerate (ADR-0007). |

## Current Data Flow

1. `scan` reads local Steam files and builds the library model.
2. `workflow::prepare` hydrates from the local cache (stale entries included) and classifies junk. It does not do bulk network enrichment. With a Steam Web API key it may make one bounded owned-games name-map request (ADR-0009).
3. `scan --enrich`, `cache refresh`, or the GUI's post-scan background populate job fetch missing or stale metadata into the cache. The GUI library re-hydrates when that job completes.
4. Playlist match may fetch Steam Store prices for missing entries unless offline.
5. `--offline` blocks every network call, including the name-map request and price fetches.

For developers, workflow commands call `vapourfly_api::workflow::prepare` and
then re-classify junk with the desired mode if different from Default.

## Supported Data Sources

| Source | Credential | Used for |
|---|---|---|
| IGDB | `VAPOURFLY_IGDB_CLIENT_ID` and `VAPOURFLY_IGDB_CLIENT_SECRET` | Genres, themes, keywords, ratings, time-to-beat, similar games |
| RAWG | `VAPOURFLY_RAWG_KEY` | Genres, tags, ratings, store availability |
| ProtonDB | None | Linux and Steam Deck compatibility tier |
| PCGamingWiki | None | Controller support, Steam Deck notes, fixes URL |
| HLTB | None (enabled in the default build; disable with `--no-default-features`) | Completion times from HowLongToBeat |
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

## GUI Smoke Test

The interactive GUI checklist lives in [gui-smoke-test.md](gui-smoke-test.md).
Run it with `--ui-demo` and at 1024px / 1280px / 1440px before a release.
