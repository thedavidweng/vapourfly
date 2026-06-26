# GUI Smoke Test

Manual testing steps for the Vapourfly GUI. Automated unit tests cover app
creation, fixture scanning, view switching, and playtime formatting; this
document covers the interactive flows that require a running GUI.

## Prerequisites

- Build the GUI: `cargo build -p vapourfly-gui`
- Have fixture data available: `data/fixtures/steam_minimal/`

## Test Steps

### 1. Fixture Scan

1. Launch GUI with fixtures: `cargo run -p vapourfly-gui -- --fixtures data/fixtures/steam_minimal`
2. Verify the Library view loads automatically on startup.
3. Confirm the heading shows the fixture account name (`vapourfly_fixture_user`) and path.
4. Verify the table displays at least two games (Counter-Strike 2, Factorio) with columns: App ID, Name, Installed, Playtime.
5. Confirm CS2 shows a checkmark for Installed and playtime `6h 58m`.
6. Confirm Factorio shows a checkmark for Installed and playtime `17h 18m`.
7. Confirm app 999 (non-installed) shows a dash for Installed and playtime `5m`.

### 2. Junk Preview

1. Navigate to Junk view via the left sidebar.
2. Verify junk candidates are displayed with reasons (e.g., low playtime, zero playtime).
3. Verify confidence scores are shown for each candidate.
4. Test mode switching between Default, Strict, and Aggressive thresholds.
5. Confirm changing mode re-filters the list without errors.

### 3. Write Dry-Run Modal

1. From the Junk view, run junk detection until candidates are listed.
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
5. Confirm each recommendation shows a score, reason line, and game metadata (name, playtime).
6. Verify games already played recently are ranked lower or excluded.

### 5. Backup List

1. Navigate to Backups view via the left sidebar.
2. Verify the backup list is displayed (empty for a fresh install).
3. Trigger a backup from the Settings or action area.
4. Verify the new backup entry appears with metadata: file path, created_at timestamp, SHA256 prefix.
5. Verify restoring a backup replaces the current Steam config file.

## Expected Behavior

- All views load without errors.
- No direct file writes to Steam files (all through core WritePlan).
- Steam running warning appears if Steam is detected as active.
- Credential status is displayed without revealing secrets.
- All write operations require explicit user confirmation.
- The left sidebar highlights the currently selected view.
- The Refresh button re-scans and updates the Library table.
