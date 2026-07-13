# Vapourfly product-readiness report

Date: 2026-07-13  
Scope: Architecture candidates C1–C8 from the full-codebase review, plus defect
cleanup and CONTEXT/ADR alignment. C9 (models bag split) deferred.

## Fixed defects

| Issue | Fix |
|-------|-----|
| `WritePlan.backup_path` lied (decorative name never written) | `execute_write_plan` / `write::commit` return the real timestamped backup path; plan placeholders are empty until commit |
| Backup retention hardcoded to 5, ignored config | `write::commit_with_retention` + `DEFAULT_BACKUP_RETENTION` aligned with config default; tests with retention=1 |
| CLI/GUI duplicated WriteOp assembly | `core::disposition` owns junk apply/hide, recommend collection, playlist sync |
| CLI/GUI duplicated playlist store I/O | `core::playlist_store` put/get/list |
| Eligibility drift (Finish It admitted Tool/DLC) | `core::eligibility` shared by recommend/discover/mood/dynamic Finish It |
| `signal` depended on `junk::ManualOverrides` | `ManualOverrides` moved to `models`; signal is independent of junk module |
| Manual overrides never loaded in product path | Optional load from `{data}/vapourfly/manual_overrides.json` in `workflow::prepare` |
| Enrichment built a new HTTP client per game for ProtonDB/PCGW/HLTB/RAWG | One client per source batch (rate limiting accumulates) |
| Docs claimed workflows were cache-only only | CONTEXT.md, ADR-0002, API_SOURCES.md describe lazy hydrate + offline |
| Cloud path join string duplicated | `steam::cloud_storage_path` / `CLOUD_STORAGE_RELATIVE` |
| Junk signal / ID mask wording duplicated | `core::display`; CLI/GUI thin wrappers |

## Architecture modules added (deep seams)

- `crates/core/src/disposition.rs` — Steam Collection write disposition  
- `crates/core/src/playlist_store.rs` — Playlist store  
- `crates/core/src/eligibility.rs` — generator eligibility  
- `crates/core/src/display.rs` — shared presentation strings  
- Expanded `write.rs` — commit returns real backup path + retention  

## Entrypoint size

| File | Before | After |
|------|--------|-------|
| `crates/gui/src/main.rs` | ~4752 | ~4665 |
| `crates/cli/src/main.rs` | ~2337 | ~2278 |

LOC drop is modest: domain logic moved into core modules; egui/clap presentation
remains large. Shared domain workflows no longer re-implement WriteOp / store /
eligibility policy. Full GUI view-module split (C6) is deferred polish.

## Remaining risks

1. **GUI still a single large file** — navigation by View is local convention, not modules. Global `Mutex` result channels remain for background jobs.
2. **`enrichment.rs` still wide** — client lifetime fixed; six source loops still copy-paste shaped (further collapse optional).
3. **Cold-cache workflow latency / rate limits** — ADR-0002 trade-off; no negative caching for “no match” API results.
4. **HLTB scrape feature-gated** — without `hltb_scrape`, HLTB fields stay empty without a loud user signal.
5. **No live Steam E2E in CI** — write/backup proven with tempfiles; fixture doctor/scan smoke covers offline library path.
6. **`models.rs` bag** — still a wide DTO warehouse (C9 deferred; navigation improved by new domain modules).

## CONTEXT / ADR alignment

| Doc | Status |
|-----|--------|
| CONTEXT.md Hydration | Updated: lazy fetch default, offline cache-only |
| ADR-0002 | Rewritten as implemented decision |
| API_SOURCES.md | Workflows lazy-fetch; offline is cache-only |
| ADR-0001 write surface | Unchanged; cloud storage only |
| ADR-0003–0005 | Unchanged |

## Deferred polish

- Full GUI split into view modules + non-global job channels (C6)
- models.rs split by context (C9)
- Negative caching for empty external lookups
- Collapse remaining enrichment loop duplication into one policy table
- (none for retention/overrides — GUI `backup_retention()` uses Settings edit field / config; ManualOverrides loaded via `load_default_manual_overrides` on prepare and all re-classify sites)

## Verification summary

- `cargo test --workspace` — pass (core 345, api 66, cli 10, gui 36, doctests ok)
- `cargo build -p vapourfly-cli --release` — pass  
- `cargo build -p vapourfly-gui` — pass  
- Write commit test: reported backup path exists on disk with `vapourfly-backup-` name  
- Eligibility parity: Tool excluded from recommend/discover/mood/Finish It  
- Playlist store put→get round-trip  
- CLI smoke: `doctor` + `scan --offline --format json` on `data/fixtures/steam_minimal`  

Evidence logs live under the implementer scratch dir for the goal harness
(when run in goal mode); this report is the durable in-repo product summary.
