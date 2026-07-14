# 06 — Playlists restyle + Dynamic/Mood choosers

**What to build:** Playlists is restyled to the mockup information hierarchy while keeping create/edit, load existing, save, match report, preview, sync to Steam Collection, export, import file, and VF1 share code import/copy. Generator actions on this page are Dynamic and Mood only (no Discover). Each opens a lightweight chooser (templates + parameters; seven Editorial Moods) then generates through the stable playlist slot contract from ticket 02.

**Blocked by:** 01 — Design system shell + navigation IA; 02 — Generator playlist slots

**Status:** done

- [x] Playlists supports create/edit/load/save/match/sync/export/import/share without Discover controls
- [x] Dynamic button opens a chooser for deck-session and finish-it (with required parameters) then generates into its stable store slot
- [x] Mood button opens a chooser for the seven canonical Editorial Moods then generates into its stable store slot
- [x] Sync still uses dry-run confirmation; share codes remain VF1
- [x] Visual structure matches design system fidelity bar
- [x] FEATURES and gui-smoke-test Playlists/generator steps match the new chooser + no-Discover IA
