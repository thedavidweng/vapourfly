# 05 — Recommendations restyle

**What to build:** Recommendations matches the mockup structure: available minutes, count, installed only, Deck mode, optional seed AppID, preview with scores and reason codes, and write to the vapourfly-picks Steam Collection after dry-run confirmation. Seed filled from Library’s hover Recommend (ticket 03) continues to work. Invalid inputs still surface validation errors.

**Blocked by:** 01 — Design system shell + navigation IA

**Status:** done

- [x] Recommendations view exposes minutes, count, installed only, Deck mode, and seed controls
- [x] Preview shows scores and human-readable reason codes
- [x] Write to vapourfly-picks still requires dry-run confirmation and write safety
- [x] Library-seeded AppID still populates the seed field when navigated from a card action
- [x] Visual structure matches design system fidelity bar
- [x] FEATURES and gui-smoke-test Recommendations steps match the restyled surface
