# GUI Smoke Test

Manual testing steps for the Vapourfly GUI. Automated unit tests cover app
creation, fixture scanning, view switching, and playtime formatting; this
document covers the interactive flows that require a running GUI.

## Prerequisites

- Build the GUI: `cargo build -p vapourfly-gui`
- Have fixture data available: `data/fixtures/steam_minimal/`
- Junk write-action checks require at least one low-playtime game with a second
  junk signal from cached metadata (low rating or short completion time).
  `steam_minimal` by itself has only playtime data; without a seeded cache or a
  real library/cache that produces a candidate, Junk Preview should show zero
  candidates without errors and the write buttons should remain unavailable.

## Test Steps

### 1. Fixture Scan

1. Launch GUI with fixtures: `cargo run -p vapourfly-gui -- --fixtures data/fixtures/steam_minimal`
2. Verify the Library view loads automatically on startup.
3. Verify the sidebar game count appears after the scan completes.
4. Verify the table displays at least two games (Counter-Strike 2, Factorio) with columns: App ID, Name, Installed, Playtime, Status, Metadata.
5. Confirm CS2 shows a checkmark for Installed and playtime `6h 58m`.
6. Confirm Factorio shows a checkmark for Installed and playtime `17h 18m`.
7. Confirm app 999 (non-installed) shows a dash for Installed and playtime `5m`.
8. If cached external metadata exists, verify the Metadata column shows hydrated values such as Proton tier, HLTB time, rating, or genre; otherwise the column should show `—` without errors.

### 2. Junk Preview

1. Navigate to Junk view via the left sidebar.
2. Choose Default, Strict, or Aggressive mode.
3. Click "Run Junk Detection".
4. Verify the evaluation table is displayed with confidence scores and signal text.
5. If the active fixture/cache has candidates, verify the candidate count is nonzero.
6. If using bare `steam_minimal`, verify the table reports zero candidates without errors.
7. Switch modes, run detection again, and confirm the table updates without errors.

### 3. Write Dry-Run Modal

1. From the Junk view, run junk detection with a fixture/cache that produces at least one candidate.
2. Click the "Apply to Collection" or "Add to Hidden" action button.
3. Verify a dry-run diff modal appears before any file is written.
4. Confirm the modal shows the target `cloud-storage-namespace-1.json` path.
5. Verify added/removed AppID counts match the junk candidates.
6. Confirm dismissing the modal does NOT write to disk.
7. Confirm accepting the modal writes the plan and refreshes the view.

### 4. Recommendation Result

1. Navigate to Recommend view via the left sidebar.
2. Set available time (e.g., 60 minutes).
3. Click "Get Recommendations".
4. Verify recommendations are displayed sorted by score.
5. Confirm each recommendation shows name, AppID, score, and reason lines.
6. Toggle Deck mode and Installed only, rerun recommendations, and confirm the result set updates without errors.
7. Click "Save to Steam Collection".
8. Verify the dry-run diff targets the `vapourfly-picks` collection and requires confirmation before writing.

### 5. Playlists

1. Navigate to Playlists view via the left sidebar.
2. Fill Create / Edit Playlist with an ID, name, description, and comma-separated AppIDs.
3. Click "Save Playlist" and verify a match report appears.
4. Click "Copy Share Code" and verify a `VF1:` code is shown.
5. Paste the code into Share code and click "Import Code"; verify the same playlist is loaded.
6. Enter an export path and click "Export Playlist"; verify a Vapourfly playlist JSON file is written.
7. Click "Sync to Steam Collection"; verify a dry-run diff modal appears before any file is written and targets the slugged playlist ID as the Steam collection.
8. Cancel the sync modal and verify the Steam cloud storage file is unchanged.
9. Confirm the sync modal and verify a backup is created before the Steam collection is written.
10. Click "Generate Discover" and verify a Discover playlist is created.
11. Compile `deck-session`, `finish-it`, `mood`, and `playlist-radio` templates and verify each produces a playlist or rule summary without errors.

### 6. Collections

1. Navigate to Collections view via the left sidebar.
2. Verify collection names, game counts, and hidden status appear after scan.
3. Confirm `Favorites` reports `2` games against `steam_minimal`.
4. Enter an export path and click "Export Collections"; verify a JSON file is written.
5. Open the exported JSON and confirm it contains the `favorite` collection with AppIDs `730` and `223850`.

### 7. Settings Diagnostics

1. Navigate to Settings view via the left sidebar.
2. Click "Run Setup Check".
3. Verify a diagnostics report appears with redacted Steam path, account count, cloud storage status, cache root, and IGDB/RAWG credential state.
4. Enter a diagnostics export path and click "Export Diagnostics".
5. Verify a sanitized JSON file is written with version, platform, arch, source credential state, and timestamp.

### 8. Backup List

1. Navigate to Backups view via the left sidebar.
2. Verify the backup list is displayed (empty for a fresh install).
3. If no backups exist, create one by confirming a Junk write against fixture data.
4. Click "Refresh Backups".
5. Verify the new backup entry appears with filename, created timestamp, SHA256 prefix, and Restore action.
6. Verify restoring a backup asks for confirmation before replacing the current Steam config file.

## Expected Behavior

- All views load without errors.
- No direct file writes to Steam files (all through core WritePlan).
- Steam running warning appears if Steam is detected as active.
- Credential status is displayed without revealing secrets.
- All write operations require explicit user confirmation.
- The left sidebar highlights the currently selected view.
- The Refresh button re-scans and updates the Library table.
