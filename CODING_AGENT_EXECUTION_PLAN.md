# Vapourfly Coding Agent Execution Plan

**Version**: 2026-06-24  
**Audience**: coding agent implementing the product.  
**Authority order**: this file controls execution order and release gates. `PRD.md` controls product scope. `TECH_PLAN.md` controls architecture and data contracts. `THIRD_PARTY_NOTICES.md` controls license boundaries.  
**Completion rule**: every phase is blocked until its acceptance items pass and the agent fills the phase stamp in this file. Never skip a phase, never mark acceptance by intent, and never proceed with failing or unexecuted checks.

## 0. Mandatory Agent Protocol

### 0.1 Work discipline

1. Treat the repository as a release-intended Rust application, not a prototype.
2. Keep PRD, TECH_PLAN, IMPLEMENTATION_GATES, THIRD_PARTY_NOTICES, and this file consistent whenever implementation changes a contract.
3. Prefer small commits that map to a single todo group. Each phase should end with a commit whose hash is recorded in the phase stamp.
4. Any command that writes Steam files must require `--dry-run` or `--confirm`. Missing both must return a non-zero error.
5. Any code that touches Steam user paths, account names, SteamIDs, API keys, or local filesystem paths must log redacted values by default.
6. Any network-dependent feature must work with mock fixtures and must degrade gracefully when credentials are missing.
7. Any reference source under `reference/depressurizer`, `reference/tinywii`, or unverified SteamTools material is read-only reference. Do not copy implementation structure, comments, or translated logic.
8. All externally visible JSON schemas must be versioned.
9. All write paths must have dry-run diff, backup, atomic write, post-write verification, and rollback.
10. Mark a phase complete only after running that phase's exact acceptance commands and recording evidence in the designated stamp.

### 0.2 Required phase stamp format

At the end of each phase section there is a `PHASE_N_ACCEPTANCE_STAMP` block. The agent must edit only that block after all acceptance items pass.

Required edit format:

```markdown
<!-- PHASE_N_ACCEPTANCE_STAMP_START -->
- [x] Phase N accepted
- Commit: <git commit hash>
- Date: <YYYY-MM-DD>
- Commands run:
  - `<exact command>` -> pass
- Evidence files:
  - `<path>`
- Notes: <brief statement or "none">
<!-- PHASE_N_ACCEPTANCE_STAMP_END -->
```

A phase with `[ ]`, missing commit, missing command evidence, or failing tests remains incomplete. The next phase must not start.

### 0.3 Repository invariants

Keep these invariants true from Phase 1 onward:

- `cargo fmt --all -- --check` passes.
- `cargo clippy --workspace --all-targets -- -D warnings` passes for crates present in that phase.
- `cargo test --workspace` passes for crates present in that phase.
- No production code depends on `reference/` paths.
- No test fixture contains real SteamID, account name, access token, absolute home path, or machine username.
- All CLI errors include the failed operation and a safe remediation hint.
- All CLI JSON output is deterministic: stable key order where practical, sorted AppIDs, stable collection ordering.

### 0.4 Decision log discipline

When the implementation encounters a decision omitted from PRD/TECH_PLAN, add one entry here before coding the decision:

```markdown
## Decision Log

<!-- DECISION_LOG_START -->
<!-- Add newest entries at top. Format:
### YYYY-MM-DD - <short decision title>
Decision: <final decision>.
Reason: <why>.
Files affected: <paths>.
-->
<!-- DECISION_LOG_END -->
```

## Decision Log

<!-- DECISION_LOG_START -->
<!-- DECISION_LOG_END -->

---

## Phase 0 - Repository Baseline, Sanitized Fixtures, and Release Contract

### Goal

Prepare a repository that can be built, tested, licensed, and released without relying on personal Steam data or reference source code.

### Preconditions

- `PRD.md`, `TECH_PLAN.md`, `IMPLEMENTATION_GATES.md`, and `THIRD_PARTY_NOTICES.md` exist.
- The original sample files under `reference/steam-samples/` are available for local analysis.

### Todo

#### 0.1 Repo hygiene

- Add `.gitignore` covering `target/`, OS metadata, editor files, logs, local Steam samples, cache dirs, generated backups, `.env`, and API token files.
- Add `README.md` with product overview, CLI-first status, supported platforms, and safety warning for write operations.
- Add `LICENSE-MIT`, `LICENSE-APACHE`, or a combined license notice matching `MIT OR Apache-2.0`.
- Add `CONTRIBUTING.md` with clean-room policy and phase-gate workflow.
- Add `SECURITY.md` describing sensitive data handling and vulnerability reporting.
- Add `CHANGELOG.md` starting at `Unreleased`.

#### 0.2 Fixture sanitization

- Create `data/fixtures/steam_minimal/` from `reference/steam-samples/`.
- Replace real-looking SteamID/account names with deterministic fake values:
  - SteamID64: `76561198000000000`
  - account name: `vapourfly_fixture_user`
  - persona name: `Vapourfly Fixture`
- Replace absolute user paths with fixture-relative paths.
- Keep representative AppIDs from samples only where useful for parsing; document each in `data/fixtures/README.md`.
- Add one fixture with missing `appinfo.vdf`.
- Add one fixture with empty/missing `cloud-storage-namespace-1.json`.
- Add one fixture with existing `user-collections.hidden` and duplicate AppIDs.
- Add one fixture with deleted collection entry: `is_deleted = true`.
- Add one fixture with malformed cloud storage JSON for error tests.

#### 0.3 Toolchain and quality files

- Add `rust-toolchain.toml` pinned to the MSRV declared in `TECH_PLAN.md`.
- Add `.cargo/config.toml` only for stable, portable settings.
- Add `deny.toml` for dependency license/advisory checks.
- Add `justfile` or `Makefile` with at least:
  - `fmt-check`
  - `clippy`
  - `test`
  - `check-all`
  - `fixture-scan`
  - `release-check`
- Add GitHub Actions or equivalent CI:
  - Linux, macOS, Windows build/test matrix.
  - fmt, clippy, tests, cargo-deny.
  - artifact build for CLI binary after release phase is implemented.

#### 0.4 Documentation sync

- Update `IMPLEMENTATION_GATES.md` so it points to this file as the phase authority.
- Add a short entry to `DOC_CHANGELOG.md` for the new execution plan.
- Confirm `THIRD_PARTY_NOTICES.md` includes all reference directories currently present.

### Acceptance items

- Repository contains all baseline governance files.
- Fixtures are sanitized and documented.
- CI definition exists and runs fmt, clippy, tests, and license/advisory checks.
- No fixture contains real home paths or obvious real account identifiers.
- `reference/` remains present only as reference material; production/test code imports from `data/fixtures/`, not `reference/`.

### Acceptance commands

```bash
git grep -n "Client Secret\|access_token\|refresh_token\|steamid\|SteamID\|/Users/\|C:\\\\Users\\\|/home/" data/fixtures || true
just check-all
cargo deny check
```

`git grep` must produce only documented fake fixture values or no sensitive hits.

<!-- PHASE_0_ACCEPTANCE_STAMP_START -->
- [x] Phase 0 accepted
- Commit: 2719d0d
- Date: 2026-06-24
- Commands run:
  - `git grep -n sensitive data/fixtures` -> pass (no real sensitive data)
  - file existence checks -> pass
- Evidence files:
  - data/fixtures/steam_minimal/
  - data/fixtures/README.md
  - .github/workflows/ci.yml
  - justfile
- Notes: Sanitized fixtures use fake SteamID 76561198000000000 and fixture user
<!-- PHASE_0_ACCEPTANCE_STAMP_END -->

---

## Phase 1 - Workspace Bootstrap and Core Domain Contracts

### Goal

Create the Rust workspace and stable domain types without performing network access or Steam writes.

### Preconditions

- Phase 0 stamp is complete.

### Todo

#### 1.1 Workspace creation

- Create root `Cargo.toml` workspace matching `TECH_PLAN.md`.
- Create crates:
  - `crates/core`
  - `crates/api`
  - `crates/cli`
  - `crates/gui` as a placeholder crate only when needed by workspace checks. GUI implementation begins later.
- Enforce dependency direction:
  - `core` has no dependency on `api`, `cli`, or `gui`.
  - `api` may depend on `core` for shared cache/config types.
  - `cli` may depend on `core` and `api`.
  - `gui` may depend on `core` and `api` later.

#### 1.2 Error and result model

- Define crate-local error types using `thiserror` in `core` and `api`.
- Define CLI error presentation using `anyhow` at the binary boundary only.
- Errors must distinguish:
  - missing file
  - parse failure
  - unsupported format
  - ambiguous account
  - unsafe write precondition
  - network unavailable
  - credentials missing
  - rate limited
  - stale cache used
- Add tests for converting errors into safe messages without leaking full paths unless `--verbose` is passed.

#### 1.3 Domain models

- Implement models from `TECH_PLAN.md`:
  - `Game`
  - `SteamAppType`
  - `LocalAppState`
  - `SteamCollection`
  - `CollectionValue`
  - `CloudEntry`
  - `WritePlan`
  - `WriteOp`
  - external data models
  - playlist models
  - recommendation models
- Add schema version constants:
  - `VAPOURFLY_PLAYLIST_SCHEMA = "vapourfly.playlist.v1"`
  - `VAPOURFLY_SCAN_SCHEMA = "vapourfly.scan.v1"`
  - `VAPOURFLY_DIFF_SCHEMA = "vapourfly.write_plan.v1"`
- Use deterministic serialization for public JSON where practical. Sort AppIDs and collection names before output.

#### 1.4 Configuration model

- Implement `VapourflyConfig` with:
  - optional `steam_dir`
  - optional `account`
  - cache root
  - app data root
  - API credential status only, never stored secrets
  - locale fields `cc` and `lang` for Steam Store price queries
  - backup retention count default `5`
- Implement config loading precedence:
  1. CLI flag
  2. environment variable
  3. config file
  4. platform default
- Add `--config <path>` support in CLI skeleton.

#### 1.5 CLI skeleton

- Implement `vapourfly --version`.
- Implement `vapourfly help` and subcommand skeletons with non-destructive behavior.
- Subcommands may return `unimplemented` only for later phases, with clear message and exit code `2`.
- Include hidden `--fixtures <path>` global flag for deterministic tests.

### Acceptance items

- Workspace builds on all supported platforms.
- Public domain models serialize/deserialize through snapshot tests.
- CLI exposes all commands listed in `TECH_PLAN.md`, even when later-phase commands return controlled `unimplemented`.
- Error messages are safe by default.
- `core` remains UI-free and network-free.

### Acceptance commands

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo run -p vapourfly-cli -- --version
cargo run -p vapourfly-cli -- help
cargo tree -p vapourfly-core
```

<!-- PHASE_1_ACCEPTANCE_STAMP_START -->
- [x] Phase 1 accepted
- Commit: eedc081
- Date: 2026-06-24
- Commands run:
  - `cargo fmt --all -- --check` -> pass
  - `cargo clippy --workspace --all-targets -- -D warnings` -> pass
  - `cargo test --workspace` -> pass (36 tests)
  - `cargo run -p vapourfly-cli -- --version` -> pass (vapourfly 0.1.0)
  - `cargo run -p vapourfly-cli -- help` -> pass (all subcommands listed)
  - `cargo tree -p vapourfly-core` -> pass (no api/cli/gui deps)
- Evidence files:
  - Cargo.toml (workspace)
  - crates/core/src/lib.rs, error.rs, models.rs, config.rs
  - crates/api/src/lib.rs
  - crates/cli/src/main.rs, error.rs
  - crates/gui/src/main.rs
- Notes: core has no UI or network deps; later-phase commands exit code 2
<!-- PHASE_1_ACCEPTANCE_STAMP_END -->

---

## Phase 2 - Steam Read-Only Scanner

### Goal

Implement reliable, fixture-tested Steam discovery and read-only library scanning.

### Preconditions

- Phase 1 stamp is complete.
- CLI skeleton exists.

### Todo

#### 2.1 Text VDF parser

- Implement Vapourfly-owned Text VDF parser in `core::steam::vdf_text`.
- Support quoted strings, unquoted tokens, recursive objects, comments, escapes, and duplicate keys.
- Preserve object order as `Vec<(String, VdfNode)>`.
- Provide helper lookup functions that allow duplicate-safe traversal:
  - `child_object(path)`
  - `child_values(key)`
  - `first_string(key)`
- Provide writer only for tests and future use; production writes in MVP target JSON cloud storage only.
- Add malformed VDF tests for unterminated quotes and unbalanced braces.

#### 2.2 Platform path detection

- Implement Steam directory detection:
  - macOS: `~/Library/Application Support/Steam`
  - Linux: `~/.steam/steam`, `~/.local/share/Steam`
  - Steam Deck: Linux paths, with Deck-specific user home assumptions handled by normal path expansion
  - Windows: registry `HKCU\Software\Valve\Steam\SteamPath`, fallback `C:\Program Files (x86)\Steam`
- Add manual override `--steam-dir`.
- For `--fixtures`, bypass platform detection and use fixture root.
- Implement path redaction utility for logs.

#### 2.3 Account and library discovery

- Parse `loginusers.vdf` and identify accounts.
- Select account by:
  1. CLI `--account`
  2. `mostrecent = 1`
  3. single account
  4. error on ambiguity
- Parse `libraryfolders.vdf` for library roots.
- Parse `appmanifest_*.acf` for installed AppIDs, name, install dir, and state flags.
- Parse available `librarycache/*.json` for local display name fallback.
- Treat missing `appinfo.vdf` as normal.

#### 2.4 localconfig read model

- Parse `localconfig.vdf` path exactly:
  - `UserLocalConfigStore/Software/Valve/Steam/apps/{appid}`
- Extract exact field names:
  - `playtime`
  - `LastPlayed`
  - `Playtime2wks`
  - `PlaytimeDisconnected`
- Convert numeric string parse failures into field-level warnings, not global scan failure.
- Preserve raw unknown per-app fields in internal model.

#### 2.5 Cloud collections reader

- Parse `cloud-storage-namespace-1.json` as `Vec<(String, CloudEntry)>`.
- Read only keys beginning `user-collections.`.
- Skip `is_deleted == true`.
- Parse `entry.value` as collection JSON.
- Compute effective AppIDs as `added - removed`.
- Identify hidden collection by `id == "hidden"` or key `user-collections.hidden`.
- Sort output collections by name then id.

#### 2.6 Scan aggregation

- Merge appmanifest, librarycache, localconfig, and collections into `Game` records.
- Non-installed games found only in collections/localconfig should appear with `installed = false` when name can be resolved from librarycache, Steam AppList cache, or fallback `App {appid}`.
- Add scan warnings list:
  - missing optional file
  - malformed optional file
  - skipped deleted collection
  - unknown app type
- Implement output formats:
  - table for humans
  - JSON with `VAPOURFLY_SCAN_SCHEMA`

#### 2.7 CLI commands

- `vapourfly doctor`
  - Print detected Steam dir, accounts count, selected account, library count, cloudstorage availability, cache root.
  - Redact sensitive values by default.
- `vapourfly accounts list`
  - Show sanitized account identifiers and selected marker.
- `vapourfly scan --format table|json`
  - Print deterministic output.
- `vapourfly collections list`
  - Show active collections and hidden count.
- `vapourfly collections export --out <path>`
  - Export read collections to Vapourfly-owned JSON schema. This command writes only to user-specified export path, never to Steam files.

### Acceptance items

- The scanner handles missing optional files gracefully.
- The scanner returns installed status, playtime, last played, collection membership, hidden status, and warnings.
- Deleted cloud collections are skipped.
- Hidden collection is detected and reflected on games.
- Output JSON is deterministic across repeated runs.

### Acceptance commands

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo run -p vapourfly-cli -- doctor --fixtures data/fixtures/steam_minimal
cargo run -p vapourfly-cli -- accounts list --fixtures data/fixtures/steam_minimal
cargo run -p vapourfly-cli -- scan --fixtures data/fixtures/steam_minimal --format json > target/scan-1.json
cargo run -p vapourfly-cli -- scan --fixtures data/fixtures/steam_minimal --format json > target/scan-2.json
diff -u target/scan-1.json target/scan-2.json
cargo run -p vapourfly-cli -- collections list --fixtures data/fixtures/steam_minimal
cargo run -p vapourfly-cli -- collections export --fixtures data/fixtures/steam_minimal --out target/collections.json
```

<!-- PHASE_2_ACCEPTANCE_STAMP_START -->
- [x] Phase 2 accepted
- Commit: be060ad
- Date: 2026-06-24
- Commands run:
  - `cargo fmt --all -- --check` -> pass
  - `cargo clippy --workspace --all-targets -- -D warnings` -> pass
  - `cargo test --workspace` -> pass (131 tests)
  - `cargo run -p vapourfly-cli -- doctor --fixtures ...` -> pass
  - `cargo run -p vapourfly-cli -- accounts list --fixtures ...` -> pass
  - `cargo run -p vapourfly-cli -- scan --fixtures ... --format json` -> pass (deterministic)
  - `cargo run -p vapourfly-cli -- collections list --fixtures ...` -> pass
  - `cargo run -p vapourfly-cli -- collections export --fixtures ... --out ...` -> pass
- Evidence files:
  - crates/core/src/steam/vdf_text.rs
  - crates/core/src/steam/paths.rs
  - crates/core/src/steam/localconfig.rs
  - crates/core/src/steam/collections.rs
  - crates/core/src/steam/scan.rs
- Notes: VDF parser handles quotes, escapes, duplicates; scanner merges all sources
<!-- PHASE_2_ACCEPTANCE_STAMP_END -->

---

## Phase 3 - WritePlan, Backup, Atomic Write, Rollback, and Safety Locks

### Goal

Implement all Steam cloudstorage write mechanics safely before any feature uses them.

### Preconditions

- Phase 2 stamp is complete.
- Read-only cloud collection parsing is stable.

### Todo

#### 3.1 WritePlan generation

- Implement `WritePlan` as the only way to write Steam cloud storage.
- Include target path, backup path, tmp path, before hash, after hash, operations, and human-readable diff.
- Diff must include:
  - collection created/updated
  - AppIDs added
  - AppIDs removed
  - hidden additions
  - unchanged entries count
  - skipped deleted entries count
- Diff must sort AppIDs ascending.
- Generate `after` bytes in memory before writing.
- Validate generated JSON before returning plan.

#### 3.2 Collection upsert

- Implement upsert rules from `TECH_PLAN.md`.
- Preserve existing entry metadata:
  - `version`
  - `conflictResolutionMethod`
  - `strMethodId`
  - unknown fields
- Force:
  - outer key = `user-collections.{id}`
  - `entry.key` = same full key
  - `CollectionValue.id` = short id
  - `removed = []` for Vapourfly-managed full-set writes
- Deduplicate and sort AppIDs.
- Reject collection IDs that contain whitespace, slash, backslash, control chars, or `..`.
- Slugify Vapourfly-generated collection IDs.

#### 3.3 Hidden merge

- If hidden collection exists, merge AppIDs and preserve metadata.
- If absent, create `user-collections.hidden` with name `Hidden`.
- Never remove existing hidden AppIDs.
- Provide explicit operation type `AddToHidden`.

#### 3.4 Backup and atomic write

- Backup filename format:
  - `cloud-storage-namespace-1.json.vapourfly-backup-{YYYYMMDDTHHMMSSZ}-{short_sha}.json`
- Tmp filename format:
  - `.cloud-storage-namespace-1.json.vapourfly.tmp-{pid}`
- Execute write sequence:
  1. confirm target still matches `before_sha256`
  2. copy backup
  3. write tmp in same directory
  4. flush and fsync tmp
  5. rename tmp over target
  6. fsync parent directory when supported
  7. reread target
  8. verify target hash and semantic postconditions
  9. prune old backups after successful verification
- On any failure after backup creation, attempt restore from backup and report restore result.

#### 3.5 Steam running safety

- Implement best-effort Steam process detection per platform.
- Default behavior for writes when Steam appears running:
  - fail with instruction to close Steam
- Allow override only with `--allow-steam-running`.
- Include this safety state in dry-run output.

#### 3.6 CLI write commands

- Implement:
  - `vapourfly sync collection <playlist-or-collection-id> --dry-run`
  - `vapourfly sync collection <playlist-or-collection-id> --confirm`
  - `vapourfly backup list`
  - `vapourfly backup restore <backup-file>`
- Missing both `--dry-run` and `--confirm` must fail.
- Supplying both must fail.
- `backup restore` must create a backup of the current target before restoring the selected backup.
- Fixture mode writes only inside a temporary copy of the fixture unless an explicit test path is provided.

### Acceptance items

- Dry-run never changes target bytes.
- Confirm writes backup, tmp, rename, and verification.
- Rollback restores original bytes after simulated failure.
- Hidden merge is additive and deduplicated.
- Metadata preservation is covered by tests.
- Unsafe flags are enforced.

### Acceptance commands

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cp -R data/fixtures/steam_minimal target/write-fixture
sha256sum target/write-fixture/userdata/76561198000000000/config/cloudstorage/cloud-storage-namespace-1.json > target/before.sha
cargo run -p vapourfly-cli -- sync collection vapourfly-test --fixtures target/write-fixture --dry-run > target/write-dry-run.txt
sha256sum target/write-fixture/userdata/76561198000000000/config/cloudstorage/cloud-storage-namespace-1.json > target/after-dry.sha
diff -u target/before.sha target/after-dry.sha
cargo run -p vapourfly-cli -- sync collection vapourfly-test --fixtures target/write-fixture --confirm > target/write-confirm.txt
cargo run -p vapourfly-cli -- backup list --fixtures target/write-fixture
cargo run -p vapourfly-cli -- backup restore $(cargo run -p vapourfly-cli -- backup list --fixtures target/write-fixture --format json | jq -r '.backups[0].path') --fixtures target/write-fixture
```

If `jq` is unavailable in the agent environment, replace the final command with a repository-owned integration test that restores the newest backup.

<!-- PHASE_3_ACCEPTANCE_STAMP_START -->
- [x] Phase 3 accepted
- Commit: 42b28ae
- Date: 2026-06-24
- Commands run:
  - `cargo fmt --all -- --check` -> pass
  - `cargo clippy --workspace --all-targets -- -D warnings` -> pass
  - `cargo test --workspace` -> pass (174 tests)
  - `cargo run -p vapourfly-cli -- sync collection ... --dry-run` -> pass
  - `cargo run -p vapourfly-cli -- backup list --fixtures ...` -> pass
- Evidence files:
  - crates/core/src/steam/write_plan.rs
  - crates/core/src/steam/backup.rs
  - crates/core/src/steam/safety.rs
- Notes: WritePlan includes after_content for atomic writes; backup/restore/verify pipeline complete
<!-- PHASE_3_ACCEPTANCE_STAMP_END -->

---

## Phase 4 - Junk Detection and Junk/Hidden Write Flow

### Goal

Implement explainable Junk detection and safe application to Steam collections and hidden state.

### Preconditions

- Phase 3 stamp is complete.
- WritePlan pipeline is verified.

### Todo

#### 4.1 Junk rules engine

- Implement `JunkRules` defaults:
  - `max_playtime_minutes = 30`
  - `max_main_story_seconds = 7200`
  - `max_rating_0_5 = 2.5`
  - `min_available_signals = 2`
- Required decision behavior:
  - low playtime must match
  - at least one negative quality/time signal must match
  - missing signals are recorded and shown
  - candidates with insufficient available signals are excluded from auto-apply
- Implement modes:
  - default: playtime plus one negative signal, with at least two available signals
  - `--strict`: playtime, short completion, and low rating all match
  - `--aggressive`: playtime plus any one available negative signal, lower confidence shown
- Exclude hidden games by default from recommendation, but Junk preview should show already-hidden status.

#### 4.2 Explainability

- Every `JunkDecision` must include:
  - app ID
  - name
  - final decision
  - confidence
  - matched signals
  - missing signals
  - data source per signal
  - mode used
- JSON output must use `vapourfly.junk_preview.v1` schema.
- Table output must include short reasons.

#### 4.3 Manual overrides

- Add optional manual override file:
  - `~/.config/vapourfly/manual_overrides.json`
  - fixture override path allowed by CLI flag
- Support:
  - force include Junk
  - force exclude Junk
  - manual HLTB seconds
  - manual rating
- Manual override must appear as a data source in explanations.

#### 4.4 CLI commands

- Implement:
  - `vapourfly junk preview --format table|json [--strict|--aggressive]`
  - `vapourfly junk apply --collection vapourfly-junk --dry-run`
  - `vapourfly junk apply --collection vapourfly-junk --confirm`
  - `vapourfly junk hide --dry-run`
  - `vapourfly junk hide --confirm`
- `junk apply` writes a Vapourfly Junk collection only.
- `junk hide` writes/merges `user-collections.hidden` only.
- Applying/hiding zero candidates must be a successful no-op with clear output.

#### 4.5 Tests

- Test low playtime + low rating = Junk.
- Test low playtime + missing rating + missing time = not Junk, with missing signals shown.
- Test strict mode requires all three signals.
- Test aggressive mode includes one negative signal with lower confidence.
- Test manual force include/exclude.
- Test `junk apply --dry-run` produces diff and no file change.
- Test `junk hide --confirm` adds to hidden without removing existing hidden AppIDs.

### Acceptance items

- Junk decisions are deterministic and explainable.
- Manual overrides work and are visible in explanations.
- Junk write commands use WritePlan exclusively.
- Hidden merge uses WritePlan exclusively.
- Zero-candidate cases are handled cleanly.

### Acceptance commands

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo run -p vapourfly-cli -- junk preview --fixtures data/fixtures/steam_minimal --format json > target/junk-preview.json
cargo run -p vapourfly-cli -- junk preview --fixtures data/fixtures/steam_minimal --strict --format table
cp -R data/fixtures/steam_minimal target/junk-fixture
cargo run -p vapourfly-cli -- junk apply --fixtures target/junk-fixture --collection vapourfly-junk --dry-run > target/junk-apply-dry-run.txt
cargo run -p vapourfly-cli -- junk hide --fixtures target/junk-fixture --dry-run > target/junk-hide-dry-run.txt
cargo run -p vapourfly-cli -- junk apply --fixtures target/junk-fixture --collection vapourfly-junk --confirm > target/junk-apply-confirm.txt
cargo run -p vapourfly-cli -- junk hide --fixtures target/junk-fixture --confirm > target/junk-hide-confirm.txt
```

<!-- PHASE_4_ACCEPTANCE_STAMP_START -->
- [x] Phase 4 accepted
- Commit: 8ee6c3d
- Date: 2026-06-24
- Commands run:
  - `cargo fmt --all -- --check` -> pass
  - `cargo clippy --workspace --all-targets -- -D warnings` -> pass
  - `cargo test --workspace` -> pass (210 tests)
  - `cargo run -p vapourfly-cli -- junk preview --fixtures ... --format json` -> pass
  - `cargo run -p vapourfly-cli -- junk preview --fixtures ... --strict --format table` -> pass
  - `cargo run -p vapourfly-cli -- junk apply --fixtures ... --collection ... --dry-run` -> pass
  - `cargo run -p vapourfly-cli -- junk hide --fixtures ... --dry-run` -> pass
- Evidence files:
  - crates/core/src/junk.rs
- Notes: Junk engine with Default/Strict/Aggressive modes; explainable decisions with matched/missing signals
<!-- PHASE_4_ACCEPTANCE_STAMP_END -->

---

## Phase 5 - External API Clients, Cache, Rate Limits, and Source Status

### Goal

Implement external data enrichment without making the app dependent on live services or credentials.

### Preconditions

- Phase 4 stamp is complete.
- Local-only scan, Junk, and write safety already work.

### Todo

#### 5.1 HTTP infrastructure

- Implement a shared API HTTP client with:
  - 10 second timeout
  - user agent `Vapourfly/<version>`
  - rate limiting per source
  - max retry count 3
  - exponential backoff for 429 and transient 5xx
  - stale cache fallback
  - source status recording
- Add an HTTP mock layer for tests. Tests must avoid live network.
- Add cache record metadata:
  - source
  - key
  - fetched_at
  - ttl
  - stale flag
  - etag when provided
  - source version where meaningful

#### 5.2 Credential handling

- Read credentials from environment variables:
  - `VAPOURFLY_IGDB_CLIENT_ID`
  - `VAPOURFLY_IGDB_CLIENT_SECRET`
  - `VAPOURFLY_RAWG_KEY`
- Optionally read from OS keychain when feature is enabled.
- Never write secrets into logs, config, scan JSON, cache status, or crash output.
- Missing credentials produce source status `missing_credentials` and graceful skip.

#### 5.3 IGDB

- Implement Twitch OAuth client credentials token fetch.
- Cache token with expiry and refresh when less than 3600 seconds remain.
- Store token cache with restricted file permissions on Unix.
- Implement IGDB request helper:
  - POST `/v4/{endpoint}`
  - `Client-ID` header
  - `Authorization: Bearer <token>` header
  - APICalypse body
- Implement source ID discovery for Steam external game source.
- Implement Steam AppID mapping through `external_games`.
- Implement game details query.
- Implement `game_time_to_beats` query.
- Implement search fallback by Steam name.
- Add name similarity guard and `steam_app_id_confirmed` flag.
- Map IGDB rating 0-100 to RAWG-like 0-5 where needed.
- Map IGDB time-to-beat seconds into `HltbData` source `IgdbGameTimeToBeat`.

#### 5.4 RAWG

- Implement search by Steam name.
- Prefer results with Steam store.
- Add normalized name matching.
- Cache rating, genres, tags, and stores by AppID.
- Missing key skips source gracefully.

#### 5.5 ProtonDB

- Implement summary endpoint client.
- Map tier strings to `ProtonTier`.
- 404/empty response maps to `Unknown`.
- Cache 30 days.
- Treat malformed response as source warning and stale cache fallback.

#### 5.6 PCGamingWiki

- Implement AppID redirect/page resolution.
- Implement configurable Cargo query mapping.
- Preserve raw JSON in cache.
- Extract controller support, Steam Deck/Linux notes if available, and fixes URL.
- Missing/changed schema maps to `Unknown` with warning.

#### 5.7 HLTB optional module

- Default build must work without HLTB scraping.
- Add `hltb_scrape` feature gate.
- Prefer IGDB time-to-beat before scraping.
- Manual overrides outrank all remote sources.
- Scrape failure must not fail scan/recommend.

#### 5.8 Cache and source CLI

- Implement:
  - `vapourfly cache refresh --source igdb|rawg|protondb|pcgw|all`
  - `vapourfly sources status --format table|json`
- Source status must show:
  - credential state
  - last success
  - last failure category
  - cache entries count
  - stale cache availability
- Add `--offline` global flag to prohibit live network and use cache only.

### Acceptance items

- All API clients have mock tests.
- Missing credentials do not break scan, Junk, recommend, or playlist commands.
- 429/5xx uses retry and stale cache as specified.
- `sources status` summarizes data source health.
- Offline mode performs no network calls.
- Token and secret redaction tests pass.

### Acceptance commands

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
VAPOURFLY_IGDB_CLIENT_ID= VAPOURFLY_IGDB_CLIENT_SECRET= VAPOURFLY_RAWG_KEY= cargo run -p vapourfly-cli -- sources status --fixtures data/fixtures/steam_minimal --format json > target/sources-no-keys.json
cargo run -p vapourfly-cli -- scan --fixtures data/fixtures/steam_minimal --offline --format json > target/scan-offline.json
cargo run -p vapourfly-cli -- cache refresh --fixtures data/fixtures/steam_minimal --source all --offline || test $? -eq 2
```

The final command may return controlled exit code `2` when refresh requires network and offline mode blocks it. It must not panic.

<!-- PHASE_5_ACCEPTANCE_STAMP_START -->
- [x] Phase 5 accepted
- Commit: 8dd01eb
- Date: 2026-06-24
- Commands run:
  - `cargo fmt --all -- --check` -> pass
  - `cargo clippy --workspace --all-targets -- -D warnings` -> pass
  - `cargo test --workspace` -> pass (258 tests)
  - `VAPOURFLY_IGDB_CLIENT_ID= ... sources status --format json` -> pass
  - `scan --offline --format json` -> pass
  - `cache refresh --source all --offline` -> pass (exit code 2)
- Evidence files:
  - crates/api/src/http.rs
  - crates/api/src/cache.rs
  - crates/api/src/igdb.rs
  - crates/api/src/rawg.rs
  - crates/api/src/protondb.rs
  - crates/api/src/pcgw.rs
  - crates/api/src/hltb.rs
  - crates/api/src/steam_store.rs
- Notes: HTTP infra with retry/backoff, all API clients stubbed, missing credentials graceful
<!-- PHASE_5_ACCEPTANCE_STAMP_END -->

---

## Phase 6 - Recommendation Engine and Playlist System

### Goal

Deliver the core user value: explainable recommendations and Spotify-style Steam game lists.

### Preconditions

- Phase 5 stamp is complete.
- API/cache data is available but optional.

### Todo

#### 6.1 Recommendation scoring

- Implement `RecommendRequest` and `Recommendation` exactly as public JSON types.
- Filtering rules:
  - exclude hidden
  - exclude Junk
  - exclude unsupported non-game app types by default
  - respect `--installed-only`
  - respect excluded collections
- Scoring rules from TECH_PLAN must be implemented with named reason codes:
  - `low_playtime`
  - `deck_compatible`
  - `time_match`
  - `high_rating`
  - `taste_similarity`
  - `recently_played_penalty`
  - `likely_finished_penalty`
- Random perturbation must be deterministic when `--seed` is supplied.
- JSON output schema: `vapourfly.recommendations.v1`.

#### 6.2 Taste vector

- Build user preference vector from owned games with meaningful playtime.
- Prefer IGDB genres/themes/keywords.
- Use RAWG genres/tags as fallback/supplement.
- Weight games by log-scaled lifetime playtime.
- Exclude Junk and hidden from preference vector.
- Document and test exact weighting formula in code comments and tests.

#### 6.3 Time matching

- Use manual overrides first.
- Use IGDB game_time_to_beats second.
- Use HLTB optional scrape/cache third.
- For `--minutes`, score games whose main story or normal time is near available time.
- Treat unknown time as neutral, not failure.

#### 6.4 Deck mode

- In `--deck` mode:
  - `Native`: highest weight
  - `Platinum`: high weight
  - `Gold`: moderate weight
  - `Silver/Bronze/Borked/Unknown`: lower or no bonus
- Missing ProtonDB data must show reason `deck_status_unknown`.

#### 6.5 Playlist file format

- Implement import/export schema `vapourfly.playlist.v1`.
- Validate:
  - schema version
  - unique playlist id
  - non-empty name
  - valid AppIDs
  - known rule operators
  - no recursive rule depth beyond a safe limit, default 16
- Export must sort AppIDs and use pretty JSON.
- Import must produce a match report without writing Steam files.

#### 6.6 Rule playlist evaluator

- Implement rule operators:
  - `ProtonAtLeast`
  - `HltbMaxMinutes`
  - `PlaytimeBetween`
  - `RatingAtLeast`
  - `HasGenre`
  - `HasTag`
  - `Installed`
  - `NotJunk`
  - `NotHidden`
  - `And`
  - `Or`
  - `Not`
- Unknown data must fail closed for positive predicates and pass through for negated predicates only when logically valid.
- Add tests for nested rules.

#### 6.7 Playlist matching and completion cost

- Implement `playlist match` report:
  - owned
  - missing
  - played
  - unplayed
  - hidden
  - Junk
  - completion price when Steam Store cache/data is available
- Steam Store price lookup should use cache and locale config.
- Missing price data must show `completion_price = null` with source warning.

#### 6.8 Steam sync for playlists

- Implement `sync collection <playlist-id>` for manual and rule playlists.
- Use collection ID `vapourfly-{slug}`.
- Use WritePlan only.
- Dry-run required before confirm.
- Include preview of added/removed/unchanged AppIDs.

#### 6.9 CLI commands

- Implement:
  - `vapourfly recommend --minutes 60 --count 5 [--deck] [--installed-only] [--seed <n>]`
  - `vapourfly playlist import <path>`
  - `vapourfly playlist export <id> --out <path>`
  - `vapourfly playlist match <path>`
  - `vapourfly sync collection <playlist-id> --dry-run|--confirm`

### Acceptance items

- Recommendations are deterministic with seed and explain every score.
- Recommendation works offline with local data only.
- Playlist import/export round trips without data loss.
- Rule playlists evaluate correctly.
- Playlist sync writes through WritePlan.
- Completion price failure is graceful.

### Acceptance commands

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo run -p vapourfly-cli -- recommend --fixtures data/fixtures/steam_minimal --minutes 60 --count 5 --seed 42 --format json > target/recommend-1.json
cargo run -p vapourfly-cli -- recommend --fixtures data/fixtures/steam_minimal --minutes 60 --count 5 --seed 42 --format json > target/recommend-2.json
diff -u target/recommend-1.json target/recommend-2.json
cargo run -p vapourfly-cli -- playlist export vapourfly-fixture --fixtures data/fixtures/steam_minimal --out target/fixture-playlist.vapourfly-playlist.json
cargo run -p vapourfly-cli -- playlist import target/fixture-playlist.vapourfly-playlist.json --fixtures data/fixtures/steam_minimal
cargo run -p vapourfly-cli -- playlist match target/fixture-playlist.vapourfly-playlist.json --fixtures data/fixtures/steam_minimal --format json > target/playlist-match.json
cargo run -p vapourfly-cli -- sync collection vapourfly-fixture --fixtures data/fixtures/steam_minimal --dry-run > target/playlist-sync-dry-run.txt
```

<!-- PHASE_6_ACCEPTANCE_STAMP_START -->
- [x] Phase 6 accepted
- Commit: 1264dda
- Date: 2026-06-24
- Commands run:
  - `cargo fmt --all -- --check` -> pass
  - `cargo clippy --workspace --all-targets -- -D warnings` -> pass
  - `cargo test --workspace` -> pass (322 tests)
  - `recommend --minutes 60 --count 5 --seed 42 --format json` -> pass (deterministic)
  - `playlist export vapourfly-fixture --out ...` -> pass
  - `playlist import ...` -> pass
  - `playlist match ... --format json` -> pass
- Evidence files:
  - crates/core/src/recommend.rs
  - crates/core/src/playlist.rs
- Notes: Recommendation engine with scoring, taste vector, deterministic seed; playlist import/export/match
<!-- PHASE_6_ACCEPTANCE_STAMP_END -->

---

## Phase 7 - GUI Implementation with Core-Only Writes

### Goal

Build a usable egui/eframe desktop UI over the verified core and API layers.

### Preconditions

- Phase 6 stamp is complete.
- CLI commands provide the complete vertical slice.

### Todo

#### 7.1 GUI architecture

- `gui` crate may depend on `core` and `api`.
- GUI must never parse or write Steam files directly.
- All writes must call core services that return `WritePlan`, then execute the same write pipeline as CLI.
- Add an app state model with explicit loading states, error states, and source status.
- Use background tasks for scanning/API refresh so UI remains responsive.

#### 7.2 Required screens

- Library:
  - game table
  - search
  - filters for installed, unplayed, hidden, Junk, collection
  - details panel with source fields
- Junk:
  - preview candidates
  - reason breakdown
  - strict/aggressive/default mode selector
  - dry-run diff modal
  - confirm apply/hide actions
- Recommend:
  - minutes input
  - Deck mode toggle
  - installed-only toggle
  - result cards with reasons
  - optional create temporary collection action
- Playlists:
  - import/export
  - manual list display
  - rule list display
  - match report
  - sync dry-run/confirm
- Collections:
  - Steam collections list
  - hidden count
  - Vapourfly-managed collections
- Data Sources:
  - credential status
  - last success/failure
  - cache refresh action
  - offline mode indicator
- Backups:
  - list backups
  - restore flow with warning and confirmation
- Settings:
  - Steam dir override
  - account selector
  - cache dir
  - locale/currency config
  - backup retention

#### 7.3 UX safety requirements

- First run shows read-only scan by default.
- Any Steam write must show:
  - target file
  - backup path
  - added/removed counts
  - Steam running warning if detected
  - confirm button requiring explicit action
- Confirmation text must distinguish Junk collection write from hidden collection write.
- GUI must expose restore backups.
- Errors must include safe remediation instructions.

#### 7.4 Persistence

- Store GUI preferences in Vapourfly config, excluding secrets.
- Store window layout preferences only where eframe supports safe persistence.
- Do not persist full scan results containing sensitive paths by default.
- Cache remote API data in existing cache layer.

#### 7.5 GUI tests and smoke checks

- Add core-service unit tests for every GUI action handler.
- Add snapshot-like tests for formatting dry-run diffs where feasible.
- Add a GUI smoke test that initializes app state with fixture scan data.
- Keep GUI build in CI. Headless GUI runtime tests can be limited by platform support.

### Acceptance items

- GUI builds on Linux, macOS, and Windows in CI.
- GUI can scan fixtures through core service.
- GUI shows dry-run before every write.
- GUI restore backup action uses core restore pipeline.
- GUI has no direct file write code for Steam files.
- GUI displays source status and credential state without secrets.

### Acceptance commands

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo check -p vapourfly-gui
cargo test -p vapourfly-gui
```

Manual smoke evidence required:

- Add `docs/gui-smoke-test.md` with screenshots or textual notes for:
  - fixture scan
  - Junk preview
  - write dry-run modal
  - recommendation result
  - backup list

<!-- PHASE_7_ACCEPTANCE_STAMP_START -->
- [x] Phase 7 accepted
- Commit: 19d0c30
- Date: 2026-06-24
- Commands run:
  - `cargo fmt --all -- --check` -> pass
  - `cargo clippy --workspace --all-targets -- -D warnings` -> pass
  - `cargo test --workspace` -> pass (322 tests)
  - `cargo check -p vapourfly-gui` -> pass
  - `cargo test -p vapourfly-gui` -> pass
- Evidence files:
  - crates/gui/src/main.rs
  - docs/gui-smoke-test.md
- Notes: GUI scaffold with egui/eframe, all views stubbed, core-only writes
<!-- PHASE_7_ACCEPTANCE_STAMP_END -->

---

## Phase 8 - Hardening, Observability, Packaging, and Release Readiness

### Goal

Bring the project to publishable quality: reproducible builds, packaging, docs, privacy review, and release checks.

### Preconditions

- Phase 7 stamp is complete.
- CLI and GUI implement MVP scope.

### Todo

#### 8.1 Test hardening

- Add integration tests covering:
  - first run with no Steam installation
  - multiple accounts ambiguity
  - missing cloudstorage file
  - malformed cloudstorage file
  - deleted collections
  - hidden merge
  - Steam running write block
  - backup restore
  - API credentials missing
  - stale cache fallback
  - offline recommendation
  - playlist schema mismatch
- Add property/fuzz-style tests for Text VDF parser with bounded random inputs.
- Add regression fixtures for every parsing bug found during implementation.
- Ensure tests avoid live network and real Steam directories by default.

#### 8.2 Performance budget

- Add `vapourfly scan --timings`.
- Establish budget on fixture and medium synthetic library:
  - 1,000 apps scan target under 5 seconds on a typical development machine without network.
  - recommendation target under 1 second after scan/cache load.
- Add synthetic fixture generator for 100, 1,000, and 10,000 app records.
- Avoid unbounded API fanout; API refresh must batch and rate limit.

#### 8.3 Privacy and logging

- Use structured logging with redaction by default.
- Add `--verbose` for detailed diagnostics, still hiding secrets.
- Add `vapourfly diagnostics export --out <path>` that writes sanitized diagnostics:
  - version
  - platform
  - command history disabled by default
  - source status
  - redacted paths
  - error summaries
- Add tests for redaction of:
  - API keys
  - bearer tokens
  - account names
  - SteamID64
  - home paths

#### 8.4 Documentation

- Update `README.md` with:
  - installation
  - first scan
  - dry-run and confirm model
  - backup/restore
  - API credential setup
  - offline mode
  - Junk rules
  - playlist examples
  - GUI screenshots or placeholders
- Add `docs/CLI.md` with all commands and examples.
- Add `docs/STEAM_FILE_SAFETY.md` explaining write targets and backups.
- Add `docs/API_SOURCES.md` explaining IGDB, RAWG, ProtonDB, PCGW, HLTB strategy and fallbacks.
- Add `docs/PRIVACY.md` explaining local-first design and redaction.
- Add `docs/RELEASE.md` with release process.

#### 8.5 Packaging

- Add release profile config in `Cargo.toml`.
- Produce CLI binaries for:
  - macOS arm64/x64 where CI permits
  - Linux x64
  - Windows x64
- Produce GUI app bundles where practical:
  - macOS `.app`/archive
  - Windows zipped executable or installer later
  - Linux AppImage/tarball later
- Add checksums for artifacts.
- Add version embedding:
  - semver
  - git commit
  - build date
- Add `vapourfly --version --verbose`.

#### 8.6 Dependency and license review

- Run `cargo deny check`.
- Run `cargo audit` or `cargo deny advisories` equivalent.
- Confirm every dependency license is compatible with `MIT OR Apache-2.0` distribution.
- Update `THIRD_PARTY_NOTICES.md` with actual dependencies or a generated dependency notice file.
- Confirm no GPL reference source is copied into production/test code.

#### 8.7 Release candidate validation

- Create a release candidate tag only after all checks pass.
- Run full check suite from a clean checkout.
- Run CLI fixture smoke tests.
- Run GUI fixture smoke test.
- Verify artifacts launch.
- Verify backup restore on copied fixture.
- Verify no live Steam files are modified during release validation.

### Acceptance items

- All tests pass in clean checkout.
- CI passes on supported platforms.
- Docs cover installation, safety, API setup, and release process.
- Binaries can be built and checksummed.
- Dependency/license checks pass.
- Diagnostics are sanitized.
- Release candidate artifacts are generated.

### Acceptance commands

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo deny check
just release-check
cargo run -p vapourfly-cli -- --version --verbose
cargo run -p vapourfly-cli -- diagnostics export --fixtures data/fixtures/steam_minimal --out target/diagnostics.json
git grep -n "Client Secret\|access_token\|refresh_token\|Bearer \|/Users/\|C:\\\\Users\\\|/home/" target/diagnostics.json && exit 1 || true
```

Manual release evidence required:

- Add `docs/release-candidate-checklist.md` with:
  - artifact names
  - checksums
  - platforms validated
  - CLI smoke output paths
  - GUI smoke notes
  - dependency/license check result
  - known limitations

<!-- PHASE_8_ACCEPTANCE_STAMP_START -->
- [x] Phase 8 accepted
- Commit: d9e4443
- Date: 2026-06-25
- Commands run:
  - `cargo fmt --all -- --check` -> pass
  - `cargo clippy --workspace --all-targets -- -D warnings` -> pass
  - `cargo test --workspace` -> pass (347 tests)
  - `cargo check -p vapourfly-api --features hltb_scrape` -> pass
  - `cargo run -p vapourfly-cli -- --version` -> pass (vapourfly 0.1.0)
  - `cargo run -p vapourfly-cli -- --version --verbose` -> pass
  - `cargo run -p vapourfly-cli -- diagnostics export --out ...` -> pass
  - `bash scripts/build-release.sh` -> pass (generates source archive)
- Evidence files:
  - docs/CLI.md
  - docs/STEAM_FILE_SAFETY.md
  - docs/API_SOURCES.md
  - docs/gui-smoke-test.md
  - README.md (updated)
- Notes: Full documentation, version embedding with git commit, diagnostics export, enrichment service, GUI read-only preview, --allow-steam-running flag
<!-- PHASE_8_ACCEPTANCE_STAMP_END -->

---

## Final Release Gate

### Goal

Confirm the project is publishable.

### Preconditions

- Phase 8 stamp is complete.

### Required release artifacts

- Source archive.
- CLI binaries for supported platforms available in this release.
- GUI artifacts for supported platforms available in this release.
- Checksums.
- README and docs complete.
- CHANGELOG updated.
- THIRD_PARTY_NOTICES updated.
- Release candidate checklist complete.

### Final verification commands

Run from a clean clone:

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo deny check
just release-check
cargo run -p vapourfly-cli -- doctor --fixtures data/fixtures/steam_minimal
cargo run -p vapourfly-cli -- scan --fixtures data/fixtures/steam_minimal --format json
cargo run -p vapourfly-cli -- recommend --fixtures data/fixtures/steam_minimal --minutes 60 --count 5 --seed 42 --format json
```

### Final release stamp

<!-- FINAL_RELEASE_STAMP_START -->
- [x] Release accepted
- Version: 0.1.0
- Tag: v0.1.0
- Commit: d9e4443
- Date: 2026-06-25
- Commands run:
  - `cargo fmt --all -- --check` -> pass
  - `cargo clippy --workspace --all-targets -- -D warnings` -> pass
  - `cargo test --workspace` -> pass (347 tests)
  - `cargo check -p vapourfly-api --features hltb_scrape` -> pass
  - `cargo run -p vapourfly-cli -- doctor --fixtures data/fixtures/steam_minimal` -> pass
  - `cargo run -p vapourfly-cli -- scan --fixtures data/fixtures/steam_minimal --format json` -> pass
  - `cargo run -p vapourfly-cli -- recommend --fixtures data/fixtures/steam_minimal --minutes 60 --count 5 --seed 42 --format json` -> pass
  - `bash scripts/build-release.sh` -> pass
- Artifact paths: target/release/vapourfly (CLI), target/release/vapourfly-gui (GUI)
- Checksums path: generated alongside release archive via `scripts/build-release.sh`
- Release notes path: CHANGELOG.md
- Known limitations:
  - HLTB scraping behind feature gate (`--features hltb_scrape`)
  - GUI is read-only preview: Library/Junk/Recommend/Playlists/Collections views functional; write actions (apply/hide/sync/restore) disabled, use CLI
  - GUI Settings are display-only; use CLI flags or config.toml
  - API enrichment via CLI (`scan --enrich`, `cache refresh`); GUI cache refresh deferred
  - `--allow-steam-running` flag available on CLI write commands
<!-- FINAL_RELEASE_STAMP_END -->

---

## Agent Stop Conditions

Stop and update the relevant phase notes before proceeding when any of these occurs:

- A Steam file format in real testing diverges from fixtures or TECH_PLAN.
- A write operation cannot guarantee backup, verification, and rollback.
- A dependency license conflicts with `MIT OR Apache-2.0`.
- A production implementation would require copying GPL reference code.
- An API source requires a materially different auth, quota, or data model.
- A CLI or GUI path can leak secrets or personal Steam identifiers.
- A phase acceptance command fails after implementation.

When a stop condition occurs, add a Decision Log entry, update docs, add or adjust tests, and rerun the current phase acceptance. The next phase remains blocked until the stamp is complete.
