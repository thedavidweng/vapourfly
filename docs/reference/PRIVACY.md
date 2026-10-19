# Privacy

Vapourfly is designed with a local-first philosophy. This document explains what data Vapourfly accesses, what leaves your machine, and how sensitive information is handled.

## Local-First Design

- **No Vapourfly server.** Local library state and authored preferences stay on your machine. Optional provider requests described below go directly to those providers.
- **No telemetry.** Vapourfly does not phone home, report usage statistics, or send analytics.
- **No account creation.** There is no Vapourfly account, login, or cloud sync service.
- **Local personal storage.** Vapourfly stores optional account-specific API snapshots and authored marks/preferences locally. It never collects Steam login credentials; optional API credentials and identifiers are transmitted only as described below.

## What Vapourfly Reads

Vapourfly reads the following files from your Steam installation:

| File | Data Read |
|---|---|
| `config/loginusers.vdf` | Steam account names and IDs (for account selection) |
| `userdata/<id>/config/localconfig.vdf` | Per-app playtime, last-played timestamps |
| `userdata/<id>/config/cloudstorage/cloud-storage-namespace-1.json` | Collections, hidden apps, and other cloud storage entries |
| `steamapps/appmanifest_*.acf` | Game names, install state, install directories |
| `steamapps/libraryfolders.vdf` | Library folder paths |
| `userdata/<id>/config/librarycache/librarycache.json` | Legacy aggregated game name fallbacks, when present |
| `appcache/appinfo.vdf` | App display names from Steam's app metadata cache |

## What Vapourfly Writes

The Steam write target is limited to:

```
userdata/<account_id_or_steam_id64>/config/cloudstorage/cloud-storage-namespace-1.json
```

This is Steam's cloud storage file for user-defined collections. See [STEAM_FILE_SAFETY.md](STEAM_FILE_SAFETY.md) for details on the write process and backup strategy.

Outside Steam, Vapourfly writes its own caches, settings, saved Playlists,
backups, and account-scoped local story marks and preference feedback. These
authored records are distinct from disposable provider caches.

## External API Calls

When credentials are configured or a source needs no key, Vapourfly makes HTTPS requests to external services:

| Service | Data Sent | Data Received |
|---|---|---|
| IGDB (api.igdb.com) | OAuth token, game queries (AppID or name) | Genres, ratings, time-to-beat, keywords |
| RAWG (api.rawg.io) | API key, game queries (AppID or name) | Genres, tags, ratings |
| ProtonDB (protondb.com) | AppID (in URL path) | Compatibility tier and confidence |
| PCGamingWiki (pcgamingwiki.com) | Game name (in MediaWiki query) | Controller support, fixes URL |
| Steam Store (store.steampowered.com) | AppID (in query parameter) | Store metadata, pricing, platform info |
| Steam Web API (api.steampowered.com) | User-created API key, SteamID64 and, for achievements, AppID | Owned/recent activity, name map, per-game achievement progress and shared schema |

IGDB, RAWG, ProtonDB, PCGW, and Steam Store requests use game AppIDs or names,
not the selected Steam account. Live eligibility is constrained by
[API_SOURCES.md](API_SOURCES.md), including currently disabled interfaces.
Optional Steam Web API requests send the selected SteamID64 and user-created
key for the documented name/activity/achievement operations. Local account
selection does not grant remote permission or bypass profile visibility. Steam
login credentials and local story/preferences are never sent.

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
- API credential status (configured / not configured, never the actual keys — including Steam Web API)
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

- **Personal inventory:** `accounts inventory` or Data Sources > Personal records
  reads only the selected SteamID64's activity/achievement caches and authored
  story/preferences, including orphaned title records. Counts and local write
  times are distinct from provider freshness, visibility and record validity.
  Inventory exports omit identifiers and per-game payloads; credentials appear
  only as configured/not configured. It does not clear records or create storage.
  Reopen the GUI dialog to refresh this read-only snapshot.
- **Account separation:** Activity uses `steam-activity/<SteamID64>.json`,
  achievement progress `steam-achievements/<SteamID64>-<AppID>.json` under the
  cache root. Story and preference roots each use `<SteamID64>/<AppID>.json`.
  Shared achievement schema, metadata, saved Playlists, settings and Steam
  collections are separate from this personal inventory.
- **API cache:** Stored locally until explicitly refreshed (`vapourfly cache refresh`) or manually deleted.
- **Backups:** Stored alongside the original file, pruned to the most recent 5 after each write.
- **Playlists:** Stored locally in the Vapourfly playlist directory. Never uploaded anywhere.

## Credential Storage

- **IGDB and RAWG** credentials are read from environment variables at runtime. They are not written to `config.toml`.
- **Steam Web API key** may be stored locally in `config.toml` as `steam_api_key`, or supplied via `VAPOURFLY_STEAM_API_KEY` (env wins). The key is never bundled with the app, never logged, and is shown masked in `doctor` / `settings show`.
- Credentials are transmitted only as required by the external API's authentication flow (IGDB OAuth token exchange, Steam Web API query parameter) over HTTPS.
