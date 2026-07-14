# 07 — Discover restyle + post-generate continuation

**What to build:** Discover is a first-class page matching the mockup: seed AppID, count, generate, and result cards with scores/reasons. Generate uses the stable Discover playlist slot from ticket 02 and shows results on-page. Users get a clear continuation into Playlists (load/open the generated Playlist) and may be offered sync only behind the normal confirmation-gated write path.

**Blocked by:** 01 — Design system shell + navigation IA; 02 — Generator playlist slots

**Status:** done

- [x] Discover view provides seed, count, generate, and on-page results
- [x] Generate writes/overwrites the stable Discover playlist slot
- [x] User can continue to Playlists with the generated Playlist available to load/manage
- [x] Any Steam Collection write remains dry-run/confirm
- [x] Visual structure matches design system fidelity bar
- [x] FEATURES and gui-smoke-test Discover steps describe the top-level page (not Playlists-embedded Discover)
