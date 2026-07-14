# Spec: GUI visual system + information architecture overhaul

Status: done  
Feature: gui-ui-overhaul  
Source: grill-with-docs session (design mockups ×8) + seams agreement  
Implementation: commits through Data Sources/Settings restyle (ADR-0006/0007); tickets 01–09 marked done  

## Problem Statement

Vapourfly’s desktop GUI already exposes the product’s library, playlist, recommendation, and Steam Collection workflows, but its shell no longer matches the intended product surface. The current app uses a light visual system, emoji sidebar icons, and a navigation map that buries Discover inside Playlists, elevates Junk and Backups as top-level destinations, and presents a denser Library filter set than the redesigned experience. The user has a complete dark design system and screen mockups for Library, Collections, Recommendations, Playlists, Discover, Data Sources, and Settings. They need that design—visual system and information architecture together—implemented in the existing egui GUI without regressing domain capabilities (Playlist vs Steam Collection, generators, write safety, hydration).

## Solution

Overhaul the Vapourfly GUI so the design mockups are the source of truth for chrome, tokens, components, and top-level navigation, while core domain behavior continues to live behind existing workflows.

Users get:

- A dark design-token shell with monochrome line icons and shared components (buttons, cards, inputs, status pills, empty states).
- Top-level navigation: **Library**, **Collections**, **Recommendations**, **Playlists**, **Discover**, **Data Sources**, **Settings**.
- **Discover** as its own destination that generates a Playlist into the local playlist store.
- **Junk** as a full-capability Library tool (panel), not a sidebar item.
- **Backups** (and restore) inside Settings, not a sidebar item.
- Playlists focused on create/edit, match, sync, share, import, plus Dynamic Template and Editorial Mood generators (not Discover).
- Delivery that lands the shell first, then restyles views in sidebar order, keeping features available under the new shell even before each page is fully restyled.
- FEATURES and GUI smoke documentation updated in the same change as any user-visible contract shift.

## User Stories

1. As a Steam library owner, I want a clear left sidebar of primary destinations, so that I always know where I am in the app.
2. As a Steam library owner, I want the sidebar to list Library, Collections, Recommendations, Playlists, Discover, Data Sources, and Settings only, so that navigation matches the product map I was shown.
3. As a Steam library owner, I want monochrome line icons next to each nav item, so that the shell feels like a serious desktop tool rather than an emoji prototype.
4. As a Steam library owner, I want a dark background and surface hierarchy aligned to the design tokens, so that long sessions are easy on the eyes and match the mockups.
5. As a Steam library owner, I want consistent primary, secondary, and ghost buttons, so that destructive and constructive actions read the same on every page.
6. As a Steam library owner, I want shared card, input, and status-pill components, so that every view feels like one product.
7. As a Steam library owner, I want empty states with a clear title and subtitle when a view has nothing to show, so that I know what to do next (for example scan first).
8. As a Steam library owner, I want the app to open on Library after launch, so that my games are the default landing surface.
9. As a Steam library owner, I want to search my library by title or AppID, so that I can find a game quickly.
10. As a Steam library owner, I want filters for Installed only, Not hidden, and Not junk, so that the grid stays focused on playable, visible games.
11. As a Steam library owner, I do not want extra Library filter toggles (for example Unplayed) in the overhauled UI, so that the filter bar matches the design.
12. As a Steam library owner, I want each game shown as a poster card with title, playtime, and key badges (such as Proton tier and Deck when available), so that I can scan the library visually.
13. As a Steam library owner, I want a footer or summary of how many games match and how many are installed, so that I understand the size of my filtered library.
14. As a Steam library owner, I want a Recommend action to appear when I hover or select a game card, so that I can seed Recommendations without cluttering the default card chrome.
15. As a Steam library owner, I want that Recommend action to open Recommendations with the game’s AppID as seed, so that I can get session picks related to a game I care about.
16. As a Steam library owner, I want a Junk… control on the Library toolbar, so that I can clean the library without a separate sidebar section.
17. As a Steam library owner, I want the Junk panel to offer Default, Strict, and Aggressive modes, so that I can choose how aggressive classification is.
18. As a Steam library owner, I want to preview Junk candidates with explainable signals and confidence, so that I understand why a game was flagged.
19. As a Steam library owner, I want to apply Junk candidates to a Steam Collection after dry-run confirmation, so that I can group junk without guessing what will be written.
20. As a Steam library owner, I want to hide Junk candidates via Steam’s hidden collection after dry-run confirmation, so that they leave my normal library view safely.
21. As a Steam library owner, I want Junk write actions to refuse or warn according to existing write-safety rules (including Steam-running policy and backups), so that cloud storage is not corrupted.
22. As a Steam library owner, I want Collections to appear as cards with name, count, and a poster collage when art is available, so that I can recognize collections at a glance.
23. As a Steam library owner, I want an Export all action on Collections, so that I can dump my Steam collections for backup or inspection.
24. As a Steam library owner, I want Collections to be read-only overview in v1 (no drill-in editor), so that editing membership stays on the Playlist sync path and in Steam itself.
25. As a Steam library owner, I want a Recommendations view with available minutes, count, installed-only, Deck mode, and optional seed AppID, so that I can shape a play session.
26. As a Steam library owner, I want to preview recommendations with scores and reason codes, so that I understand why each Game was suggested.
27. As a Steam library owner, I want to write recommendations to the vapourfly-picks Steam Collection after dry-run confirmation, so that the picks show up in Steam.
28. As a Steam library owner, I want validation errors when recommendation inputs are invalid (for example non-numeric minutes), so that the app fails loudly instead of silently defaulting.
29. As a playlist curator, I want to create and edit a Playlist with id, name, description, and Manual or Rules content, so that I can maintain Vapourfly-owned artifacts.
30. As a playlist curator, I want to save a Playlist to the local playlist store, so that it persists across sessions.
31. As a playlist curator, I want to load an existing Playlist, so that I can continue editing or matching.
32. As a playlist curator, I want a match report (owned, missing, played, unplayed, hidden, junk, completion price when prices are cached), so that I know how a Playlist intersects my library.
33. As a playlist curator, I want to preview matched games, so that I can verify content before syncing.
34. As a playlist curator, I want to sync a Playlist to a Steam Collection after dry-run confirmation, so that Steam reflects my Vapourfly artifact.
35. As a playlist curator, I want to export a Playlist to JSON and import from file, so that I can back up and move lists.
36. As a playlist curator, I want to copy and import VF1 share codes, so that I can share playlists compactly.
37. As a playlist curator, I want Dynamic and Mood actions on Playlists (not Discover), so that transparent templates and editorial moods stay next to playlist management.
38. As a playlist curator, I want Dynamic to open a lightweight chooser for deck-session and finish-it (with needed parameters such as session length), so that I am not stuck with a single hard-coded template.
39. As a playlist curator, I want Mood to open a lightweight chooser for the seven canonical Editorial Moods, so that I can compile a curated list without seeing hidden criteria as editable rules.
40. As a playlist curator, I do not want a Discover button on Playlists, so that similar-picks generation has a single home.
41. As a discovery user, I want a top-level Discover view with seed AppID and count, so that I can generate similar picks without entering Playlists first.
42. As a discovery user, I want Generate Discover playlist to create a Manual Playlist in the playlist store, so that the result is a real artifact I can load later.
43. As a discovery user, I want Discover results shown on the Discover page (names, scores, reasons), so that I can inspect the generation without leaving the page.
44. As a discovery user, I want a clear way to continue to Playlists (and optionally start sync with confirmation), so that generation is not a dead end.
45. As a discovery user, I want regenerating Discover to overwrite a stable Discover playlist id, so that my store does not fill with one-off discover playlists.
46. As a discovery user, I want Dynamic and Mood generations to also write stable ids and overwrite on regenerate, so that all generators share one mental model.
47. As a discovery user, I want generator playlists to use vapourfly authorship where the domain already does, so that store entries remain distinguishable from hand-built lists.
48. As a power user, I want to keep a generator result long-term by changing id/name and saving again, so that overwrite slots do not trap me (explicit Duplicate can wait).
49. As a data steward, I want a Data Sources table of IGDB, RAWG, ProtonDB, PCGW, HLTB, and Steam Store with credential, entry, stale, and last-success signals, so that I know what metadata I have.
50. As a data steward, I want per-source and/or all refresh actions, so that I can repopulate cache when needed.
51. As a data steward, I want an offline mode control on Data Sources, so that workflows stop hitting the network and use cache-only hydration behavior.
52. As a Steam library owner, I want Settings for Steam directory, account override, store locale, backup retention, and write-while-Steam policy, so that the app matches my install.
53. As a Steam library owner, I want detected accounts listed with a one-click override, so that multi-account machines are usable.
54. As a Steam library owner, I want setup diagnostics in Settings, so that I can see path, account, cloud storage, and credential health without the CLI.
55. As a Steam library owner, I want backup list and restore inside Settings, so that recovery lives next to other maintenance tools.
56. As a Steam library owner, I want restore to require confirmation and to respect write safety, so that I do not overwrite cloud storage by accident.
57. As a Steam library owner, I want to export diagnostics from Settings, so that I can share sanitized support data.
58. As a returning user of the old GUI, I want no silent loss of Junk apply/hide, recommend-to-collection, playlist sync, share codes, or source refresh, so that the overhaul is a shell and IA change, not a feature cut.
59. As a user mid-overhaul, I want every top-level destination reachable under the new shell even if a page is not fully restyled yet, so that I am never stuck without a capability that still exists in code.
60. As a user mid-overhaul, I accept temporarily old page layouts inside the new shell, so that shipping can proceed page by page.
61. As a contributor, I want FEATURES and the GUI smoke test updated in the same change as navigation or workflow entry changes, so that the documented contract stays true.
62. As a contributor, I want automated tests to lock the navigation map and generator store-write contract, so that regressions in IA and generator slots fail in CI.
63. As a contributor, I want visual fidelity judged by tokens, component kinds, and layout structure—not pixel diffs—so that egui constraints do not block completion.
64. As a contributor, I want approved deviations documented (hover Recommend on Library cards), so that implementers do not “fix” them away against the mockups.
65. As a Steam library owner, I want write operations to still create timestamped backups and show dry-run diffs, so that the cloud-storage-only write surface remains safe (ADR-0001).
66. As a Steam library owner, I want workflows to keep lazy hydration with graceful degradation unless offline is on (ADR-0002), so that recommendations and junk still work with partial metadata.
67. As a playlist sharer, I want share codes to remain VF1 compact binary (ADR-0003), so that sharing stays short and current.
68. As a mood browser, I want Editorial Moods to remain opaque curated generators (ADR-0004), so that I am not editing hidden criteria as rules.
69. As a discovery user, I want Discover to own all similar-picks generation (ADR-0005), so that Playlist Radio does not reappear as a second concept.
70. As a keyboard/mouse desktop user, I want hover-reveal card actions to work with pointer hover or selection, so that the approved Recommend shortcut remains usable on desktop.
71. As a Steam library owner, I want error and success banners that match the design system, so that feedback is readable on the dark shell.
72. As a Steam library owner, I want Refresh/scan of the library to remain available from the shell or Library, so that Steam file changes show up without restarting the app.

## Implementation Decisions

### Authority and scope

- Design mockups are the source of truth for **visual system** and **information architecture**.
- Domain glossary (`CONTEXT.md`) and ADRs 0001–0005 remain authoritative for Playlist, Steam Collection, Dynamic Template, Editorial Mood, Discover, Junk, Recommendation, Hydration, Share Code, and write surface.
- Stay on **egui** / existing GUI crate; no framework migration.
- No CLI behavior change is required for this spec except where shared helpers already live in core and GUI simply calls them.

### Navigation map

Top-level views (labels as shown to users):

1. Library  
2. Collections  
3. Recommendations (rename from previous “Recommend” if needed for display)  
4. Playlists  
5. Discover  
6. Data Sources  
7. Settings  

Removed from top-level navigation:

- Junk (capability moves into Library)  
- Backups (capability moves into Settings)  

Default view on launch: Library.

### Visual system

- Adopt the mockup dark palette and type/spacing/radius scales as design tokens (background, surface tiers, borders, text tiers, brand/accent, semantic success/warning/error, spacing steps, radii).
- Fidelity bar: match **tokens, component kinds, information hierarchy, and layout regions**. Do not require pixel-perfect match to Figma/export, system font metrics, native scrollbars, or exact shadow/blur.
- Sidebar uses **monochrome line icons**, not emoji. Icon rendering technology is an implementation choice (geometry, icon font, or textures) as long as the result reads as stroke icons.
- Build or restyle shared helpers for: view header, section card, primary/secondary/ghost buttons, text inputs, filter toggles, metric/status pills, empty state, error/success banners, game poster card chrome.
- Prefer extracting a small shared UI layer inside the GUI crate over a large opportunistic rewrite of unrelated architecture. Full view-module split of the entire GUI entrypoint is **not** required by this spec (prior readiness notes deferred it); extract only what the shell and tokens need.

### Delivery order

1. Design tokens + shared components + new nav shell (including view enum / routing re-map).  
2. Restyle pages in order: Library (+ Junk panel) → Collections → Recommendations → Playlists → Discover → Data Sources → Settings (including backups).  

Until a page is restyled, it may keep older inner layout **inside the new shell**, but must remain reachable and functionally complete for its responsibilities (including relocated Junk and Backups).

### Library

- Search field; filters limited to **Installed only**, **Not hidden**, **Not junk**.
- Poster card grid with playtime and available Proton/Deck (or equivalent) badges when hydrated data exists.
- **Approved deviation:** per-card **Recommend** shortcut revealed on **hover or selection**, navigating to Recommendations with seed AppID filled.
- No per-card Junk actions.
- No multi-select batch bar; no full game detail page in this overhaul.
- **Junk…** toolbar control opens a panel/drawer/section: mode → preview → Apply to collection / Hide, using existing disposition + dry-run/confirm write path. Full parity with the former Junk view’s GUI capabilities.

### Collections

- Read-only card grid: name, game count, poster collage when images resolve.
- Page-level **Export all** (prefer a sensible save location UX over a bare unexplained path field when practical).
- No drill-in member editor; no per-card export requirement.

### Recommendations

- Controls: available minutes, count, installed only, Deck mode, seed AppID.
- Preview list with score and reason codes.
- Write to `vapourfly-picks` after dry-run confirmation (existing safety model).

### Playlists

- Create/edit fields, load existing, save, match report, preview, sync, export, import file, import share code.
- Action bar includes **Dynamic** and **Mood** only among generators—**no Discover** control.
- **Dynamic:** button opens lightweight chooser for `deck-session` / `finish-it` and required parameters, then generates.
- **Mood:** button opens lightweight chooser for the seven canonical Editorial Moods, then generates.
- Generator success writes through the playlist store (see Generator slots).

### Discover

- Standalone view: seed AppID, count, generate, result cards.
- Generate writes a Playlist into the playlist store and shows results on-page.
- Provide at least one clear continuation (open/load in Playlists; sync remains confirmation-gated if offered).
- No second Discover entry on Playlists.

### Generator playlist slots

- Discover, each Dynamic Template, and each Editorial Mood write to **stable playlist ids** and **overwrite on regenerate**.
- Exact slug strings are implementer-chosen but must be stable, readable, and one slot per generator identity (e.g. one Discover slot, one per template, one per mood id).
- Long-term keep: user changes id/name and saves (explicit Duplicate is optional later).
- Domain engines stay in core; GUI owns slot id policy and store put timing.

### Data Sources

- Offline toggle and source table with refresh actions as in the mockups and existing feature matrix.
- Offline continues to mean cache-only hydration for workflows (ADR-0002).

### Settings

- Existing settings fields plus setup diagnostics, detected accounts, **backup list/restore**, diagnostics export.
- Backup restore keeps confirmation and write safety.

### Documentation contract

Any change that alters user entry points or observable GUI workflow steps updates in the **same** change set:

- Feature reference GUI rows/behavior  
- GUI smoke test steps  

### Testing seams (agreed)

1. **GUI navigation contract** — which top-level destinations exist; Junk/Backups absent; Discover present; default Library.  
2. **Generator → playlist store orchestration** — generate writes store; stable id overwrite; core algorithms not re-tested here.  
3. **Library presentation pure helpers** (where extracted) — three filters; junk preview projection over existing evaluations.  
4. **Manual smoke** for visual shell, hover Recommend, Junk panel, Dynamic/Mood choosers, Collections export, Settings backups.  

Do not add screenshot CI or a new E2E framework for this work.

## Testing Decisions

### What good tests look like

- Assert **external behavior** users or other modules depend on: navigation membership, store contents after generate, filter outputs, write plan still going through dry-run paths.
- Do **not** assert egui layout coordinates, colors as RGB literals in every test, hover hit-testing, or private widget trees.
- Prefer the **highest existing seam**: GUI view enum / app orchestration methods; core `playlist_store`, discover/dynamic/mood, disposition for writes.
- One primary automated focus: **nav map + generator slot writes**. Everything visual leans on smoke.

### What will be tested (automated)

- Updated navigation set and labels (including absence of top-level Junk and Backups, presence of Discover).
- Generator orchestration: after a generate path runs against a temp playlist store (or fixture app data dir), the expected stable id exists and a second generate replaces content.
- Library filter helper(s) if factored pure: combinations of installed / hidden / junk flags.
- Existing core tests remain green; do not weaken write-safety or eligibility tests.

### Prior art

- GUI unit tests already cover app creation, fixture scan, view enum completeness, playlist build/export, collections export, playtime formatting.
- Core tests cover playlist store put/get, discover/mood/dynamic, disposition write ops, workflow prepare.
- Manual GUI smoke document covers interactive flows; rewrite steps to the new IA as part of implementation tickets.

### Manual / non-automated acceptance

- Shell tokens and line icons vs mockups (human visual check).
- Library Junk panel open/close and write modal.
- Hover/selection reveal of Recommend on cards.
- Dynamic/Mood chooser flows.
- Discover page results after generate.
- Collections cards + Export all.
- Settings backup restore confirmation.

## Out of Scope

- Replacing egui or rewriting the app as a web UI.
- Changing Steam write surface away from cloud storage only (ADR-0001).
- Changing hydration policy (ADR-0002), share code format (ADR-0003), Editorial Mood model (ADR-0004), or Discover’s ownership of similar picks (ADR-0005) beyond GUI placement.
- CLI command redesign (except incidental shared-core fixes if a bug is found).
- Per-game Steam settings editing (tags, launch options, controller configs)—still out of product scope.
- Unowned games as first-class library entities.
- User-tunable recommendation weights.
- Explicit “Duplicate playlist” feature (users can re-save under a new id).
- Collections member drill-in/edit UI.
- Multi-select batch operations on Library cards.
- Full game detail page.
- Pixel-perfect visual regression CI / screenshot tests.
- Mandatory full split of the GUI entrypoint into many modules (optional if it reduces risk for a ticket; not a goal of the overhaul).
- Localization / i18n of UI strings (mockups are English; display language work is separate).
- Reintroducing Playlist Radio or top-level Junk/Backups navigation.
- New data sources or new Editorial Mood definitions.

## Further Notes

### Design references

Eight mockups used in the grill session: Library, Collections, Recommendations, Playlists, Discover, Data Sources, Settings, and the design-system/token sheet. Implementers should treat those images (or copies checked into the repo if added later) as visual reference alongside this spec.

### Approved deviations from mockups

- Library game cards: Recommend shortcut on hover/selection (not drawn on the static mockup).
- Temporary “new shell + older page body” mid-delivery is allowed until each page ticket finishes.

### ADRs recorded

1. [ADR-0006](../../docs/adr/0006-gui-ia-and-design-system.md) — GUI IA + design system source of truth.  
2. [ADR-0007](../../docs/adr/0007-generator-playlist-slots.md) — Generator playlist slots (stable id + overwrite).

### Relationship to prior product readiness

Domain seams (`playlist_store`, `disposition`, eligibility, display helpers) should be reused. This overhaul is primarily presentation and navigation; resist re-implementing write assembly or store I/O in the GUI.

### Next step

Split this spec into tracer-bullet tickets under `.scratch/gui-ui-overhaul/issues/` via `/to-tickets`, with the design-system + shell ticket as the root blocker for page restyle tickets.
