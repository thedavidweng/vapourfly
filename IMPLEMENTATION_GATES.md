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

- [ ] `cargo test --workspace` passes.
- [ ] `vapourfly doctor` detects Steam dir, accounts and library folders.
- [ ] `vapourfly scan --format json` returns AppID, name, installed, playtime, last_played.
- [ ] `vapourfly collections list` reads all active `user-collections.*`.
- [ ] Fixtures cover empty/missing `appinfo.vdf`.

## Phase 2 exit

- [ ] `junk preview` explains every candidate with matched/missing signals.
- [ ] `sync --dry-run` prints a deterministic diff.
- [ ] `sync --confirm` creates backup, writes temp file, renames, validates, and prunes old backups.
- [ ] Rollback test restores original cloud storage file.
- [ ] hidden collection merge deduplicates AppIDs.

## Phase 3 exit

- [ ] IGDB token cache refreshes before expiry.
- [ ] IGDB Steam AppID mapping works through `external_games`.
- [ ] IGDB `game_time_to_beats` maps seconds into Vapourfly time model.
- [ ] RAWG missing key produces graceful skip.
- [ ] ProtonDB 404 produces `Unknown` tier.
- [ ] PCGW redirect and Cargo query are fixture-tested.
- [ ] API 429 and 5xx use backoff/stale cache.

## Phase 4 exit

- [ ] `recommend --minutes` returns scored recommendations with reasons.
- [ ] Playlist import/export schema is stable.
- [ ] Playlist matching reports owned/missing/played/unplayed.
- [ ] Steam Store price cache estimates missing-game completion cost.

## Phase 5 exit

- [ ] GUI calls core write plans rather than writing files directly.
- [ ] GUI shows dry-run diff before every write.
- [ ] GUI exposes API source status and cache refresh.
- [ ] GUI can restore backups.
