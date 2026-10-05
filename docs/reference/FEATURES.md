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
| Library scan | Yes | Yes | CLI `scan`; GUI scans on startup and via the Library rescan button. Both read installed games, playtime, hidden state, and Steam collections. GUI Library has a one-row toolbar: search, scope control (All, Installed, Unplayed, Hidden), a **Filters** popover (Cozy / Story-rich / Great on Deck / Short sessions presets, Steam Deck and controller switches, minimum ProtonDB tier, hide junk / exclude hidden, genre, tag, playtime and HLTB ranges), a sort menu, and a grid/list switch. Active filters show as removable chips. With no search or filter active, the grid view opens on a "continue playing" hero for the most recent game (with Play, which launches or installs the game through Steam) and a Recently played shelf. The virtualized grid of portrait capsules uses Steam artwork from the local Steam cache, downloads missing art in the background, and falls back to generated covers offline. Clicking a game opens a detail sheet. While the library snapshot is being prepared in the background, skeleton cards are shown. Short sessions filters by HLTB main_story_seconds ≤ 120 min (falls back to IGDB time_to_beat when HLTB is absent). |
| Enriched scan output | Yes | Yes | CLI `vapourfly scan --enrich` adds external metadata to that command's output and cache. GUI Library hydrates cached metadata and shows Proton/Deck badges and detail on poster cards when cache entries exist. |
| Library card actions | — | Yes | Every Library card has a context menu (View details, Find similar games, Copy App ID, Open store page); the game sheet offers the same actions. Find similar opens Discover seeded with that AppID (Discover owns the seed-based similar-picks surface — ADR-0005). |
| Collections list | Yes | Yes | CLI lists Steam collections; GUI shows a read-only card grid with a cover collage of up to four members, name, game count, and a Hidden tag when applicable. No member drill-in editor in v1. |
| Collections export | Yes | Yes | CLI `vapourfly collections export --out <file>` and GUI Collections **Export all** (native save dialog) write the same Steam collection JSON export. |
| Junk preview | Yes | Yes | Evaluates Default, Strict, or Aggressive junk modes after hydrating cached external metadata into scanned game records. GUI opens Junk cleanup from the Library header (**Clean up**), not a sidebar item (ADR-0006). Page: mode → **Run preview** → Evaluated / Flagged / Selected tiles and a table with per-row checkbox, cover, verdict, and confidence; optional Keep rows; bulk **Select all flagged** / **Clear**. Apply and hide operate on the selected subset only. |
| Junk apply to collection | Yes | Yes | Writes the selected junk candidates to a Steam collection after dry-run/confirmation and backup. GUI action lives in the Junk cleanup bottom bar (**Add to Junk collection**). |
| Junk hide | Yes | Yes | Adds the selected junk candidates to Steam's hidden collection after dry-run/confirmation and backup. GUI action lives in the Junk cleanup bottom bar (**Hide in Steam**). |
| Recommendations | Yes | Yes | Recommends from hydrated scanned game records by available minutes, count, installed-only, Deck mode, optional deterministic shuffle seed, and excluded Steam collections (CLI `--exclude-collection`, repeatable; GUI Exclude-collections picker). GUI top-level view is labeled **Recommendations**; Session Planner card with shuffle-seed field, top-3 highlight cards with 220×124 cover images, rank badges, match-percent badges, and compact metadata (HLTB, rating, ProtonDB, playtime), explanation rail (Results, Avg score, Top score, methodology), and full results list with match percent and human reason labels. Seed-by-game similarity lives in Discover (ADR-0005), not here. |
| Temporary recommendation collection | Yes | Yes | CLI `recommend --to-collection --dry-run|--confirm` and GUI Recommendations **Save as Steam collection** write to `vapourfly-picks` after dry-run confirmation. |
| Playlist import | Yes | Yes | Imports Vapourfly JSON playlist files. CLI stores imported playlists under the app data playlist directory. |
| Playlist export | Yes | Yes | CLI exports a stored playlist by ID. GUI exports the currently loaded playlist to a selected JSON path. |
| Playlist match report | Yes | Yes | Reports owned, missing, played, unplayed, hidden, and junk counts for a playlist. |
| Playlist completion price | Yes | Yes | CLI `playlist match`/`playlist import` and GUI match report show the sum of Steam Store final prices for **missing, non-free** Playlist entries with available price data. Owned/unplayed games are never included. Rule Playlists have no missing entries, so their completion price is absent. Single-currency totals return one Money value; mixed currencies return per-currency grouped totals. Price coverage (priced/missing-non-free) is shown when partial. In online mode, missing AppIDs without cache entries are fetched on-demand via the Steam Store API; in offline mode, only cached prices are used. When no price is available, the CLI reports "unavailable — missing entries may be free, unpriced, or not cached". |
| Rule-based playlists | Yes | Yes | CLI `playlist create-rules` and GUI Playlists view create rule-based playlists. CLI/GUI workflows hydrate cached metadata before rule evaluation. Rule operators: `ProtonAtLeast`, `HltbMaxMinutes`, `ControllerSupportFull`, `PlaytimeBetween`, `RatingAtLeast`, `HasGenre`, `HasTag`, `Installed`, `NotJunk`, `NotHidden`, `And`, `Or`, `Not`. |
| Playlist sync to Steam collection | Yes | Yes | CLI `vapourfly sync collection <playlist-id>` and GUI Playlists sync resolve a playlist and write a Steam collection with dry-run/confirmation. |
| Data source cache refresh | Yes | Yes | CLI `cache refresh --source <source>` and GUI Data Sources **Refresh All** / per-row Refresh support `igdb`, `rawg`, `protondb`, `pcgw`, `hltb`, `steam-store`, and `all`. Refresh is disabled while offline or when required credentials are missing. |
| Data source status | Yes | Yes | CLI `sources status`; GUI Data Sources shows Sources / Cached entries / Stale / Missing credentials tiles and a table (Steam Store, IGDB, ProtonDB, PCGW, HLTB, RAWG) with credential tag, entries, freshness, last updated, and per-row refresh. |
| Offline mode | Yes | Yes | CLI `--offline` blocks all network calls, including the bounded Steam Web API name-map request and on-demand Playlist prices. GUI offline toggle lives on **Data Sources**; when enabled it blocks cache refresh and forces cache-only hydration (ADR-0009). |
| Controller and Steam Deck | — | Yes | The GUI reads game controllers directly (Steam Deck controls, Xbox-style pads): D-pad / left stick move focus, A selects, B goes back, LB / RB switch pages, LT / RT and the right stick scroll, Y focuses search, Start opens Settings, Select toggles the sidebar. The Library grid and list move card by card. On SteamOS, text fields open Steam's on-screen keyboard. Game Mode opens fullscreen at 1280×800 and the write-confirmation dialog warns that Steam must be closed. `VAPOURFLY_NO_GAMEPAD=1` disables controller input; `--deck` simulates a Deck. Linux installs use `scripts/install-steamos.sh`. |
| Backup list | Yes | Yes | Lists backups next to the Steam cloud storage file. GUI lists backups under **Settings → Backups** (not a top-level sidebar item; ADR-0006). |
| Backup restore | Yes | Yes | Restores a selected backup after confirmation. GUI restore lives under Settings → Backups and remains write-safe. |
| Diagnostics export | Yes | Yes | CLI `vapourfly diagnostics export --out <file>` and GUI Settings diagnostics export write sanitized support data. |
| Settings | Yes | Yes | CLI `settings show` displays resolved config; `settings set <field> <value>` and `settings unset <field>` edit `config.toml`. GUI Settings is the maintenance home: Appearance (Light/Dark theme toggle, persisted in a GUI-only `gui-theme` file), Configuration (Steam directory, account override, store locale, backup retention), detected accounts, write safety, setup diagnostics, backup list/restore, diagnostics export, and About. GUI Settings fields show only what `config.toml` contains, with the value in effect as a placeholder. **Save changes** is enabled after an edit and writes only the changed keys; clearing a field removes its key. Settable fields: `steam_dir`, `account`, `cc`, `lang`, `backup_retention_count`, `steam_api_key` (user-created Steam Web API key for instant name resolution; masked input in GUI Settings with a link to steamcommunity.com/dev/apikey; status echoes are masked, status shown under Data Sources). |
| GUI navigation (sidebar) | — | Yes | Destinations are grouped as Play (Library, Discover, Recommendations), Organize (Playlists, Collections), and System (Data Sources, Settings); the sidebar collapses to icons and is headed by the signed-in Steam profile (cached avatar or initials, persona name). Default landing view is Library. The look follows SteamOS: a darker slate sidebar beside a lighter content panel, Steam blue for selection, Steam green for Play, and large type and controls. A cool-grey light theme shares the layout; dark is the default. The theme toggle lives in the title bar and Settings → Appearance (ADR-0006). Junk is a Library workflow; backups live under Settings. Status and errors appear as toast notifications. |
| Playlist creation/editing UI | Yes | Yes | CLI `playlist create`; GUI Playlists view supports load existing, create/edit fields, save, match report, export, import file, VF1 share codes, and sync to Steam Collection after dry-run confirmation. The left rail lists stored playlists and a **+** menu (blank, template, mood, import file, paste share code). The editor has a hero with cover, editable name and description, game count, average HLTB, and ID, then Games / Rules / Match / Share tabs. Games tab: search-based Add/Remove from library as the primary editor, with an optional direct App ID field. Rules tab: active rules as removable chips (recursive And/Or/Not), quick-add (Installed, NotHidden, NotJunk, ControllerSupportFull), parameterized rule rows (HasGenre, HasTag, HltbMaxMinutes, ProtonAtLeast, PlaytimeBetween with min≤max validation, RatingAtLeast with 0.0–5.0 validation), and a JSON toggle. No Discover control on this page (ADR-0006). |
| Share codes | Yes | Yes | `VF1:` compact binary playlist codes (ADR-0003) via CLI `playlist share` / `playlist import --code` and GUI copy/import controls. The payload carries content + name + description, DEFLATE-compressed and base64url-encoded. No backward compatibility with the old base64url(JSON) format. |
| Discover / similar-game playlist generation | Yes | Yes | CLI `playlist discover`; GUI **Discover** top-level view (optional searchable game seed, pick count 10 / 20 / 40, **Find matches** or **Match my taste** without a seed) shows pick cards with relative match and human reason labels. Discover owns the entire "similar picks" surface (ADR-0005); Playlists has no Discover control (ADR-0006). On success the GUI writes the stable slot id `discover` and overwrites on regenerate (ADR-0007). Continuation: **Open as playlist** (loads the slot for edit/share/sync); optional **Sync to Steam** remains dry-run confirmed. |
| Dynamic collection templates | Yes | Yes | CLI `collections dynamic <template>`; GUI Playlists **+ → From a template** generates `deck-session` / `finish-it`. Deck Session requires installed, not hidden, not junk, ProtonDB Gold-or-better, PCGW full controller support, and HLTB within the requested session length. On success the GUI writes under stable slot ids `dynamic-deck-session` / `dynamic-finish-it` and overwrites on regenerate (ADR-0007). |
| Editorial Moods | Yes | Yes | CLI `collections mood [name]`; GUI Playlists **+ → From a mood** lists the seven canonical Editorial Moods with opaque criteria (ADR-0004): Today's Biggest Hits, Indie Rising, Friday Party, Deck Guardians, Unopened Treasures, Weekend Marathon, Quick Round. On success the GUI writes under stable slot ids `mood-<id>` (one slot per mood) and overwrites on regenerate (ADR-0007). |

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

The interactive GUI checklist lives in [gui-smoke-test.md](../gui-smoke-test.md).
Run it with `--ui-demo` and at 1024px / 1280px / 1440px before a release.
