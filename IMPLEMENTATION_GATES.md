# Vapourfly Implementation Gates

**Version**: 2026-06-26

> **Single source of truth**: See [CODING_AGENT_EXECUTION_PLAN.md](CODING_AGENT_EXECUTION_PLAN.md) for detailed phase order, acceptance stamps, commands, and stop conditions. This file provides a quick-reference checklist only.

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
- [x] GUI Junk write actions show dry-run diff before writing; backup restore supported.
- [x] GUI displays API source status and credential state.
- [x] GUI backup list with restore support (dry-run diff for junk, direct restore for backups).

## Phase 6 exit

- [x] Recommendation engine with scoring, taste vector, deterministic seed.
- [x] Playlist import/export/match.
- [x] Sync collection through WritePlan.

## Phase 7 exit

- [x] GUI builds on Linux, macOS, and Windows.
- [x] GUI can scan fixtures through core service.
- [x] GUI Junk write actions show dry-run diff before writing.
- [x] GUI has no direct file write code for Steam files.
- [x] GUI displays source status and credential state without secrets.

## Phase 8 exit

- [x] All tests pass in clean checkout (349 tests).
- [x] `cargo check -p vapourfly-api --features hltb_scrape` passes.
- [x] `--allow-steam-running` flag on CLI write commands.
- [x] API enrichment service implemented (`scan --enrich`, `cache refresh`).
- [x] `sources status` reads real cache data.
- [x] `build-release.sh` generates source archive without arguments.
- [x] `Cargo.lock` committed (not gitignored).
- [x] Docs cover installation, safety, API setup, and release process.

## Final Release Gate

- [x] `cargo fmt --all -- --check` passes.
- [x] `cargo clippy --workspace --all-targets -- -D warnings` passes.
- [x] `cargo test --workspace` passes (349 tests).
- [x] Source archive builds cleanly.
- [x] Release candidate checklist complete (docs/release-candidate-checklist.md).
- [x] CHANGELOG has v0.1.0 release entry.
- [x] Execution plan stamps match validated v0.1.0 commit (9123583).
- [x] Known limitations documented honestly.
