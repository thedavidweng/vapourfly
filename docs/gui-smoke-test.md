# GUI Smoke Test

Manual testing steps for the Vapourfly GUI.

## Prerequisites

- Build the GUI: `cargo build -p vapourfly-gui`
- Have fixture data available: `data/fixtures/steam_minimal/`

## Test Steps

### 1. Fixture Scan

1. Launch GUI with fixtures: `cargo run -p vapourfly-gui -- --fixtures data/fixtures/steam_minimal`
2. Verify the Library view shows games from the fixture
3. Verify game details (AppID, name, installed status, playtime) are displayed

### 2. Junk Preview

1. Navigate to Junk view
2. Verify junk candidates are displayed with reasons
3. Verify confidence scores are shown
4. Test mode switching (Default/Strict/Aggressive)

### 3. Write Dry-Run Modal

1. From Junk view, select "Apply" action
2. Verify dry-run diff is shown before any write
3. Verify target file path is displayed
4. Verify added/removed AppID counts are correct

### 4. Recommendation Result

1. Navigate to Recommend view
2. Set available time (e.g., 60 minutes)
3. Click "Get Recommendations"
4. Verify recommendations are displayed with scores and reasons

### 5. Backup List

1. Navigate to Backups view
2. Verify backup list is displayed (empty for fresh install)
3. Verify backup metadata (path, created_at, SHA256 prefix)

## Expected Behavior

- All views load without errors
- No direct file writes to Steam files (all through core WritePlan)
- Steam running warning appears if Steam is detected
- Credential status is displayed without revealing secrets
- All write operations require explicit confirmation
