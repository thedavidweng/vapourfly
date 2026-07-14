# 03 — Library restyle (filters, cards, Junk panel, hover Recommend)

**What to build:** Library matches the mockup structure: search, only three filters (Installed only, Not hidden, Not junk), poster card grid with playtime and available badges, and a library summary (e.g. game counts). A Junk… toolbar control opens a polished panel for mode, preview, apply-to-collection, and hide with dry-run confirmation. Hovering or selecting a card reveals a Recommend shortcut that opens Recommendations with that AppID as seed (approved deviation from the static mockup). No per-card Junk; no multi-select batch bar; no game detail page.

**Blocked by:** 01 — Design system shell + navigation IA

**Status:** done

- [x] Library filter bar offers only Installed only, Not hidden, and Not junk
- [x] Poster grid and summary region follow design layout structure (token/component fidelity, not pixel-perfect)
- [x] Junk… opens a panel with Default/Strict/Aggressive, preview, apply-to-collection, and hide
- [x] Junk write actions still use dry-run/confirm and existing disposition/write safety
- [x] Recommend control appears on hover or selection and seeds Recommendations
- [x] Pure filter/projection helpers are tested where extracted; smoke steps cover Junk panel and hover Recommend
- [x] FEATURES and gui-smoke-test Library/Junk sections match the new entry points and filters
