# 02 — Generator playlist slots (stable id + store write)

**What to build:** When a user successfully runs Discover, a Dynamic Template, or an Editorial Mood from the GUI, the result is written to the local playlist store under a stable playlist id for that generator identity and overwrites on regenerate. All three generators share this contract. Scoring and eligibility remain in core; this ticket owns slot identity policy and store write timing. Automated tests prove put + overwrite without re-testing recommendation/discover algorithms.

**Blocked by:** 01 — Design system shell + navigation IA

**Status:** done

- [x] Discover generate writes a Playlist to the playlist store
- [x] Dynamic Template compile/generate writes a Playlist to the playlist store
- [x] Editorial Mood compile/generate writes a Playlist to the playlist store
- [x] Each generator identity uses a stable playlist id; a second generate overwrites the same id’s content
- [x] Automated tests cover store write + overwrite for the generator orchestration seam
- [x] FEATURES/smoke updated if user-visible generator save behavior changed from “fill form only” to auto-store
- [x] ADR-0007 behavior is reflected in GUI workflows (CLI slot mirroring not required)
