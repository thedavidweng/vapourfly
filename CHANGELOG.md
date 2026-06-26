# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

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
