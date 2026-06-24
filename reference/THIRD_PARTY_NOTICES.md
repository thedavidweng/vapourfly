# Third Party Notices and Clean-room Policy

**Version**: 2026-06-24

Vapourfly uses third-party projects as reference material for Steam file formats, GUI design and architecture, and VDF parsing behavior. Production Rust code must be implemented FULLY independently.

## Intended Vapourfly license

`Apache-2.0`

Changing this license requires a repository-level decision before code is copied from any copyleft source.

## Reference matrix

| Project | License noted in reference docs | Allowed use in Vapourfly |
|---|---|---|
| rallion/depressurizer | GPLv3 | Read behavior, file format clues, high-level ideas. Do not copy code, comments, class layout, or translated implementation. |
| mq1/TinyWiiBackupManager | GPL-3.0 | Read architecture ideas. Do not copy implementation. |
| BeyondDimension/Gameloop.Vdf | MIT | Compatible reference. Rust implementation still remains independent. |
| BeyondDimension/SteamClient / Watt Toolkit materials | Verify before reuse | Treat as read-only reference until license is confirmed for the exact file. |
| Steam local samples | User-provided sample data | Use for local tests. Remove personal paths and IDs before publishing fixtures. |

## Clean-room rules

1. Implement VDF, cloud storage, API, and Steam parsing from documented behavior, local samples, and new tests.
2. Avoid copying third-party source line structure, comments, constants blocks, or method bodies from GPL projects.
3. Keep implementation notes in Vapourfly-owned docs and tests.
4. Use fixtures with sanitized AppIDs and sample values when publishing.
5. Add attribution links in README and this file for reference-only projects.

## Release checklist

- [ ] Confirm all dependency licenses with `cargo deny` or equivalent.
- [ ] Confirm `Cargo.toml` license.
- [ ] Confirm packaged fixtures are sanitized.
- [ ] Confirm API keys and Steam account identifiers are excluded from logs and crash reports.
- [ ] Delete this file in github public releases.
