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
- Next automatic work: inspect target-specific feature trees, choose a separate
  experimental profile only when its measurement plan is ready. Keep #23 open
  until the required native/per-OS comparisons and any chosen optimization are
  genuinely verified. No performance improvement is claimed by this slice.
