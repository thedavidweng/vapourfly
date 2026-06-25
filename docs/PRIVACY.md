# Privacy

Vapourfly is designed with a local-first philosophy. This document explains what data Vapourfly accesses, what leaves your machine, and how sensitive information is handled.

## Local-First Design

- **All library data stays on your machine.** Vapourfly reads and writes Steam configuration files directly. Your library metadata, playtime, collections, and preferences never pass through a Vapourfly server.
- **No telemetry.** Vapourfly does not phone home, report usage statistics, or send analytics.
- **No account creation.** There is no Vapourfly account, login, or cloud sync service.
- **No data collection.** Vapourfly does not collect, store, or transmit your Steam credentials, library contents, or personal information.

## What Vapourfly Reads

Vapourfly reads the following files from your Steam installation:

| File | Data Read |
|---|---|
| `config/loginusers.vdf` | Steam account names and IDs (for account selection) |
| `userdata/<id>/config/localconfig.vdf` | Per-app playtime, last-played timestamps |
| `userdata/<id>/config/cloudstorage/cloud-storage-namespace-1.json` | Collections, hidden apps, and other cloud storage entries |
| `steamapps/appmanifest_*.acf` | Game names, install state, install directories |
| `steamapps/libraryfolders.vdf` | Library folder paths |
| `appcache/librarycache/*.json` | Game name fallbacks from Steam's library cache |

## What Vapourfly Writes

Vapourfly writes to a single file:

```
userdata/<steam_id64>/config/cloudstorage/cloud-storage-namespace-1.json
```

This is Steam's cloud storage file for user-defined collections. See [STEAM_FILE_SAFETY.md](STEAM_FILE_SAFETY.md) for details on the write process and backup strategy.

## External API Calls

When API credentials are configured, Vapourfly makes HTTPS requests to external services:

| Service | Data Sent | Data Received |
|---|---|---|
| IGDB (api.igdb.com) | OAuth token, game queries (AppID or name) | Genres, ratings, time-to-beat, keywords |
| RAWG (api.rawg.io) | API key, game queries (AppID or name) | Genres, tags, ratings |
| ProtonDB (protondb.com) | AppID (in URL path) | Compatibility tier and confidence |
| PCGamingWiki (pcgamingwiki.com) | Game name (in MediaWiki query) | Controller support, Deck notes |
| Steam Store (store.steampowered.com) | AppID (in query parameter) | Store metadata, pricing, platform info |

No Steam credentials, library contents, or personal identifiers are sent to any external service. API calls use only game AppIDs or names for lookup.

All API responses are cached locally. See [API_SOURCES.md](API_SOURCES.md) for caching details.

## Redaction

By default, Vapourfly redacts sensitive information in CLI output:

- **Steam directory paths** are shown as partial paths with middle segments replaced by `***` (e.g., `/Users/***/Library/Application Support/Steam`)
- **Steam account names** are shown as `***` (the persona name is shown, but the login name is masked)
- **Steam IDs** show only the last 4 characters (e.g., `***7890`)

Use `--verbose` to disable redaction and show full paths and identifiers.

```bash
# Default: redacted
vapourfly doctor

# Verbose: full paths shown
vapourfly doctor --verbose
```

## Diagnostics Export

The `vapourfly diagnostics export` command produces a sanitized JSON file for bug reports. The diagnostics file includes:

- Vapourfly version
- OS and architecture
- Detected Steam directory (redacted unless `--verbose`)
- Number of accounts and library folders
- API credential status (configured / not configured, never the actual keys)
- Cache directory location (redacted unless `--verbose`)
- Any active warnings or errors

The diagnostics file does **not** include:

- Steam account names or IDs
- Game library contents
- Playtime data
- API keys or secrets
- File contents from Steam configuration files
- Network request logs

## Offline Mode

Pass `--offline` to ensure no network calls are made:

```bash
vapourfly scan --offline
```

In offline mode, all external API calls are blocked. Only locally cached data and Steam configuration files are used.

## Data Retention

- **API cache:** Stored locally until explicitly refreshed (`vapourfly cache refresh`) or manually deleted.
- **Backups:** Stored alongside the original file, pruned to the most recent 5 after each write.
- **Playlists:** Stored locally in the Vapourfly playlist directory. Never uploaded anywhere.

## Credential Storage

API credentials are read from environment variables at runtime. Vapourfly does not store credentials to disk, embed them in configuration files, or transmit them except as required by the external API's authentication flow (e.g., IGDB OAuth token exchange over HTTPS).
