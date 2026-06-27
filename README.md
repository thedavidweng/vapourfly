# Vapourfly

A local-first CLI and desktop GUI for managing Steam game libraries like Spotify playlists.

Vapourfly helps you organize, categorize, and curate your Steam library. Define collections with expressive queries, detect junk, get recommendations, and keep your library tidy -- all without touching Steam's UI.

## Status

v0.1.0 is a source-only release. Expect breaking changes until v1.0.

## Supported Platforms

- macOS
- Linux
- Windows

## Installation

### From Source

```bash
git clone https://github.com/vapourfly/vapourfly.git
cd vapourfly
cargo install --path crates/cli
```

Build the GUI from the same checkout:

```bash
cargo run -p vapourfly-gui --release
```

Pre-built binaries are not shipped with v0.1.0.

## First Scan

Verify your setup and scan your library:

```bash
# Diagnose Steam installation, accounts, and credentials
vapourfly doctor

# Scan your library and print a table
vapourfly scan --format table

# JSON output for scripting
vapourfly scan --format json
```

`vapourfly doctor` reports the detected Steam directory, accounts, library folders, cloud storage availability, cache location, and API credential status. If Steam is not auto-detected, pass `--steam-dir` or set `VAPOURFLY_STEAM_DIR`.

## Safety Model

Vapourfly modifies your Steam configuration files. To protect your data:

- **All write operations require `--dry-run` or `--confirm`.** Omitting both is an error.
- **`--dry-run` shows a diff without writing.** Use it to preview exactly what would change.
- **`--confirm` executes the write with an automatic backup.** A timestamped backup is created before any file is modified.
- **Backups before writes.** Every write creates a backup named `{file}.vapourfly-backup-{timestamp}-{sha}.json` in the same directory.
- **No Steam process interference.** Vapourfly refuses to write if Steam is detected as actively running. Close Steam first, or use `--allow-steam-running` only when you understand the risk.
- **Atomic writes.** Changes are written to a temporary file, fsynced, and renamed over the target. If anything fails after the backup is created, an automatic restore is attempted.

See [docs/STEAM_FILE_SAFETY.md](docs/STEAM_FILE_SAFETY.md) for the full write target and backup strategy.

## Backup and Restore

Every write operation creates a timestamped backup. Manage them with:

```bash
# List available backups
vapourfly backup list

# List as JSON
vapourfly backup list --format json

# Restore a specific backup
vapourfly backup restore /path/to/cloud-storage-namespace-1.json.vapourfly-backup-20260624T120000Z-a1b2c3d4.json
```

Backups are stored alongside the original file and include a SHA-256 hash for integrity verification. The most recent 5 backups are retained by default.

## API Credential Setup

Some features (genre data, ratings, completion times) require external API credentials. Set these environment variables in your shell profile or `.env` file:

| Variable | Source | Required For |
|---|---|---|
| `VAPOURFLY_IGDB_CLIENT_ID` | [IGDB / Twitch Developer Console](https://dev.twitch.tv/console) | Genre, rating, and time-to-beat data from IGDB |
| `VAPOURFLY_IGDB_CLIENT_SECRET` | Same as above | IGDB OAuth authentication |
| `VAPOURFLY_RAWG_KEY` | [RAWG API](https://rawg.io/apidocs) | Genre, tag, and rating data from RAWG |

ProtonDB, PCGamingWiki, HLTB, and Steam Store data do not require credentials.

Check your credential status at any time:

```bash
vapourfly doctor
vapourfly sources status
```

See [docs/API_SOURCES.md](docs/API_SOURCES.md) for details on each data source.

## Offline Mode

Pass `--offline` to prohibit all network calls. Vapourfly will use only locally cached data:

```bash
vapourfly scan --offline --format table
vapourfly junk preview --offline
vapourfly recommend --offline --minutes 120
```

When offline, commands that depend on uncached external data degrade gracefully: missing fields are omitted from output, junk detection uses only available signals, and recommendations fall back to local metadata. Cache refresh is blocked in offline mode.

## Usage

```bash
# Diagnose Steam installation and credentials
vapourfly doctor

# Scan your library
vapourfly scan --format table

# List collections
vapourfly collections list

# Export collections to JSON
vapourfly collections export --out collections.json

# Preview junk candidates
vapourfly junk preview

# Get recommendations for 2 hours of play
vapourfly recommend --minutes 120 --count 5

# Import a playlist
vapourfly playlist import my-playlist.json

# Sync a playlist to a Steam collection (dry-run first)
vapourfly sync collection my-playlist-id --dry-run
vapourfly sync collection my-playlist-id --confirm
```

## Junk Detection

Identify games you are unlikely to play using three evaluation modes:

| Mode | Logic |
|---|---|
| **Default** | Low playtime + at least one other negative signal (short completion or low rating), with a minimum of 2 data points available |
| **Strict** | Every available signal must indicate junk, with a minimum of 2 data points |
| **Aggressive** | Low playtime + at least one other negative signal, no minimum data requirement |

Every decision is explainable: the output includes which signals matched, which were missing, and a confidence score reflecting data completeness.

```bash
# Preview junk candidates (default mode)
vapourfly junk preview

# Strict mode -- only flag games where all signals agree
vapourfly junk preview --strict

# Aggressive mode -- flag with fewer signals
vapourfly junk preview --aggressive

# Apply junk classification to a Steam collection (dry-run first)
vapourfly junk apply --collection "junk" --dry-run
vapourfly junk apply --collection "junk" --confirm

# Move junk games to the hidden collection
vapourfly junk hide --dry-run
vapourfly junk hide --confirm
```

## Recommendations

Get game recommendations based on available play time:

```bash
# Recommend 5 games you can play in 2 hours
vapourfly recommend --minutes 120

# Only installed games, optimized for Steam Deck
vapourfly recommend --minutes 60 --installed-only --deck

# Reproducible results with a seed
vapourfly recommend --minutes 120 --seed 42
```

## Playlists

Playlists are JSON files that describe a named subset of your library. They can be manual (explicit AppID lists) or rule-based (boolean expressions evaluated against game metadata).

### Import and Export

```bash
# Import a playlist from a JSON file
vapourfly playlist import my-playlist.json

# Export a stored playlist by ID
vapourfly playlist export my-playlist-id --out exported.json

# Export Steam collections to Vapourfly JSON
vapourfly collections export --out collections.json
```

### Rule-Based Playlists

Rule playlists support composable boolean logic:

```json
{
  "vapourfly_schema": "vapourfly.playlist.v1",
  "created_by": "user",
  "playlist": {
    "id": "short-installed-rpgs",
    "name": "Short Installed RPGs",
    "description": "Installed RPGs under 20 hours with good ratings",
    "content": {
      "type": "Rules",
      "value": {
        "rules": [
          { "op": "Installed" },
          { "op": "NotJunk" },
          { "op": "NotHidden" },
          { "op": "HasGenre", "args": { "genre": "Role-playing (RPG)" } },
          { "op": "HltbMaxMinutes", "args": { "minutes": 1200 } },
          { "op": "RatingAtLeast", "args": { "rating_0_5": 3.5 } }
        ]
      }
    }
  }
}
```

Available rule operators: `ProtonAtLeast`, `HltbMaxMinutes`, `PlaytimeBetween`, `RatingAtLeast`, `HasGenre`, `HasTag`, `Installed`, `NotJunk`, `NotHidden`, `And`, `Or`, `Not`.

### Match Reports

Match a playlist against your library to see what you own, what is missing, and what you have played:

```bash
# Table output
vapourfly playlist match my-playlist.json

# JSON output for scripting
vapourfly playlist match my-playlist.json --format json
```

The match report includes owned, missing, played, unplayed, hidden, and junk counts.

### Sync to Steam

Sync a playlist or stored collection to a Steam cloud collection:

```bash
# Preview the sync
vapourfly sync collection my-playlist-id --dry-run

# Execute the sync
vapourfly sync collection my-playlist-id --confirm
```

## Cache Management

External API responses are cached locally. Refresh specific sources:

```bash
vapourfly cache refresh --source igdb
vapourfly cache refresh --source all
```

## Diagnostics

Export sanitized diagnostics for bug reports:

```bash
vapourfly diagnostics export --out diagnostics.json
```

See [docs/PRIVACY.md](docs/PRIVACY.md) for what is included and redacted.

## Documentation

- [docs/CLI.md](docs/CLI.md) -- Full command reference with examples
- [docs/STEAM_FILE_SAFETY.md](docs/STEAM_FILE_SAFETY.md) -- Write targets, backup strategy, and atomic writes
- [docs/API_SOURCES.md](docs/API_SOURCES.md) -- IGDB, RAWG, ProtonDB, PCGW, HLTB data strategy
- [docs/PRIVACY.md](docs/PRIVACY.md) -- Local-first design, redaction, and data handling

## License

Licensed under either of:

- MIT license ([LICENSE-MIT](LICENSE-MIT))
- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE))

at your option.

## Contributing

See [CONTRIBUTING.md](CONTRIBUTING.md) for guidelines.

## Acknowledgments

Vapourfly's design was informed by studying the following open-source projects. We gratefully acknowledge their authors:

- [Depressurizer](https://github.com/rallion/depressurizer) -- Steam library categorization tool. Inspired Vapourfly's understanding of VDF formats, Steam collections, and SteamID handling. (GPLv3)
- [Gameloop.Vdf](https://github.com/BeyondDimension/Gameloop.Vdf) -- C# Text VDF library. Served as reference for VDF token-level parsing behavior. (MIT)
- [SteamTools / BD.SteamClient](https://github.com/BeyondDimension/SteamClient) -- Watt Toolkit core library. Reference for cross-platform Steam path detection, appinfo.vdf format, and librarycache layout.
- [TinyWiiBackupManager](https://github.com/mq1/TinyWiiBackupManager) -- Rust game backup manager. Inspired the workspace architecture, egui state patterns, and HTTP client design. (GPL-3.0)

No code from these projects is incorporated into Vapourfly. See [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md) for details.

## Security

See [SECURITY.md](SECURITY.md) for the vulnerability reporting policy.
