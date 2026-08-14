# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.2.0] - 2026-08-14

First packaged release after the v0.1.0 source drop. Breaking: share-code
format, removed playlist-radio / old mood template, hydration is cache-first
(ADR-0009), and the desktop GUI is GPUI rather than egui.

### Added

#### Playlist sharing, generators, and moods

- **`VF1:` compact binary share codes** (ADR-0003): `vapourfly playlist share <id>` / `vapourfly playlist import --code <code>` and GUI copy/import controls. The payload carries content + name + description, compressed and base64url-encoded. **Breaking:** the old `VF1:<base64url(JSON)>` format no longer decodes.
- **Discover** (ADR-0005): `vapourfly playlist discover [--seed <appid>] [--count <n>]` and a GUI top-level Discover view generate a similar-picks playlist from the user's taste vector, optionally seeded by an AppID. Discover owns the entire seed-based similar-picks surface.
- **Dynamic collection templates** (transparent rules): `vapourfly collections dynamic deck-session|finish-it` and a GUI chooser in Playlists.
- **Editorial Moods** (ADR-0004, opaque curated criteria): `vapourfly collections mood [name]` and a GUI chooser for the seven canonical moods (Today's Biggest Hits, Indie Rising, Friday Party, Deck Guardians, Unopened Treasures, Weekend Marathon, Quick Round).
- **Generator playlist slots** (ADR-0007): GUI generations write stable playlist ids (`discover`, `dynamic-deck-session`, `dynamic-finish-it`, `mood-<id>`) and overwrite on regenerate.

#### GUI redesign (ADR-0006)

- **GPUI desktop shell** (gpui 0.2 + gpui-component 0.5) replacing egui/eframe. Dual-theme design-system chrome (light neutral / dark cool) with persisted theme toggle, monochrome line icons, and the sidebar IA Discover, Library, Recommendations, Playlists, Collections + Data Sources, Settings.
- Library: responsive card grid with deterministic illustrated offline covers, search + scope segments (All/Installed/Unplayed/Hidden), labelled Deck/Controller/playtime/genre/tag/sort filters, editorial category chips, advanced ProtonDB/HLTB/exclusion controls, 48-per-page **Load more** pagination, insights rail, skeleton loading cards, and card actions (Discover similar, Copy AppID, Open Steam Store).
- Junk moved into a Library toolbar panel: dense preview table with per-row selection, bulk select, summary rail (threshold, HLTB coverage, focus reclaimed); apply/hide operate on the selected subset only.
- Recommendations: Session Planner (minutes, count, quick presets, shuffle seed, exclude-collections picker, Deck/installed toggles), top-3 highlight cards with covers and match-percent badges, explanation rail, and a "Why this pick?" panel.
- Playlists: master-detail layout — left rail of stored playlists with covers and generator badges, hero card, Games/Rules/Match tabs, search-based game adding, a visual rule tree editor with quick-add and parameterized rule adders (Genre, Tag, HLTB max, ProtonDB tier, Playtime range, Rating min) and a data-loss-safe Advanced JSON toggle.
- Data Sources: unified source table (credentials, entries, stale, last success, refresh) plus a Cache Health gauge; offline toggle lives here.
- Settings: grouped cards (Appearance, Configuration, accounts, write safety, setup diagnostics, backups, diagnostics export, About) with a summary rail.
- Collections: card grid with poster collages and an **Export all** action (native save dialog).
- `--ui-demo` mode: fully isolated demo data (no real Steam I/O, no CDN fetches, deterministic placeholder art).
- Background job runner: junk preview, recommendations, discover, dynamic, mood, and playlist match all run off the UI thread with staleness-checked job tickets.
- GitHub Release workflow: tag `v*` builds CLI + GUI archives for macOS (arm64/x86_64), Linux x86_64, and Windows x86_64.

#### CLI

- `vapourfly recommend --exclude-collection <NAME>` (repeatable) — exclude games in specific Steam collections from recommendations.
- `vapourfly settings show [--format json]` — display resolved Vapourfly configuration and the config file path.
- `vapourfly settings set <field> <value>` — write a config field to `config.toml` (`steam_dir`, `account`, `cc`, `lang`, `backup_retention_count`, `steam_api_key`).
- `vapourfly settings unset <field>` — remove a config field from `config.toml`.
- `vapourfly playlist create-rules --id --name --description --rules <file>` — create and store a rule-based playlist from a JSON rules array or a full playlist file.
- `vapourfly playlist match` and `vapourfly playlist import` now print the completion price line (with a hint when no Steam Store price is cached).

#### Core

- Shared domain seams: `workflow::prepare` (scan + lazy hydration + junk classification, ADR-0002), `disposition` (Steam collection write assembly), `playlist_store`, `eligibility`, and `display` modules shared by CLI and GUI.
- Manual junk overrides loaded from `{data}/vapourfly/manual_overrides.json` in all product paths.
- `PriceCoverage` split into confirmed free / non-free / unknown categories for accurate completion-price reporting.
- `vapourfly_core::config::ConfigField` enum and `set_config_field` / `unset_config_field` functions for programmatic config editing.
- `vapourfly_core::models::Money::format()` method for rendering prices as major-unit currency strings.

### Changed

#### CLI

- `vapourfly backup restore` now requires either `--dry-run` or `--confirm` (matching the write-safety convention used by other write commands). `--dry-run` previews the restore (showing backup and current SHA-256 hashes) without writing; `--confirm` executes the restore. The previous implicit-confirm behaviour is removed.
- `--steam-dir` and `--account` are now global flags and may be placed after the subcommand (e.g. `vapourfly doctor --steam-dir /path`), matching the documented "available on all commands" contract.
- `vapourfly scan --enrich` now applies cached entries (including stale ones) to its output, so `--offline` surfaces stale cache data instead of dropping it (consistent with the ADR-0002 hydration contract).
- `vapourfly diagnostics export` now includes the sanitized fields documented in PRIVACY.md: redacted Steam/cache directories (full paths with `--verbose`), account and library-folder counts, and active warnings.

#### Instant first paint & real-environment fixes (ADR-0009)

- **Instant first paint**: `workflow::prepare` no longer does bulk network enrichment — scan + cache-only hydration + junk classification render in seconds on any library size (previously ~86 minutes on a real 865-game library with a cold cache). The GUI auto-starts one background populate job per launch after the first scan (missing/stale entries only, visible under Data Sources); the library re-hydrates when it completes. Explicit `cache refresh` / `scan --enrich` remain the forced populate paths. `--offline` still means zero network. Supersedes ADR-0002's lazy fetch; the graceful-degradation contract is unchanged.
- **HLTB works again and is on by default**: migrated to howlongtobeat.com's current `bleed` endpoints (session init + token/hp-pair auth, browser UA required); the removed `/api/search` + `/api/games` endpoints are gone. `hltb_scrape` is now a default feature.
- **PCGW migrated to the Cargo API**: single joined `Infobox_game` + `Input` query resolves AppID → page + controller support; the Semantic MediaWiki `action=ask` endpoint (removed by PCGW in 2022) is gone. Steam Deck notes are no longer populated (no Cargo field exists; Deck data comes from ProtonDB).
- **Steam Store parsing fixed for real responses**: the API returns genre ids as strings but category ids as numbers; the unused `id` field that broke every response is removed.
- **Name resolution without appinfo.vdf** (the normal state on macOS): placeholder `"App <id>"` names resolve instantly via `IPlayerService/GetOwnedGames` when a `VAPOURFLY_STEAM_API_KEY` is configured (one cached request; Valve removed the keyless `GetAppList`), and progressively from Steam Store hydration otherwise. Name-keyed sources (HLTB, RAWG) skip placeholder-named games instead of querying and caching garbage. `scan` warns when names stay unresolved; `doctor` reports the key status.
- **TTLs and rate limits retuned for background repopulation**: ProtonDB 1d→7d, Steam Store 1d→3d, PCGW/IGDB/RAWG →14d; per-source rate limits (steam-store 30/min, protondb/pcgw 120/min, igdb 200/min, hltb 30/min).
- GUI accepts `--offline` at launch (parity with the CLI flag and the Data Sources toggle).
- **Guided Steam Web API key onboarding**: users create their own free key (per Valve's API terms the key is personal and never bundled with the app). New `steam_api_key` config field — `vapourfly settings set steam_api_key <key>`, or the input under GUI Settings → Configuration with a link to steamcommunity.com/dev/apikey; Data Sources shows configured/not-configured status with setup guidance; `doctor`/`settings show` report it masked. Env `VAPOURFLY_STEAM_API_KEY` still takes precedence over the config file.
- **Library design-fidelity pass** against the reference screens: cards stretch to fill the grid row with aspect-correct cover art (Steam 460×215 header capsule) and no dead space below the action row; genre chips use per-tag hue tints (IGDB genres, Steam Store fallback); editorial category chips carry their own icon + tint (Cozy pink, Story-rich blue, Great on Deck green, Short sessions orange); filter dropdowns gained leading icons; top-chrome metrics sit flush right; the insights rail shows cover thumbnails in Recent activity, a thin backlog meter, and a View-full-history shortcut.

#### Architecture (ADR-0008)

- **Act-half workflow verbs**: the evaluate → disposition → preview assembly for Junk apply/hide, recommendation collection, and Playlist sync now lives in `vapourfly_core::actions` (one verb per workflow); the two-pass Playlist match with missing-entry store prices lives in `vapourfly_api::workflow::match_playlist_full`. CLI and GUI call the verbs instead of wiring the pipelines independently — rule-Playlist sync resolution now has exactly one implementation.
- **Type-guarded confirmation gate**: `write::preview` returns a `PreviewedPlan` (no public constructor) and `write::commit`/`commit_with_retention` accept only that type — a commit that skipped preview no longer compiles. The GUI path that could commit a junk write whose diff was never displayed is removed; only backup restore executes without a stored plan.
- **One adapter per enrichment source**: the cache/offline/fetch/stale-fallback protocol is one generic state-machine bound to a per-source adapter (cache key + TTL + field + fetch). Cache-key derivation is owned by the enrichment module (writers and `hydrate_from_cache` cannot drift), credentials resolve once at the seam (`SourceCredentials`), and `HttpClient` is cheaply cloneable (shared backend + rate limiter) so a mock client can exercise all six sources — previously 4 of 6 wirings were untestable. `Ok(None)` ("source has no data") is now uniformly counted as skipped.
- **Eligibility and signal vocabulary**: `match_playlist` report buckets derive unplayed/hidden/junk through `eligibility` predicates instead of inline re-derivation; the raw HLTB signal has one accessor (`signal::main_story_seconds`); the ≤4h/≥20h session thresholds live in `scoring` (`is_short_game`/`is_long_game`) instead of per-mood magic numbers.
- New tests: `workflow::prepare` and `match_playlist_full` (previously zero coverage), CLI write-handler dry-run/gate tests, `core::actions` verb tests, enrichment wiring tests (stale fallback, offline short-circuit, credential skip), and direct `scoring`/`signal` precedence tests.

#### Core

- Recommendation `time_match` now requires a known main-story completion time (HLTB, falling back to IGDB time-to-beat) that fits the available window; games without completion-time data no longer receive the signal, and the undocumented 15-minute minimum window is removed.
- Recommendation `high_rating` is now a true independent OR (RAWG ≥ 4.0 **or** IGDB ≥ 80) instead of source precedence, matching the PRD.
- Environment variables (`VAPOURFLY_CC`, `VAPOURFLY_LANG`) now override config-file values, matching the documented CLI > env > file > default precedence.
- `config::load_config_table_at` now returns an error when the config file exists but cannot be parsed, instead of silently replacing it with an empty table. A missing file still yields an empty table so first-run creation works.
- `steam::appinfo::lookup_appinfo_names` no longer takes a generic `BuildHasher` parameter; it accepts a standard `HashSet<u32>`. No caller used a custom hasher.
- Dependency refresh: GPUI / gpui-component and workspace crates. The egui/eframe 0.35 bump from the previous cycle is superseded.

#### GUI

- Library card "Recommend" action is now **Discover similar**: it opens Discover seeded with that game's AppID (seed-based similarity belongs to Discover — ADR-0005). The Recommendations seed field is relabelled **Shuffle seed** and documents its real semantics (deterministic result ordering).
- Settings panel now writes `config.toml` via the shared `vapourfly_core::config` batch API (`apply_config_updates`) instead of a duplicated read-modify-write implementation. All field updates are validated before any write and persisted in a single atomic write (temp file + rename), so the file is never left in a partially-updated, truncated, or corrupt state — even if the process is interrupted mid-write.

### Fixed

- `sources status` (CLI) and the Data Sources table + Cache Health gauge (GUI) now compute staleness from each cache record's age and TTL; previously the persisted `stale` flag was read literally and was always `false`, so stale counts were permanently 0 and the health gauge was permanently green.
- Recommendation reason text for recently played games reported an inverted day count ("Played 14 days ago" for a game played today); it now reports the actual days.
- `WritePlan.backup_path` reports the real timestamped backup path created at commit; backup retention honours `backup_retention_count` from config.

### Removed

- **`playlist-radio` dynamic template** (breaking, ADR-0005): `collections dynamic playlist-radio` and the corresponding GUI entry are gone; Discover with a seed AppID covers every playlist-radio scenario.
- The old tag/genre-filter `mood` dynamic template (ADR-0004), replaced by Editorial Moods with curated hidden criteria.
- egui/eframe desktop implementation, replaced by GPUI.

## [0.1.0] - 2026-06-26

Initial release of Vapourfly — a local-first CLI/GUI tool for managing Steam game libraries.

### Added

#### CLI

- `vapourfly doctor` — diagnose Steam installation, detect accounts, library folders, and cloud storage.
- `vapourfly scan [--format json] [--enrich]` — scan the Steam library with optional external API enrichment.
- `vapourfly collections list` — list active Steam collections from cloud storage.
- `vapourfly junk preview [--strict|--aggressive]` — detect junk games with signal breakdown.
- `vapourfly junk apply|hide --dry-run|--confirm` — write junk classification to Steam collections.
- `vapourfly recommend --minutes <n> [--deck] [--installed-only] [--seed <n>]` — get game recommendations.
- `vapourfly playlist import|export|match` — import/export/match playlists against the library.
- `vapourfly sync collection <id> --dry-run|--confirm` — sync playlists to Steam collections.
- `vapourfly cache refresh --source <src>` — refresh external API cache (igdb, rawg, protondb, pcgw, hltb, all).
- `vapourfly sources status` — show external data source status and cache statistics.
- `vapourfly backup list|restore` — list and restore Steam cloud storage backups.
- `vapourfly diagnostics export` — export sanitized diagnostics.
- `--allow-steam-running` flag on all write commands.
- `--offline` flag to prohibit network calls.
- `--verbose` flag for detailed output with full paths.
- Version embedding with semver, git commit hash, and build date.

#### GUI

- Multi-view desktop UI built with egui/eframe.
- Library view with game table, search, and filters (installed, unplayed, hidden, junk).
- Junk detection view with mode selector, signal breakdown, and write actions (apply to collection / add to hidden) with dry-run diff preview.
- Recommendations view with minutes/count/deck/installed-only controls.
- Playlists view with import and match report.
- Collections view listing Steam collections.
- Data Sources view showing credential status and cache info.
- Backups view listing available backups.
- Settings view with editable fields that preserve existing IGDB/RAWG credentials on save.

#### Core

- Text VDF parser for Steam configuration files.
- Cloud storage JSON parser for modern collections.
- Steam account detection (macOS, Linux, Windows registry).
- Library folder detection from `libraryfolders.vdf`.
- Playtime extraction from `localconfig.vdf`.
- Write plan engine with diff, backup, atomic write, post-write verification, and rollback.
- Junk detection engine with configurable modes (default, strict, aggressive).
- Recommendation engine with scoring, taste vector, and deterministic seed.
- Playlist evaluation with rule operators (And, Or, Not, ProtonAtLeast, HltbMaxMinutes, etc.).
- Backup management with SHA-256 checksums and retention policy.

#### API

- HTTP client with rate limiting, exponential backoff, and mock backend for testing.
- IGDB client (Twitch OAuth) with game details and time-to-beat.
- RAWG client with game search by name.
- ProtonDB client with tier lookup by Steam AppID.
- PCGamingWiki client with controller support and Steam Deck compatibility.
- HLTB client behind `hltb_scrape` feature gate.
- Steam Store client for app details and pricing.
- Disk cache with TTL-based freshness and stale-while-revalidate semantics.
- Enrichment service bridging scan results with external API data (IGDB, RAWG, ProtonDB, PCGW, HLTB, Steam Store).
- Steam Store price enrichment wired into playlist completion cost estimation.

#### Safety

- Steam process detection before write operations.
- Write safety checks: file existence, parent directory writability, Steam running detection.
- Automatic backup creation before every write.
- Atomic writes (temp file + rename).
- Post-write verification with rollback on failure.
- Redacted logging by default (paths, account names, SteamIDs).

### Known Limitations

- HLTB scraping is behind the `hltb_scrape` feature gate and not included in default builds.
- GUI Junk write actions (apply to collection, add to hidden) show dry-run diff before writing; backup restore also supported.
- GUI Settings are editable and preserve existing IGDB/RAWG credentials on save.
- GUI cache refresh is available from Data Sources; scan enrichment output remains CLI-only.
- IGDB enrichment requires credentials; games without credentials fall back to cache.
- `cargo deny check` requires `cargo-deny` installed separately.

[Unreleased]: https://github.com/thedavidweng/vapourfly/compare/v0.2.0...HEAD
[0.2.0]: https://github.com/thedavidweng/vapourfly/compare/v0.1.0...v0.2.0
[0.1.0]: https://github.com/thedavidweng/vapourfly/releases/tag/v0.1.0
