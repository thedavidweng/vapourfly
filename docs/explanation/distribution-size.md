# Distribution Size Evidence

The release workflow reports the actual staged CLI, GUI, libraries, resources,
app-bundle totals and compressed assets. The AppImage packager inventories its
deployed AppDir before removal, so bundled shared libraries are included rather
than inferred from the raw GUI executable. JSON includes every logical file's
size/hash (symlinks are counted without following external targets); Markdown
shows the 20 largest staged contributors. Disk allocation is not measured.

Reports run after packaging/signing and are read-only. They do not modify or
re-sign artifacts, remove resources, or gate CI/release. A reporting error is
visible and is not evidence of a zero-size or successful report. CI's AppImage
artifact and release's separate `size-*` artifacts retain the reports.
The script requires Python 3.11 or newer; workflows explicitly install 3.12,
including the Ubuntu 22.04 AppImage runner whose system Python lacks `tomllib`.

To compare a retained report with a candidate, use the same script and provide
`--baseline previous.json`. For example, after building/packaging a real candidate:

```bash
python3 scripts/size-report.py --stage vapourfly-macos-aarch64 \
  --asset vapourfly-macos-aarch64.tar.gz --target vapourfly-macos-aarch64 \
  --revision "$(git rev-parse HEAD)" --toolchain 1.99 \
  --baseline previous.json --out size-reports/candidate
```

Without a real baseline the table says unavailable; it never fabricates a prior
measurement. Byte deltas require matching target, exact rustc details and
Cargo.lock hash. Both profiles/configurations are recorded; the profile can
deliberately differ for an experiment. A delta across source revisions is not
proof that a profile alone caused it. Cargo profile environment overrides are
not applied by this workflow; experiments using them must separately record
their effective settings. Named inherited profiles record their declared
configuration, including `inherits`; retain the parent configuration alongside
an experiment rather than interpreting omitted fields as effective settings.

## Decision Log For #23

- Adopted: non-gating, post-package logical-byte inventory and explicit
  baseline/candidate reporting. Tests use tiny files, not performance benchmarks.
- Unchanged: production release `opt-level=3`, full LTO, codegen-units=1, stripping;
  no UPX, dependency feature pruning, resource removal or panic-policy change.
- Insufficient evidence: `s`/`z`, thin LTO, panic=abort and trim-paths adoption.
  They require comparable builds and actual startup, scan, GUI/controller and
  platform evidence. Reports alone cannot satisfy those native acceptance gates.
- Next automatic work: choose a separate experimental profile only when its
  measurement plan is ready. The target-specific tree audit below does not
  establish a dependency that is safe to remove. Keep #23 open
  until the required native/per-OS comparisons and any chosen optimization are
  genuinely verified. No performance improvement is claimed by this slice.

## Target Feature Audit (2026-10-19)

Inspected the locked workspace at
`2b7a273e74a4659ce734ead2c66fc59cf1000bd9`, using rustc
`1.99.0 (b940084d7 2026-09-28)` on `aarch64-apple-darwin`.
Cargo.lock SHA256 before and after all commands was
`61b383eb892a1108b5841b9c9c1f91a952671e4e987cd147e3feeab69a6aead0`.
No build, lockfile edit or dependency removal was performed.

Reproduce each target's three views, retaining stdout and stderr separately:

```bash
set -euo pipefail
out=$(mktemp -d)
printf 'Audit output: %s\n' "$out"
for target in aarch64-apple-darwin x86_64-unknown-linux-gnu x86_64-pc-windows-msvc; do
  cargo tree --locked --workspace --target "$target" -e features \
    > "$out/$target-features.txt" 2> "$out/$target-features.stderr"
  cargo tree --locked --workspace --target "$target" -d \
    > "$out/$target-duplicates.txt" 2> "$out/$target-duplicates.stderr"
  cargo tree --locked --workspace --target "$target" -e normal,build \
    > "$out/$target-normal.txt" 2> "$out/$target-normal.stderr"
  for view in features duplicates normal; do
    test -s "$out/$target-$view.txt"
  done
done
```

All nine commands exited successfully. The captured output line counts are an
audit cross-check, **not** unique dependency counts, compiled bytes or benchmark
results. Cargo's feature graph includes feature names whose platform-specific
dependencies are absent from the selected normal/build graph.

| Target | Feature lines | Duplicate lines | Normal/build lines |
| --- | ---: | ---: | ---: |
| macOS arm64 | 6,745 | 898 | 1,273 |
| Linux x86_64 | 4,691 | 1,376 | 1,645 |
| Windows x86_64 MSVC | 3,850 | 834 | 1,146 |

Offline Linux/Windows inspection initially failed on uncached crates. A locked
target-specific fetch succeeded but did not populate every crate subsequently
requested by these workspace tree views. The successful audit used the locked
online commands above; a partial download or failed offline command was not
counted as an empty graph or a successful audit. This inspection does not require
cross-compilation toolchains, and does not prove a native OS build works.

### Findings And Decisions

- Keep GPUI platform backends. The normal/build graphs select
  `gpui-pre-macos` only for macOS, `gpui-pre-linux` plus `gpui-pre-wgpu` only
  for Linux, and `gpui-pre-windows` only for Windows. These are selected native
  implementations, not evidence that every platform is bundled in each binary.
- Keep controller and file-dialog support. Linux selects `libudev-sys`, `ashpd`,
  `x11rb` and `wayland-client`; the other two normal/build graphs do not.
  macOS selects `objc2-app-kit`; Linux/Windows do not. Removing these paths
  without native input/dialog acceptance would trade functionality for an
  unmeasured size claim.
- Keep the current HTTP features. `ureq` defaults select gzip and Rustls;
  these provide existing compressed-response/TLS behavior. A feature name in
  the tree alone does not establish that a protocol capability is unused.
- Do not force transitive version unification. Linux's duplicate view includes
  base64 0.22.1 through the GPUI SVG stack and 0.23.1 through the application,
  HTTP and localization paths. The view also repeats some identical versions
  in host/build versus target dependency domains. Neither case establishes
  removable shipped bytes, and changing a direct dependency cannot simply
  override its consumers' incompatible version requirements.
- No verified safe-pruning candidate was established. Production dependency
  features, release profile, panic policy, resources and UPX policy stay
  unchanged. A future candidate needs a compatible source-level change, actual
  per-OS tests and comparable packaged measurements before adoption.

References: [Cargo tree](https://doc.rust-lang.org/cargo/commands/cargo-tree.html),
[Cargo fetch](https://doc.rust-lang.org/cargo/commands/cargo-fetch.html), and
[feature resolution](https://doc.rust-lang.org/cargo/reference/features.html#feature-resolver-version-2).
