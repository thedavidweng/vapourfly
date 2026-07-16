# GUI Smoke Test

Manual testing steps for the Vapourfly GUI. Automated unit tests cover app
creation, fixture scanning, navigation contract, and playtime formatting; this
document covers the interactive flows that require a running GUI.

## Prerequisites

- Build the GUI: `cargo build -p vapourfly-gui`
- Have fixture data available: `data/fixtures/steam_minimal/`
- For visual review without a real Steam account, launch in demo mode:
  `cargo run -p vapourfly-gui -- --ui-demo`
  Demo mode populates every page with deterministic fixture data (24 games,
  5 Playlists, 4 Steam Collections, junk decisions, recommendations, discover
  results, accounts, and backups). Write actions, cache refresh, account
  detection, settings save, and backup operations are all disabled in demo
  mode. All I/O is isolated inside a unique per-launch temp directory so no
  real user config, cache, playlist store, or Steam data is read or written.
- Junk write-action checks require at least one low-playtime game with a second
  junk signal from cached metadata (low rating or short completion time).
  `steam_minimal` by itself has only playtime data; without a seeded cache or a
  real library/cache that produces a candidate, Junk Preview should show zero
  candidates without errors and the write buttons should remain unavailable.

## Navigation map (ADR-0006)

Sidebar lists **only**:

1. Library (default landing view)
2. Collections
3. Recommendations
4. Playlists
5. Discover
6. Data Sources
7. Settings

**Not** in the sidebar:

- Junk — open from Library toolbar (`Junk…`)
- Backups — open from Settings → Backups section

Shell defaults to the light desktop token theme (warm canvas, white cards, orchid accent) and can switch to the dark design-system palette from the top chrome (macOS-style top bar + monochrome line icons in both modes).

## Test Steps

### 0. Theme toggle

1. Launch the GUI (fixtures or real library).
2. Confirm the top chrome shows a **☾ Dark** control on the right.
3. Click it and verify the shell switches to the dark palette (cool surfaces, violet accent) without changing navigation destinations.
4. Click **☀ Light** and verify the warm light palette returns.

### 1. Fixture Scan + Library

1. Launch GUI with fixtures: `cargo run -p vapourfly-gui -- --fixtures data/fixtures/steam_minimal`
2. Verify the **Library** view loads automatically on startup (default landing).
3. Verify the sidebar lists only the seven destinations above (no Junk or Backups).
4. Verify the sidebar game count appears after the scan completes.
5. Verify the Library uses poster cards rather than a table by default.
6. Verify real Steam poster art loads for real AppIDs such as Counter-Strike 2 (`730`) and Factorio (`427520`).
7. Confirm each card shows AppID, title, installed/library status, playtime, and cached metadata when available.
8. Confirm CS2 shows Installed and playtime `6h 58m`.
9. Confirm Factorio shows Installed and playtime `17h 18m`.
10. Confirm app 999 remains in the grid with a library/non-installed status and playtime `5m`; if Steam has no poster for it, the card should keep a stable poster-sized image area without layout shift.
11. Type a title or AppID in Search and verify the card grid filters immediately.
12. Confirm the filter bar offers **only** three toggles: **Installed only**, **Not hidden**, and **Not junk** (no Unplayed / include-only Hidden or Junk toggles).
13. Toggle each filter and verify the Matching / Installed summary pills and grid update.
14. Hover a game card (or click to select it) and verify a **Recommend** control appears; without hover/selection it should not clutter the card.
15. Click **Recommend** and verify the **Recommendations** view opens with that AppID filled as the seed.
16. If cached external metadata exists, verify cards show Proton tier and Deck badges when available, plus playtime; otherwise cards should show a plain playable/library state without errors.

### 2. Junk Preview (from Library)

1. From Library, click **Junk…** in the toolbar (not the sidebar).
2. Choose Default, Strict, or Aggressive mode.
3. Click **Preview**.
4. Verify the preview table is displayed with confidence scores and signal text.
5. If the active fixture/cache has candidates, verify the Candidates metric is nonzero and **Apply to collection** / **Hide** actions appear.
6. If using bare `steam_minimal`, verify the table reports zero candidates without errors and write actions stay unavailable.
7. Switch modes, preview again, and confirm the table updates without errors.
8. Click **Back to Library** and confirm the Library grid returns.

### 3. Write Dry-Run Modal

1. From Library → Junk…, run Preview with a fixture/cache that produces at least one candidate.
2. Click **Apply to collection** or **Hide**.
3. Verify a dry-run diff modal appears before any file is written.
4. Confirm the modal shows the target `cloud-storage-namespace-1.json` path.
5. Verify added/removed AppID counts match the junk candidates.
6. Confirm dismissing the modal does NOT write to disk.
7. Confirm accepting the modal writes the plan and refreshes the view.

### 4. Recommendation Result

1. Navigate to **Recommendations** via the left sidebar (or arrive via Library card Recommend with seed filled).
2. Set available minutes (e.g., 60).
3. Set Count and optionally enter a numeric Seed AppID.
4. Toggle **Installed only** and **Deck mode** as needed.
5. Click **Preview**.
6. Verify recommendations are displayed sorted by score.
7. Confirm each recommendation shows name, AppID, score badge, and human-readable reason codes (code pill + description).
8. Clear Seed, preview again, and confirm the result set updates without errors.
9. Enter a non-numeric value in Available minutes and verify the UI shows a validation error instead of silently using a default.
10. Toggle Deck mode and Installed only, preview again, and confirm the result set updates without errors.
11. Click **Write to vapourfly-picks**.
12. Verify the dry-run diff targets the `vapourfly-picks` collection and requires confirmation before writing.

### 5. Playlists

1. Navigate to Playlists via the left sidebar.
2. Confirm the page header has **Dynamic** and **Mood** actions only among generators — **no** Discover seed/Generate control.
3. Under **Load existing**, refresh/list shows store ids after saves (may be empty on first run).
4. Fill **Create / Edit** with an ID, name, description, and comma-separated AppIDs. Leave Rules JSON empty.
5. Click **Save Playlist** and verify a match report appears (owned/missing/… pills + owned preview when games are known).
6. Click **Copy Share Code** and verify a `VF1:` code is shown.
7. Paste the code into **Share code** and click **Import Code**; verify the same playlist is loaded into edit fields.
8. Click **Export…**, choose a path in the file dialog, and verify a Vapourfly playlist JSON file is written.
9. Click **Sync to Steam Collection**; verify a dry-run diff modal appears before any write and targets the slugged playlist ID as the Steam collection.
10. Cancel the sync modal and verify the Steam cloud storage file is unchanged.
11. Confirm the sync modal and verify a backup is created before the Steam collection is written.
12. Click **Dynamic** → chooser opens for `deck-session` / `finish-it` with session minutes and count → **Generate**. Verify a playlist is written under `dynamic-deck-session` or `dynamic-finish-it` and loads into the edit surface (regenerate overwrites the same slot).
13. Click **Mood** → chooser lists the seven Editorial Moods → pick e.g. Quick Round → **Generate**. Verify a playlist is written under `mood-quick-round` (regenerate overwrites that slot).
14. Use **Load existing** to select a stored id and **Load**; verify edit fields and match report update.
15. Clear the edit fields, enter a new ID and name, paste a JSON rules array (e.g. `[{"op":"Installed"},{"op":"NotHidden"}]`) into Rules JSON, **Save Playlist**, and verify a rule-based playlist is saved.
16. Enter invalid JSON in Rules JSON and Save; verify an error is shown and no playlist is saved.
17. Verify match report completion price when Steam Store cache exists, or the cache-refresh hint when not.

### 6. Discover

1. Navigate to **Discover** via the left sidebar (not via Playlists).
2. Confirm seed AppID, count, and **Generate** controls on this page.
3. Set seed AppID and Count, click **Generate**, and verify on-page **result cards** show names, scores, and reason codes when candidates exist; the playlist is stored under stable id `discover`.
4. Clear seed AppID, regenerate, and verify the taste-based playlist **overwrites** the same `discover` slot (no second playlist id).
5. Click **Open in Playlists** and verify the generated playlist is loaded for edit/share/sync. To keep a long-term copy, change id/name and Save again.
6. Optionally click **Sync to Steam Collection** on Discover and verify dry-run confirmation before any Steam write.

### 7. Collections

1. Navigate to Collections view via the left sidebar.
2. Verify collections appear as a **card grid** (not a dense table): each card shows name, game count, and a poster collage when member AppIDs resolve art.
3. Confirm there is **no** member drill-in / edit UI on cards.
4. Confirm `Favorites` reports `2` games against `steam_minimal` (collage may show CS2 and Factorio posters).
5. Click **Export all**, choose a save location in the file dialog, and verify a JSON file is written.
6. Open the exported JSON and confirm it contains the `favorite` collection with AppIDs `730` and `427520`.

### 8. Data Sources

1. Navigate to Data Sources via the left sidebar.
2. Verify the page shows an **Offline mode** control and a unified **Sources** table with columns for Source, Credential, Entries, Stale, Last success, and Action (Refresh).
3. Confirm the table lists IGDB, RAWG, ProtonDB, PCGW, HLTB, and Steam Store (display names).
4. Enable "Offline mode (cache only)".
5. Verify **Refresh All** and per-row Refresh actions are disabled and the UI states that cache refresh is disabled while offline mode is on.
6. Disable offline mode.
7. Click a source Refresh button and verify the refresh begins only after a library scan is available (or the UI prompts to scan first).

### 9. Settings Diagnostics

1. Navigate to Settings via the left sidebar.
2. Verify Configuration fields: Steam directory, account override, store country/language, backup retention, and Save Settings.
3. Click "Refresh Accounts".
4. Verify detected accounts appear with persona, account name, masked Steam ID, and most-recent status.
5. Click "Use" on an account and verify Account override is populated with that account name.
6. Click "Run Setup Check".
7. Verify a diagnostics report appears with redacted Steam path, account count, cloud storage status, cache root, and IGDB/RAWG credential state.
8. Enter a diagnostics export path and click "Export Diagnostics".
9. Verify a sanitized JSON file is written with version, platform, arch, source credential state, and timestamp.
10. Confirm offline mode is **not** required on Settings (it lives on Data Sources).

### 10. Backup List (under Settings)

1. Navigate to Settings via the left sidebar (Backups is **not** a top-level item).
2. Find the **Backups** section (maintenance home for list/restore).
3. Verify the backup list is displayed (empty for a fresh install).
4. If no backups exist, create one by confirming a Junk write against fixture data.
5. Click "Refresh Backups".
6. Verify the new backup entry appears with filename, created timestamp, SHA256 prefix, and Restore action.
7. Verify restoring a backup asks for confirmation before replacing the current Steam config file.

## Expected Behavior

- All views load without errors.
- No direct file writes to Steam files (all through core WritePlan).
- Steam running warning appears if Steam is detected as active.
- Credential status is displayed without revealing secrets.
- All write operations require explicit user confirmation.
- The left sidebar highlights the currently selected view.
- The Refresh button re-scans and updates the Library poster grid.
- Light shell, macOS-style top chrome, and monochrome line icons remain consistent across destinations.
