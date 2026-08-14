# CLI Reference

Full command reference for the `vapourfly` CLI.

For CLI/GUI parity and current feature status, see
[FEATURES.md](FEATURES.md).

## Global Flags

These flags are available on all commands:

| Flag | Description |
|---|---|
| `--steam-dir <path>` | Override the Steam installation directory |
| `--account <id>` | Override the Steam account identifier |
| `--verbose` | Show full paths instead of redacted names (also enables debug logging) |
| `--offline` | Prohibit network calls; use cache only |
| `--allow-steam-running` | Allow writes while Steam is detected as running |
| `--help` | Print help for a command |
| `--version` | Print the version |

---

## Commands

### `vapourfly doctor`

Diagnose the Steam installation, accounts, credentials, and cache.

```bash
vapourfly doctor
vapourfly doctor --verbose
```

Reports:
- Steam directory (auto-detected or overridden)
- Number of detected accounts and which one is selected
- Number of library folders
- Cloud storage availability
- Cache root location
- IGDB, RAWG, and Steam Web API credential status (keys are never printed)

### `vapourfly accounts list`

List detected Steam accounts. The active account is marked with `*`.

```bash
vapourfly accounts list
vapourfly accounts list --verbose
```

### `vapourfly scan`

Scan the Steam library and print game metadata.

```bash
# Table output (default)
vapourfly scan

# JSON output
vapourfly scan --format json

# JSON output with external metadata populated when available
vapourfly scan --enrich --format json
```

**Output columns (table):** AppID, Name, Installed, Playtime (minutes), Collections count.

**JSON output** includes: `app_id`, `name`, `installed`, `playtime_minutes`, `playtime_2wks_minutes`, `collections`, `is_hidden`, plus a `warnings` array.

`--enrich` uses IGDB, RAWG, ProtonDB, PCGW, HLTB, and Steam Store sources. It
populates the local cache (and applies cached entries, including stale ones)
unless global `--offline` is set.

### `vapourfly collections`

Manage Steam collections stored in cloud storage.

```bash
# List all collections with app counts
vapourfly collections list

# Export collections to a Vapourfly JSON file
vapourfly collections export --out collections.json

# Compile a dynamic template into a stored playlist
vapourfly collections dynamic deck-session --minutes 90
vapourfly collections dynamic finish-it --out finish-it.json

# List available Editorial Moods
vapourfly collections mood

# Compile an Editorial Mood into a stored playlist
vapourfly collections mood friday-party
vapourfly collections mood quick-round --out quick-round.json
```

Hidden collections are reported separately as a count.

Dynamic templates and Editorial Moods hydrate from the local cache before
compiling (ADR-0009). Fetch failures degrade gracefully. Use `--offline` to
block network entirely. Populate missing cache entries with `cache refresh`
or `scan --enrich`.

### `vapourfly junk`

Junk detection and management. See the [junk rules section](#junk-modes) below for mode details.

```bash
# Preview junk candidates (default mode)
vapourfly junk preview

# Strict mode
vapourfly junk preview --strict

# Aggressive mode
vapourfly junk preview --aggressive

# JSON output
vapourfly junk preview --format json

# Apply junk classification to a Steam collection
vapourfly junk apply --collection "junk" --dry-run
vapourfly junk apply --collection "junk" --confirm

# Move junk games to the hidden collection
vapourfly junk hide --dry-run
vapourfly junk hide --confirm
```

**Output columns (preview table):** App ID, Name, Playtime, Confidence, Classification (junk + reasons, or ok).

#### Junk Modes

| Mode | Flag | Logic |
|---|---|---|
| Default | (none) | Low playtime AND at least one other negative signal, minimum 2 data points |
| Strict | `--strict` | All available signals must indicate junk, minimum 2 data points |
| Aggressive | `--aggressive` | Low playtime AND at least one other negative signal, no minimum |

Cannot use `--strict` and `--aggressive` together.

**Signals evaluated:**
- **Low playtime** -- playtime below threshold (default: 30 minutes)
- **Short completion** -- HLTB main story below threshold (default: 7200 seconds / 2 hours)
- **Low rating** -- rating below threshold (default: 2.5 out of 5)

Each decision includes a confidence score (fraction of possible signals for which data was available) and lists which signals matched and which were missing.

Current CLI junk commands scan the library, hydrate from the local cache
(ADR-0009), and then evaluate junk rules. Fetch failures degrade gracefully;
use `--offline` to block network entirely.

### `vapourfly recommend`

Get game recommendations based on available play time.

```bash
# 5 recommendations for 2 hours of play time
vapourfly recommend --minutes 120

# 3 recommendations, installed only, Steam Deck optimized
vapourfly recommend --minutes 60 --count 3 --installed-only --deck

# Reproducible with a seed
vapourfly recommend --minutes 120 --seed 42

# Exclude games in specific Steam collections (repeatable, by name)
vapourfly recommend --minutes 120 --exclude-collection "Favorites" --exclude-collection "Backlog"

# JSON output
vapourfly recommend --minutes 120 --format json

# Save recommendations to the temporary Steam collection `vapourfly-picks`
vapourfly recommend --minutes 120 --count 5 --to-collection --dry-run
vapourfly recommend --minutes 120 --count 5 --to-collection --confirm
```

**Output columns (table):** App ID, Name, Score, Reasons.

Current CLI recommendations scan the library, hydrate from the local cache
(ADR-0009), annotate junk flags, and then score games. Fetch failures degrade
gracefully; use `--offline` to block network entirely.

### `vapourfly playlist`

Import, export, and match playlists.

```bash
# Create a manual playlist and store it locally
vapourfly playlist create --id deck-shortlist --name "Deck Shortlist" --app-ids 292030,367520

# Create a rule-based playlist from a JSON rules file
vapourfly playlist create-rules --id installed-unplayed --name "Installed Unplayed" --rules rules.json

# Import a playlist from JSON
vapourfly playlist import my-playlist.json

# Import a playlist from a share code
vapourfly playlist import --code 'VF1:...'

# Export a stored playlist by ID
vapourfly playlist export my-playlist-id --out exported.json

# Print a share code for a stored playlist
vapourfly playlist share my-playlist-id

# Generate a Discover playlist from cached metadata
vapourfly playlist discover --count 20
vapourfly playlist discover --seed 367520 --out discover.json

# Match a playlist against the library
vapourfly playlist match my-playlist.json
vapourfly playlist match my-playlist.json --format json
```

**Match report columns (table):** Owned, Missing, Played, Unplayed, Hidden, Junk counts, plus Completion price (sum of Steam Store final prices for **missing, non-free** playlist entries; owned/unplayed games are never included; rule playlists have no missing entries so their completion price is absent). In online mode missing prices are fetched on-demand from the Steam Store API; with `--offline` only cached prices are used. Mixed currencies are reported as per-currency grouped totals.

**Playlist types:**
- **Manual** -- explicit list of AppIDs
- **Rules** -- boolean expression tree evaluated against game metadata

Available rule operators: `ProtonAtLeast`, `HltbMaxMinutes`,
`ControllerSupportFull`, `PlaytimeBetween`, `RatingAtLeast`, `HasGenre`,
`HasTag`, `Installed`, `NotJunk`, `NotHidden`, `And`, `Or`, `Not`.

`playlist create-rules --rules <file>` accepts either a bare JSON rules array
(`[{"op":"Installed"}, ...]`) or a full Vapourfly playlist JSON file (the rules
are extracted from `content.value.rules`).

Playlist import, match, sync, and discover workflows hydrate from the local
cache before evaluating rules or similarity (ADR-0009). Playlist match may
fetch Steam Store prices for missing entries unless `--offline` is set. Fetch
failures degrade gracefully. Rules that depend on external metadata only
match when the relevant data is available.

### `vapourfly sync`

Sync a playlist or stored collection to Steam cloud storage.

```bash
# Preview sync
vapourfly sync collection my-playlist-id --dry-run

# Execute sync
vapourfly sync collection my-playlist-id --confirm
```

The playlist ID is slugified to produce the Steam collection ID. For rule-based playlists, the rules are evaluated against the current library to resolve matching AppIDs.
Rule-based sync hydrates from the local cache before resolving matching AppIDs
(ADR-0009).

### `vapourfly cache`

Manage the local API data cache.

```bash
# Refresh a specific source
vapourfly cache refresh --source igdb
vapourfly cache refresh --source steam-store

# Refresh all sources
vapourfly cache refresh --source all
```

Valid sources: `igdb`, `rawg`, `protondb`, `pcgw`, `hltb`, `steam-store`, `all`.

Cache refresh is blocked in `--offline` mode.

### `vapourfly sources`

Show the status of external data sources.

```bash
vapourfly sources status
vapourfly sources status --format json
```

**Output columns (table):** Source, Credentials (configured / missing / not required), Last Success, Entries, Stale, Cached (whether the cache directory exists).

### `vapourfly backup`

Manage timestamped backups of Steam files.

```bash
# List backups
vapourfly backup list
vapourfly backup list --format json

# Preview a restore without writing
vapourfly backup restore /path/to/backup.json --dry-run

# Restore a backup (a write operation: requires --dry-run or --confirm)
vapourfly backup restore /path/to/backup.json --confirm
```

**Output columns (list table):** Path, Created, SHA256 (first 8 chars).

### `vapourfly diagnostics`

Export sanitized diagnostics for bug reports.

```bash
vapourfly diagnostics export --out diagnostics.json
```

See [PRIVACY.md](PRIVACY.md) for what is included and redacted.

### `vapourfly settings`

Show or edit Vapourfly settings stored in `config.toml`.

```bash
# Show resolved configuration (table)
vapourfly settings show

# Show resolved configuration (JSON)
vapourfly settings show --format json

# Set a field
vapourfly settings set cc JP
vapourfly settings set lang japanese
vapourfly settings set backup_retention_count 10
vapourfly settings set steam_dir /opt/steam
vapourfly settings set account myuser
vapourfly settings set steam_api_key <your-key>   # from https://steamcommunity.com/dev/apikey

# Remove a field (falls back to env var or platform default)
vapourfly settings unset account
```

**Settable fields:** `steam_dir`, `account`, `cc`, `lang`, `backup_retention_count`, `steam_api_key`.

`steam_api_key` is your own free Steam Web API key (create one in a minute at
<https://steamcommunity.com/dev/apikey>; any domain works, e.g. `localhost`).
With a key configured, every scan resolves all owned game names in one
request; without one, names backfill progressively from Steam Store
hydration. The key stays on your machine (`config.toml`, or the
`VAPOURFLY_STEAM_API_KEY` environment variable, which takes precedence) —
it is personal per Valve's API terms and is never bundled with the app.
`settings show` displays it masked.

`settings show` reports the resolved configuration (CLI flags and environment
variables override config file values, matching the precedence documented at
the top of `vapourfly-core/src/config.rs`). The config file path is printed so
you can edit it by hand if needed.

---

## Write Operations

All commands that modify Steam files (`junk apply`, `junk hide`, `recommend --to-collection`, `sync collection`, `backup restore`) require exactly one of:

| Flag | Behaviour |
|---|---|
| `--dry-run` | Show the diff without writing anything |
| `--confirm` | Execute the write with an automatic backup |

Omitting both flags is an error. Specifying both is an error.

---

## Output Formats

Most commands that produce tabular output support `--format`:

| Value | Description |
|---|---|
| `table` | Human-readable table (default) |
| `json` | Structured JSON with a schema version field |

---

## Environment Variables

| Variable | Purpose |
|---|---|
| `VAPOURFLY_STEAM_DIR` | Override Steam installation directory |
| `VAPOURFLY_ACCOUNT` | Override Steam account selection |
| `VAPOURFLY_CC` | Steam Store country code for price queries |
| `VAPOURFLY_LANG` | Steam Store language |
| `VAPOURFLY_IGDB_CLIENT_ID` | IGDB / Twitch client ID |
| `VAPOURFLY_IGDB_CLIENT_SECRET` | IGDB / Twitch client secret |
| `VAPOURFLY_RAWG_KEY` | RAWG API key |
| `VAPOURFLY_STEAM_API_KEY` | Steam Web API key for owned-game name resolution (overrides `steam_api_key` in config) |
| `RUST_LOG` | Override log level (default: `warn`, or `debug` with `--verbose`) |
