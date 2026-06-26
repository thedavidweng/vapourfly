# Vapourfly v0.1.0 Release Candidate Checklist

**Version**: 0.1.0
**Tag**: v0.1.0
**Date**: 2026-06-25
**Commit**: 79b8695

## Release Type

v0.1.0 is a **source-only release**. No pre-built binaries are shipped.
Users build from source using `cargo build --release` or the provided
`scripts/build-release.sh` script.

Pre-built binary releases (CLI + GUI for macOS/Linux/Windows) are planned
for a future version.

## Artifacts

| Artifact | Path | Description |
|---|---|---|
| Source archive | `target/release-artifacts/vapourfly-0.1.0-source.tar.gz` | Full source tree |
| Checksum | `target/release-artifacts/vapourfly-0.1.0-source.tar.gz.sha256` | SHA-256 of source archive |

### Source Archive Checksum

```
a4946ebea8efc23138fa615a2762d3b5bf539875410ebfdd34d6c6055c4d6467  vapourfly-0.1.0-source.tar.gz
```

## Platforms Validated

| Platform | Build | Tests | Notes |
|---|---|---|---|
| macOS (aarch64) | ✅ | ✅ 349 passed | Primary dev platform |

Cross-platform builds (Linux, Windows) have not been validated in this
release candidate. The codebase uses only cross-platform Rust std and
egui/eframe; no platform-specific build steps are expected to fail.

## CLI Smoke Test

```bash
cargo run -p vapourfly-cli -- --version
# vapourfly 0.1.0

cargo run -p vapourfly-cli -- doctor --fixtures data/fixtures/steam_minimal
# Detects fixture Steam dir, account, library folders

cargo run -p vapourfly-cli -- scan --fixtures data/fixtures/steam_minimal --format json
# Returns AppID, name, installed, playtime, last_played

cargo run -p vapourfly-cli -- recommend --fixtures data/fixtures/steam_minimal --minutes 60 --count 5 --seed 42 --format json
# Returns scored recommendations with reasons

cargo run -p vapourfly-cli -- diagnostics export --out target/diagnostics.json
# Exports sanitized diagnostics
```

## GUI Smoke Test

See [gui-smoke-test.md](gui-smoke-test.md) for the full checklist.
Summary: GUI launches, all 8 views render, scan produces results,
no direct file writes to Steam files.

## Dependency and License Check

```
$ cargo deny check
advisories ok, bans ok, licenses ok, sources ok
```

No known security advisories. All dependencies use acceptable licenses
(MIT, Apache-2.0, BSD, ISC, Unicode-DFS-2016, Zlib). See `deny.toml`
for the full allow list.

## Security Scan

Diagnostics export checked for secret leakage:

```bash
cargo run -p vapourfly-cli -- diagnostics export --out target/diagnostics.json
if grep -nE 'Client Secret|access_token|refresh_token|Bearer |/Users/|C:\\Users\\|/home/' target/diagnostics.json; then
  echo "FAIL: secrets found in diagnostics"
  exit 1
fi
# PASS: no secrets found
```

## Gate Verification

All items in [IMPLEMENTATION_GATES.md](../IMPLEMENTATION_GATES.md) are
checked `[x]`. No unchecked items remain.

## Known Limitations

1. HLTB scraping behind `hltb_scrape` feature gate (not in default builds).
2. GUI Junk write actions (apply to collection, add to hidden) show dry-run diff before writing; backup restore also supported.
3. GUI Settings are editable and preserve existing IGDB/RAWG credentials on save.
4. GUI cache refresh is available from Data Sources; scan enrichment output remains CLI-only.
5. IGDB enrichment requires credentials; falls back to cache without them.
6. `cargo deny check` requires `cargo-deny` installed separately.
7. Source-only release: no pre-built binaries.
