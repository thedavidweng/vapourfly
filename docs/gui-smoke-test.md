# GUI Smoke Test

Manual testing steps for the Vapourfly GUI. Automated unit tests cover app
creation, fixture scanning, navigation contract, and playtime formatting; this
document covers the interactive flows that require a running GUI.

## Prerequisites

- Build the GUI: `cargo build -p vapourfly-gui` (GPUI + gpui-component desktop shell)
- Have fixture data available: `data/fixtures/steam_minimal/`
- For visual review without a real Steam account, launch in demo mode:
  `cargo run -p vapourfly-gui -- --ui-demo`
  Demo mode populates every page with deterministic fixture data (24 games,
  5 Playlists, 4 Steam Collections, junk decisions, recommendations, discover
  results, source statuses, accounts, and backups). Write actions, cache
  refresh, account detection, settings save, and backup operations are all
  disabled in demo mode. All I/O is isolated inside a unique per-launch temp
  directory so no real user config, cache, playlist store, or Steam data is
  read or written.
- Optional launch flags for review and screenshots:
  - `--theme light|dark` overrides the saved theme for this launch.
  - `--view <page>` opens a page directly: `library`, `discover`,
    `recommendations`, `playlists`, `collections`, `data-sources`, `settings`.
  - `--deck` opens a 1280×800 window with the Steam Deck Game Mode profile
    (see [Controller and Steam Deck](#controller-and-steam-deck)).
- Junk write-action checks require at least one low-playtime game with a second
  junk signal from cached metadata (low rating or short completion time).
  `steam_minimal` by itself has only playtime data; without a seeded cache or a
  real library/cache that produces a candidate, the Junk preview should show
  zero candidates without errors and the write buttons should remain unavailable.

## Navigation map (ADR-0006)

The sidebar opens with the signed-in Steam profile (avatar, persona name and
login name) and then lists **only** these destinations, in three groups
separated by divider lines:

- **Play:** Library (default landing view), Discover, Recommendations
- **Organize:** Playlists, Collections
- **System:** Data Sources, Settings

**Not** in the sidebar:

- Junk cleanup: open from the Library header (**Clean up**)
- Backups: Settings → Backups section

Library, Playlists, and Collections show their item counts in the sidebar.
The title bar holds the sidebar toggle, a sync-status pill, and the theme
toggle. The Library grid uses a virtualized gpui `list` and the row list a
virtualized `uniform_list`.

## Test Steps

### 0. Shell and theme

1. Launch the GUI with no saved theme and confirm it opens in dark mode.
2. Verify the dark theme follows SteamOS (a darker slate sidebar beside a lighter content panel, a full-width band on the current destination) with Steam-blue selection and no violet/indigo accent, and that text and controls read comfortably at 100% scaling (16px base text, 48px sidebar rows).
3. Verify the top of the sidebar shows your Steam avatar and persona name (from Steam's local avatar cache; initials when there is no cached picture). In demo mode it shows **Demo Player** with "DP" initials.
4. Click the sun/moon button in the title bar and verify the palette switches without changing layout or navigation.
5. Relaunch and confirm the chosen theme persists (not in demo mode).
6. Click the sidebar toggle (left of the title bar) and verify the sidebar collapses to icons; click again to expand. Below 1180px wide the sidebar collapses automatically.
7. Verify the status pill shows a spinner while scanning, then **Up to date** (or **Demo library** / **Offline**).

### 1. Fixture Scan + Library

1. Launch GUI with fixtures: `cargo run -p vapourfly-gui -- --fixtures data/fixtures/steam_minimal`
2. Verify the **Library** view loads automatically on startup and shows skeleton cards while the snapshot is prepared.
3. Verify the header subtitle shows game count, installed count, and total playtime.
4. Verify the toolbar is one row: search, the All / Installed / Unplayed / Hidden segmented control, **Filters**, the sort menu, and the grid/list switch.
   - With no search or filter, verify the grid opens on a **Continue playing** hero for the most recently played game (hero art and logo when Steam has them, otherwise a generated backdrop and the title), with **Play** (or **Install** when not installed), **Details**, and **More like this**. Below it, verify a **Recently played** shelf and an **All games** heading. Play hands off to Steam (`steam://rungameid/<id>`); in demo mode it shows a notice instead. Typing in Search or choosing another scope hides the hero and shelf.
5. Verify real Steam art loads for AppIDs Steam has cached locally (for example Counter-Strike 2 `730`, Factorio `427520`), and that missing art is downloaded in the background when online. In demo/offline mode, verify every game gets a generated duotone cover with no network request.
6. Confirm each card shows a 2:3 portrait capsule (Steam `library_600x900` art, with a Deck pill when controller support is full), the title, and a status dot with playtime. Hovering a card draws a focus ring.
7. Confirm CS2 shows Installed and `6h 58m played`; Factorio shows Installed and `17h 18m played`.
8. Confirm app 999 remains in the grid as Not installed with `5m played`; missing artwork keeps a stable image area without layout shift.
9. Type a title or AppID in Search and verify the grid filters immediately.
10. Select each scope and verify Installed, Unplayed, and Hidden return only their named subsets.
11. Open **Filters** and verify presets (Cozy, Story-rich, Great on Deck, Short sessions), Steam Deck / controller switches, a minimum ProtonDB tier control, Hide junk / Exclude hidden switches, genre/tag inputs, playtime and length ranges, and **Reset all**. Active filters appear as removable chips under the toolbar, and the Filters button shows a count.
12. Open the sort menu and verify each sort option and the ascending/descending choice reorder the grid.
13. Switch to list view and verify columns Game, Status, Played, Last played, and Time to beat align with their headers.
14. Click a card and verify a side sheet opens with hero or header art, tags, **Play**, **Find similar**, **Store page**, a copy-App-ID button, and a details list (playtime, last played, time to beat, ProtonDB, controller, rating, App ID, Steam collections).
15. Right-click a card and verify the context menu offers View details, Find similar games, Copy App ID, and Open store page. Find similar opens Discover with that game as the seed.

### 2. Junk cleanup (from Library)

1. From Library, click **Clean up** in the header (not the sidebar).
2. Verify the page reads **Junk cleanup** with a **← Library** link, a Default / Strict / Aggressive control, and **Run preview**.
3. Click **Run preview** and verify the Evaluated / Flagged as junk / Selected tiles and a table with checkbox, cover, game, verdict tag, and confidence bar.
4. Toggle **Show games marked keep** and verify Keep rows appear.
5. Use **Select all flagged** and **Clear** in the bottom bar; the selected count updates.
6. If the active fixture/cache has candidates, verify **Add to Junk collection** and **Hide in Steam** are enabled once rows are selected.
7. If using bare `steam_minimal`, verify the table reports zero candidates without errors and write actions stay unavailable.
8. Switch modes, preview again, and confirm the table updates without errors.
9. Click **← Library** and confirm the Library grid returns.

### 3. Write confirmation dialog

1. From Junk cleanup, run a preview that produces at least one candidate and select it.
2. Click **Add to Junk collection** or **Hide in Steam**.
3. Verify a modal dialog opens before any file is written, first showing "Preparing a preview of the change…".
4. Confirm the dialog shows **Added** / **Removed** tiles and the target `cloud-storage-namespace-1.json` path, plus the backup note.
5. Verify the added/removed counts match the selected candidates.
6. Click **Cancel** (or press Escape) and confirm nothing is written.
7. Repeat and click **Write to Steam**; verify the plan is written, a success toast appears, and the view refreshes.

### 4. Recommendations

1. Navigate to **Recommendations** via the sidebar.
2. Choose a session length (30m – 4h+) and a pick count (5 / 10 / 20).
3. Toggle **Installed only** and **Deck mode** as needed.
4. Generate and verify three featured cards (rank, relative match, reason, time to beat, playtime, rating) followed by a ranked list with Proton tier, time to beat, and match.
5. Change options, generate again, and confirm the results update without errors.
6. Click a result and verify the game sheet opens.
7. Click **Save as collection** and verify the confirmation dialog targets the `vapourfly-picks` collection before writing.

### 5. Playlists

1. Navigate to Playlists via the sidebar.
2. Confirm the master-detail layout: the left rail lists stored playlists (cover thumbnail, name, type and game count); the right pane shows the editor.
3. Open the **+** menu in the rail and verify: Blank playlist, From a template, From a mood (the seven Editorial Moods), Import from file…, and Paste share code.
4. Create a blank playlist and verify the ID auto-generates from the Name (slugified). Manually editing the ID disables auto-generation.
5. Enter an ID with path separators (e.g. `../test`), click **Save**, and verify an error toast appears and nothing is saved.
6. In the **Games** tab, search **Add from library** and add games; verify they appear in the playlist with per-row remove.
7. Turn on **Edit App IDs directly**, enter `730, invalid, 440`, and Save; verify the error names the invalid token and no playlist is saved.
8. Click **Save** and verify a success toast and the rail entry update.
9. Open the **Match** tab and verify owned/missing tiles, the **Owned / Missing** control, and the completion price for missing priced games (or no price when none is available).
10. Open the **Share** tab: **Copy share code** puts a `VF1:` code on the clipboard; **Import from clipboard** loads it back; **Export…** writes a Vapourfly playlist JSON file; the JSON panel shows the current file.
11. Click **Sync to Steam** and verify the confirmation dialog targets the slugged playlist ID as the Steam collection. Cancel and confirm the cloud storage file is unchanged; confirm and verify a backup is created first.
12. From the **+** menu, choose a template, Deck Session or Finish It (writes `dynamic-deck-session` or `dynamic-finish-it`) and a mood (e.g. Quick Round writes `mood-quick-round`); regenerating overwrites the same slot.
13. Click a stored playlist in the rail and verify the editor and match report update.
14. In the **Rules** tab, verify:
    - Active rules shown as removable chips (removing a Not's child removes the Not node)
    - Quick add: Installed, Not hidden, Not junk, Full controller
    - **Build a rule** rows: Has genre, Has tag, Max time to beat, Minimum ProtonDB tier, Playtime between, Minimum rating
    - Each row's **Add** stays disabled until its value is valid (for example playtime min ≤ max, rating within 0–5)
    - **Show rules as JSON** for raw editing
15. Add rules, turn on JSON, and verify the JSON reflects them.
16. Enter invalid JSON, then click a Quick add button; verify an error is shown and the invalid JSON is **not** wiped.
17. Save a rule-based playlist and verify it appears in the rail as a Rules playlist.

### 6. Discover

1. Navigate to **Discover** via the sidebar.
2. Confirm the seed panel: a searchable game picker, a pick-count control (10 / 20 / 40), and a generate button.
3. Pick a seed, click **Find matches**, and verify a grid of pick cards with relative match and reason; the playlist is stored under stable id `discover`.
4. Clear the seed (the picker's clear button), click **Match my taste**, and verify the taste-based playlist **overwrites** the same `discover` slot.
5. Click **Open as playlist** and verify the generated playlist loads in Playlists for edit/share/sync.
6. Optionally click **Sync to Steam** and verify the confirmation dialog appears before any write.

### 7. Collections

1. Navigate to Collections via the sidebar.
2. Verify a card grid: each card shows a cover collage of up to four member games, the name, game count, and a **Hidden** tag for Steam's hidden collection.
3. Click a collage tile and verify that game's sheet opens.
4. Confirm `Favorites` reports `2 games` against `steam_minimal`.
5. Click **Export all**, choose a location in the save dialog, and verify a JSON file is written containing the `favorite` collection with AppIDs `730` and `427520`.

### 8. Data Sources

1. Navigate to Data Sources via the sidebar.
2. Verify the header has an **Offline mode** switch and **Refresh all**, followed by Sources / Cached entries / Stale / Missing credentials tiles.
3. Verify the table lists Steam Store, IGDB, ProtonDB, PCGW, HLTB, and RAWG with credential tag, entries, freshness, last updated, and a per-row Refresh.
4. Turn on **Offline mode** and verify **Refresh all** and every row's Refresh are disabled.
5. Turn it off and click a source's Refresh; verify the refresh runs (or reports that a scan is needed first).

### 9. Settings

1. Navigate to Settings via the sidebar.
2. Verify the groups: Appearance (Light / Dark), Steam (Steam folder with folder picker, Account, accounts found on this computer, Store region, Steam Web API key), Write safety (Write while Steam is running, Backups to keep), Backups, Diagnostics, and About.
3. Verify the fields show only what is written in `config.toml`. A field missing from the file is empty, and its placeholder shows the value that applies instead (the detected Steam folder, `US`, `english`, `5`). **Save changes** is disabled until a field changes.
4. Click **Use** on a detected account and verify the Account field is filled with its SteamID64 and the row shows **In use**.
5. Change a field and click **Save changes**. Verify the save message, and verify `config.toml` gains only that key. Clear a field and save to verify its key is removed. Then verify **Save changes** is disabled again.
6. Click **Run check** and verify a report appears with redacted Steam path, account count, cloud storage status, cache root, and IGDB/RAWG credential state.
7. Click **Export**, choose a path, and verify a sanitized JSON file is written with version, platform, arch, source credential state, and timestamp.

### 10. Backups (under Settings)

1. In Settings → Backups, verify each backup shows its date, short SHA-256, file name, and **Restore**. A fresh install shows an empty message.
2. If no backups exist, create one by confirming a Junk write against fixture data and return to Settings.
3. Click **Restore** and verify the confirmation dialog appears before the Steam file is replaced; the current file is backed up first.

## Expected Behavior

- All views load without errors.
- No direct file writes to Steam files (all through core WritePlan).
- A warning appears if Steam is running and writes are not allowed.
- Credential status is displayed without revealing secrets.
- All write operations require explicit confirmation in a dialog.
- Errors and successes appear as toast notifications.
- The sidebar highlights the current destination.
- Rescan (refresh icon in the Library header) re-scans and updates the grid.

## Controller and Steam Deck

Run with any Xbox-style gamepad connected, on a Steam Deck, or with
`cargo run -p vapourfly-gui -- --ui-demo --deck` on another machine.

1. Connect the controller and press the D-pad. Focus lands on the current sidebar entry, and Settings → Controller names the controller. Settings → Controller → Session reads *Steam Deck · Game Mode* under `--deck` or Game Mode, and *Desktop* elsewhere.
2. Up / Down move through the sidebar without switching page; A or Right opens the focused page and focuses its first control.
3. LB / RB cycle the pages (wrapping) and focus the first control of the new page. Start opens Settings; Select hides and shows the sidebar.
4. In the Library grid, the D-pad moves one card left / right and one row up / down; holding a direction repeats. Moving past the visible rows scrolls the grid so the focused capsule stays on screen. Up from the first grid row goes to the Recently played shelf. Repeat in the list layout with Up / Down.
5. A on a card opens its detail sheet; B closes it and focus returns to the page. B on a page control returns focus to the sidebar.
6. The right stick scrolls the page under focus smoothly; LT / RT scroll about a screen.
7. Open Filters, a sort menu and the confirmation dialog: the D-pad moves inside them, A selects and B closes them.
8. Y opens Library and focuses search. On SteamOS, Y, X, and A on a text field also bring up Steam's on-screen keyboard (not in `--ui-demo`). Up / Down leave a text field; Left / Right move the caret.
9. While the controller is in use, a footer shows the button hints. Moving the mouse hides it.
10. In Game Mode (or `--deck`), start a write: the confirmation dialog warns that Steam is running in Game Mode.
11. On a Deck in Game Mode, Vapourfly opens fullscreen at 1280×800 and the Library hero is shorter, so the Recently played shelf starts on the first screen. In Desktop Mode at 125% scaling the window fits the screen.
