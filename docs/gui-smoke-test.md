# GUI Smoke Test

Manual testing steps for the Vapourfly GUI. Automated unit tests cover app
creation, fixture scanning, navigation contract, and playtime formatting; this
document covers the interactive flows that require a running GUI.

## Prerequisites

- Build the GUI: `cargo build -p vapourfly-gui`
- Have fixture data available: `data/fixtures/steam_minimal/`
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

Shell uses a dark design-token theme and monochrome line icons (not emoji).

## Test Steps

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
12. Toggle Installed, Unplayed, Hidden, and Junk filters and verify the matching count and grid update.
13. Click a card's "Recommend" button and verify the **Recommendations** view opens with that AppID filled as the seed.
14. If cached external metadata exists, verify cards show hydrated values such as Proton tier, HLTB time, rating, or genre; otherwise cards should show a plain playable/library state without errors.

### 2. Junk Preview (from Library)

1. From Library, click **Junk…** in the toolbar (not the sidebar).
2. Choose Default, Strict, or Aggressive mode.
3. Click "Run Junk Detection".
4. Verify the evaluation table is displayed with confidence scores and signal text.
5. If the active fixture/cache has candidates, verify the candidate count is nonzero.
6. If using bare `steam_minimal`, verify the table reports zero candidates without errors.
7. Switch modes, run detection again, and confirm the table updates without errors.
8. Click **Back to Library** and confirm the Library grid returns.

### 3. Write Dry-Run Modal

1. From Library → Junk…, run junk detection with a fixture/cache that produces at least one candidate.
2. Click the "Apply to Collection" or "Add to Hidden" action button.
3. Verify a dry-run diff modal appears before any file is written.
4. Confirm the modal shows the target `cloud-storage-namespace-1.json` path.
5. Verify added/removed AppID counts match the junk candidates.
6. Confirm dismissing the modal does NOT write to disk.
7. Confirm accepting the modal writes the plan and refreshes the view.

### 4. Recommendation Result

1. Navigate to **Recommendations** via the left sidebar.
2. Set available time (e.g., 60 minutes).
3. Set Count and optionally enter a numeric Seed.
4. Click "Get Recommendations".
5. Verify recommendations are displayed sorted by score.
6. Confirm each recommendation shows name, AppID, score, and reason lines.
7. Clear Seed, rerun recommendations, and confirm the result set updates without errors.
8. Enter a non-numeric value in Available minutes and verify the UI shows a validation error instead of silently using a default.
9. Toggle Deck mode and Installed only, rerun recommendations, and confirm the result set updates without errors.
10. Click "Save to Steam Collection".
11. Verify the dry-run diff targets the `vapourfly-picks` collection and requires confirmation before writing.

### 5. Playlists

1. Navigate to Playlists view via the left sidebar.
2. Fill Create / Edit Playlist with an ID, name, description, and comma-separated AppIDs. Leave the Rules JSON field empty.
3. Click "Save Playlist" and verify a match report appears.
4. Click "Copy Share Code" and verify a `VF1:` code is shown.
5. Paste the code into Share code and click "Import Code"; verify the same playlist is loaded.
6. Enter an export path and click "Export Playlist"; verify a Vapourfly playlist JSON file is written.
7. Click "Sync to Steam Collection"; verify a dry-run diff modal appears before any file is written and targets the slugged playlist ID as the Steam collection.
8. Cancel the sync modal and verify the Steam cloud storage file is unchanged.
9. Confirm the sync modal and verify a backup is created before the Steam collection is written.
10. Confirm Playlists has **no** Discover seed/Generate Discover control (Discover is a top-level view).
11. Compile `deck-session` and `finish-it` templates and verify each produces a playlist or rule summary without errors, and that each is saved under the stable slot ids `dynamic-deck-session` / `dynamic-finish-it` (a second compile overwrites the same slot).
12. Pick an Editorial Mood from the dropdown (e.g. "Quick Round") and click "Compile Editorial Mood"; verify a playlist is written under `mood-quick-round` without errors (regenerate overwrites that slot).
13. Clear the edit fields, enter a new ID and name, and paste a JSON rules array (e.g. `[{"op":"Installed"},{"op":"NotHidden"}]`) into the Rules JSON field. Click "Save Playlist" and verify a rule-based playlist is saved and the match report appears.
14. Enter invalid JSON in the Rules JSON field and click "Save Playlist"; verify an error is shown and no playlist is saved.
15. Verify the match report shows "Completion price:" with a formatted price when Steam Store cache data exists, or a hint to run `cache refresh --source steam-store` when no price data is cached.

### 6. Discover

1. Navigate to **Discover** via the left sidebar (not via Playlists).
2. Set Discover seed AppID and Count, click "Generate Discover", and verify a Discover playlist is created with the requested count when enough candidates exist and is stored under the stable id `discover`.
3. Clear Discover seed AppID, rerun generation, and verify the taste-based Discover playlist still works and **overwrites** the same `discover` slot (no second playlist id).
4. Optionally click "Open in Playlists" and verify the generated playlist is available for edit/sync. To keep a long-term copy, change id/name and Save again.

### 7. Collections

1. Navigate to Collections view via the left sidebar.
2. Verify collection names, game counts, and hidden status appear after scan.
3. Confirm `Favorites` reports `2` games against `steam_minimal`.
4. Enter an export path and click "Export Collections"; verify a JSON file is written.
5. Open the exported JSON and confirm it contains the `favorite` collection with AppIDs `730` and `427520`.

### 8. Data Sources

1. Navigate to Data Sources via the left sidebar.
2. Enable "Offline mode (cache only)".
3. Verify cache refresh buttons are disabled and the UI states that cache refresh is disabled while offline mode is on.
4. Disable offline mode.
5. Click a source refresh button and verify the refresh begins only after a library scan is available.

### 9. Settings Diagnostics

1. Navigate to Settings view via the left sidebar.
2. Click "Refresh Accounts".
3. Verify detected accounts appear with persona, account name, masked Steam ID, and most-recent status.
4. Click "Use" on an account and verify Account Override is populated with that account name.
5. Click "Run Setup Check".
6. Verify a diagnostics report appears with redacted Steam path, account count, cloud storage status, cache root, and IGDB/RAWG credential state.
7. Enter a diagnostics export path and click "Export Diagnostics".
8. Verify a sanitized JSON file is written with version, platform, arch, source credential state, and timestamp.

### 10. Backup List (under Settings)

1. Navigate to Settings via the left sidebar (Backups is **not** a top-level item).
2. Find the **Backups** section.
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
- Dark shell + monochrome line icons remain consistent across destinations.
