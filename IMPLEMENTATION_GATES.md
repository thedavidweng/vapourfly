# Vapourfly Implementation Gates

**Version**: 2026-06-24

> **Phase authority**: See CODING_AGENT_EXECUTION_PLAN.md for detailed phase order, acceptance stamps, and stop conditions.

This file defines the minimum checks before moving between phases.

## Phase 0 -> Phase 1

- [x] PRD and TECH_PLAN agree on `Rust + egui/eframe`.
- [x] Steam collections use `cloud-storage-namespace-1.json`.
- [x] `localconfig.vdf` is documented as read-only playtime source.
- [x] CLI-first route is locked.
- [x] IGDB, RAWG, ProtonDB, PCGW and HLTB strategies are documented.
- [x] License policy is documented.

## Phase 1 exit

- [x] `cargo test --workspace` passes.
- [x] `vapourfly doctor` detects Steam dir, accounts and library folders.
- [x] `vapourfly scan --format json` returns AppID, name, installed, playtime, last_played.
- [x] `vapourfly collections list` reads all active `user-collections.*`.
- [x] Fixtures cover empty/missing `appinfo.vdf`.

## Phase 2 exit

- [x] `junk preview` explains every candidate with matched/missing signals.
- [x] `sync --dry-run` prints a deterministic diff.
- [x] `sync --confirm` creates backup, writes temp file, renames, validates, and prunes old backups.
- [x] Rollback test restores original cloud storage file.
- [x] hidden collection merge deduplicates AppIDs.

## Phase 3 exit

- [x] IGDB token cache refreshes before expiry.
- [x] IGDB Steam AppID mapping works through `external_games`.
- [x] IGDB `game_time_to_beats` maps seconds into Vapourfly time model.
- [x] RAWG missing key produces graceful skip.
- [x] ProtonDB 404 produces `Unknown` tier.
- [x] PCGW redirect and Cargo query are fixture-tested.
- [x] API 429 and 5xx use backoff/stale cache.

## Phase 4 exit

- [x] `recommend --minutes` returns scored recommendations with reasons.
- [x] Playlist import/export schema is stable.
- [x] Playlist matching reports owned/missing/played/unplayed.
- [x] Steam Store price cache estimates missing-game completion cost.

## Phase 5 exit

- [x] GUI calls core write plans rather than writing files directly.
- [x] GUI shows dry-run diff before every write.
- [x] GUI exposes API source status and cache refresh.
- [x] GUI can restore backups.
