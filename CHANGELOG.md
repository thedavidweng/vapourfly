# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Fixed

- Library Filters retain focused-button Enter/Space activation instead of
  toggling the ancestor popover, using the released GPUI Kit 0.8.0 repair.

### Added

- Account-scoped playstyle traits explain original Completion-focused, Deep diver,
  and Explorer rules, with thresholds and supporting, contrary, and unknown games
  in the existing Library/Discover Evidence dialog and `accounts profile` output.

### Changed

- Discover consumes the same account-scoped canonical profile, progress and
  feedback as Recommendations, with bounded validated IGDB seed links, novelty
  and deterministic variety. CLI and GUI expose full signed reasons, including
  missing seed/activity evidence; cold libraries retain eligible picks.
  `DiscoverReason` now aliases the shared `RecommendReason` (owned code and
  description strings). Existing Manual Playlist formats and write guards
  are unchanged; no persisted migration is needed.

- Owned-library Recommendations now use canonical continuous account taste,
  bounded quality/recency/explicit progress/feedback and deterministic variety.
  Seed affects ties only, never relevance. CLI and GUI show all signed reasons
  and coverage limitations; no completion inference from playtime/duration.
  Story edits invalidate in-flight and displayed personalized picks.

- Unreleased Rust profile API: `profile::derive` now requires the selected account's
  progress map after the activity snapshot argument. `AccountProfile` adds `traits`
  and `TasteProfile` adds `titles`; downstream struct literals must supply these
  fields. The CLI profile JSON gains the corresponding fields. Profiles are
  derived, serialize-only output, not persisted records or a deserialization
  contract; no stored-data migration or defaulted evidence is introduced.

## [0.3.1] - 2026-10-17

### Fixed

- macOS releases now require Developer ID signatures, Apple notarization, and stapled tickets before publication.

## [0.3.0] - 2026-10-04

First public release of Vapourfly, a local-first CLI and desktop GUI for
managing a Steam library like Spotify playlists. Builds are available for
macOS (Apple Silicon), Linux x86_64 (including SteamOS), and Windows x86_64.

Vapourfly is licensed under [AGPL-3.0-only](LICENSE). Contributions require
signing the [Contributor License Agreement](CLA.md), and the name is covered
by the [trademark policy](TRADEMARKS.md).

### Library and setup

- `vapourfly doctor` detects the Steam install, accounts, library folders,
  cloud storage, cache location, and API credential status on macOS, Linux,
  and Windows.
- `vapourfly scan` reads installed games, playtime, hidden state, and Steam
  collections. `--enrich` adds external metadata.
- `vapourfly settings show|set|unset` edits `config.toml` (`steam_dir`,
  `account`, `cc`, `lang`, `backup_retention_count`, `steam_api_key`).

### Playlists and collections

- Manual and rule-based Playlists with composable rules (`ProtonAtLeast`,
  `HltbMaxMinutes`, `ControllerSupportFull`, `PlaytimeBetween`,
  `RatingAtLeast`, `HasGenre`, `HasTag`, `Installed`, `NotJunk`, `NotHidden`,
  `And`, `Or`, `Not`).
- Import, export, and match reports (owned, missing, played, unplayed, hidden,
  junk, and the price to complete a Playlist).
- Compact `VF1:` share codes (ADR-0003).
- Sync a Playlist to a Steam collection, and export Steam collections to JSON.
- Generators: Discover similar-game playlists (ADR-0005), dynamic templates
  `deck-session` and `finish-it`, and seven Editorial Moods (ADR-0004).

### Junk cleanup and recommendations

- Explainable junk detection in Default, Strict, and Aggressive modes, with
  manual overrides. Move junk to a collection or to Steam's hidden list.
- Recommendations by available minutes, count, installed-only, Steam Deck
  mode, excluded collections, and a deterministic shuffle seed. Save picks to
  a temporary Steam collection.

### Data sources

- Metadata from Steam Store, ProtonDB, PCGamingWiki, and legacy HLTB cache
  records without credentials, plus documented IGDB time-to-beat and RAWG with
  your own API keys. An optional Steam Web API key resolves all owned-game
  names in one request. HLTB is no longer fetched or refreshed.
- Cache-first hydration (ADR-0009): commands render from the local cache
  immediately, and missing entries are filled by `cache refresh`,
  `scan --enrich`, or the GUI background job. `--offline` blocks all network
  access.

### Desktop GUI

- GPUI and gpui-component 0.7 app with a SteamOS-style design (ADR-0006):
  Library with real Steam artwork, a "continue playing" hero and a
  virtualized capsule grid; Discover; Recommendations; Playlists with a visual
  rule editor; Collections; Data Sources; and Settings with backups and
  diagnostics.
- Full controller support for the Steam Deck and Xbox-style gamepads, Steam's
  on-screen keyboard on SteamOS, and a fullscreen Game Mode layout.
  `scripts/install-steamos.sh` adds Vapourfly to Steam as a non-Steam game.
- `--ui-demo` runs on isolated demo data. `--theme`, `--view`, `--deck`, and
  `--offline` launch flags are available.

### Safety

- Every write to Steam files requires `--dry-run` or `--confirm`; the GUI
  shows a preview and asks for confirmation. A commit that skipped the
  preview does not compile (ADR-0008).
- Timestamped, SHA-256-checked backups before every write, atomic writes,
  post-write verification with rollback, and `vapourfly backup list|restore`.
- Writes are refused while Steam is running unless `--allow-steam-running` is
  passed.
- Logs and `vapourfly diagnostics export` redact paths, account names, and
  SteamIDs by default.

[Unreleased]: https://github.com/thedavidweng/vapourfly/compare/v0.3.1...HEAD
[0.3.1]: https://github.com/thedavidweng/vapourfly/compare/v0.3.0...v0.3.1
[0.3.0]: https://github.com/thedavidweng/vapourfly/releases/tag/v0.3.0
