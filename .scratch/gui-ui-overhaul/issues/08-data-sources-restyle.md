# 08 — Data Sources restyle

**What to build:** Data Sources is restyled to the mockup: offline mode control and a table of sources (IGDB, RAWG, ProtonDB, PCGW, HLTB, Steam Store) with credential/entries/stale/last-success signals and refresh actions. Behavior remains aligned with lazy hydration and offline cache-only workflows (ADR-0002)—this ticket is presentation and shell consistency, not a hydration policy change.

**Blocked by:** 01 — Design system shell + navigation IA

**Status:** done

- [x] Offline toggle is available on Data Sources and blocks network refresh/hydration paths as today when enabled
- [x] Source table shows status columns and refresh actions for supported sources
- [x] Visual structure matches design system fidelity bar
- [x] FEATURES and gui-smoke-test Data Sources steps match the restyled surface
