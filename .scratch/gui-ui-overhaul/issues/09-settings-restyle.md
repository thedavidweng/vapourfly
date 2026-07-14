# 09 — Settings restyle + backups home

**What to build:** Settings is restyled and is the home for maintenance: Steam directory, account override, store locale, backup retention, write-while-Steam policy, detected accounts, setup diagnostics, backup list/restore, and diagnostics export. There is no top-level Backups destination. Restore remains confirmation-gated and write-safe.

**Blocked by:** 01 — Design system shell + navigation IA

**Status:** done

- [x] Settings exposes configuration fields and setup diagnostics consistent with the feature contract
- [x] Backup list and restore live under Settings (no Backups sidebar item)
- [x] Restore requires confirmation and respects write safety
- [x] Diagnostics export remains available
- [x] Visual structure matches design system fidelity bar
- [x] FEATURES and gui-smoke-test Settings/backup steps match the restyled surface
