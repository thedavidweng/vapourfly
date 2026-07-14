# 01 — Design system shell + navigation IA

**What to build:** The app launches into a dark design-token shell with monochrome line sidebar icons and the mockup navigation map. Users can open exactly these top-level destinations: Library, Collections, Recommendations, Playlists, Discover, Data Sources, Settings. Junk is no longer a sidebar item but remains fully reachable from Library (panel may be minimal as long as mode, preview, apply-to-collection, and hide work with existing write safety). Backups/restore live under Settings, not a top-level Backups view. Discover is its own view (existing Discover UI may be moved as-is). Playlists no longer offers a Discover entry. Until later tickets restyle pages, older page bodies may sit inside the new shell, but every capability remains reachable. Navigation contract tests and FEATURES/smoke nav steps update in this ticket.

**Blocked by:** None — can start immediately

**Status:** done

- [x] Sidebar lists only Library, Collections, Recommendations, Playlists, Discover, Data Sources, Settings (no top-level Junk or Backups)
- [x] Default landing view is Library
- [x] Shell uses dark design tokens and monochrome line icons (not emoji nav icons)
- [x] Shared chrome primitives exist enough for the shell (e.g. headers, nav active state, basic buttons/cards) consistent with the design system direction
- [x] Full Junk GUI capability is reachable from Library without a Junk sidebar item
- [x] Backup list and restore are reachable from Settings without a Backups sidebar item
- [x] Discover is a top-level view; Playlists has no Discover control
- [x] Automated tests lock the new navigation set (presence/absence of destinations)
- [x] `docs/FEATURES.md` and `docs/gui-smoke-test.md` navigation/entry steps match the new IA
- [x] Existing domain write-safety and workflow behavior is not regressed for relocated entry points
