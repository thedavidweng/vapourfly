# Command Reference

Full command reference for the `vapourfly` CLI.

If you are learning the tool or trying to accomplish a specific task, start
with a guide instead -- each one walks a scenario end to end with real
output:

- First contact: [Getting started](../tutorials/getting-started.md)
- Flag and remove games you will never play:
  [Purge junk](../how-to/purge-junk.md) (`junk`)
- Picks that fit your available time: [Plan a Deck session](../how-to/plan-deck-session.md)
  (`recommend`, `collections dynamic`)
- Curated lists, share codes, Steam collections:
  [Share and sync playlists](../how-to/share-and-sync-playlists.md)
  (`playlist`, `sync collection`)
- Zero-network operation: [Work offline](../how-to/work-offline.md)
  (`--offline`, `cache`)
- Recovery from an unwanted write:
  [Back up and restore](../how-to/back-up-and-restore.md) (`backup`)
- Unlocking IGDB/RAWG/Steam Web API data:
  [Configure API credentials](../how-to/configure-api-credentials.md)
  (`settings set steam_api_key`, `sources`)

For CLI/GUI parity and current feature status, see
[FEATURES.md](FEATURES.md). For what happens to Steam files on writes, see
[STEAM_FILE_SAFETY.md](STEAM_FILE_SAFETY.md).

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

### Personal Record Inventory (Read-Only)

```bash
vapourfly --offline accounts inventory
vapourfly --account <steamid64> accounts inventory --format json
```

Inspect selected-account cached activity, cached achievements, local story marks
and preference files, including titles no longer in the Library. This performs
no network requests or writes. Counts describe on-disk records, not ownership,
valid records or achievement coverage. Local write timestamps are not provider
freshness. Activity evidence separately reports its cached availability and
freshness; absent or unreadable snapshots mean unknown, not an empty library.
Storage access failures are errors, not zero counts.

Data Sources > Personal records shows the same inventory, credential-configured
boolean and refresh prerequisites. Local selection grants no remote permission;
refresh still requires an authorized key and provider visibility, and is disabled
offline. Reopen the dialog for a fresh snapshot. Inventory exports omit account
identifiers, per-game payloads and credentials. This command does not clear data.

### Confirmed Personal Cleanup

```bash
vapourfly --offline --account <steamid64> accounts clear --scope cached
vapourfly --offline --account <steamid64> accounts clear --scope cached --confirm
vapourfly --offline --account <steamid64> accounts clear --scope authored --confirm
```

Without `--confirm`, print the inventory and scope warning without removing
records. The preview takes account locks until it exits; live personal refresh
and authored writers fail busy while another preview/cleanup owns their scope.
`cached` removes the activity snapshot and all per-game player achievement
records, including orphaned titles; shared achievement schemas remain.
`authored` permanently removes the selected account's story marks and preferences
(including invalid/temporary authored files). This cannot be undone.
Other accounts, metadata, Playlists, unrelated settings and Steam files remain.
No key or network is required. Partial failures are explicit; reopen inventory
before retry. Following profile/ranking commands read current disk evidence.
Data Sources has the same two preview/confirm actions, rejects active personal
refresh, checks the current account again on confirm and reloads derived evidence.

### Account Progress (Local And Cache-Only)

```bash
vapourfly accounts progress 730 --story completed
vapourfly accounts progress 730 --story undo
vapourfly --offline accounts progress --filter unfinished
vapourfly accounts progress --filter near-perfect --format json
```

With no AppID, list/filter the selected account's owned Library. `--story`
requires an owned AppID and changes only Vapourfly's local narrative mark,
never Steam files or achievements. Unfinished excludes marked stories and
achievement Perfected games, and requires known activity evidence. Near-perfect
requires fresh authoritative achievement counts in [90%, 100%). Unknown activity
excludes Unfinished; missing or stale achievement counts exclude Near-perfect.
Known playtime alone can qualify for Unfinished. Table output includes evidence coverage; JSON
contains AppID and progress only, without account identifiers or unlock history.
No network requests or API credentials are required. Local marks survive cache
clears; corrupt records report an error instead of silently losing the mark.

### Local Preferences (Selected Account)

```bash
vapourfly accounts preferences list --format json
vapourfly accounts preferences set 730 interested
vapourfly accounts preferences set 730 not-interested
vapourfly accounts preferences set 730 not-now
vapourfly accounts preferences reset 730
vapourfly accounts preferences reset-all --confirm
```

All preference operations are local/cache-only, use the resolved selected account,
and never write Steam files. Set requires an owned AppID; reset can remove an old
record even if the game is no longer scanned. List includes expired Not now
records, with an explicit expiry in JSON; output omits account identifiers.
Reset all requires confirmation and removes only this account's choices, not
story marks, caches, Playlists or settings. It cannot be undone.

Last choice wins. Not interested and active Not now exclude before scoring in
`recommend` and `playlist discover`; Not now expires exactly 30 days after setting
it. Interested adds an explained +0.5 without overriding existing eligibility or
excluded Collections. Preferences are not Steam/community tags. Rescan the GUI
after CLI edits; newly generated picks evaluate expiry using the current time.
Corrupt preference records report errors rather than silently restoring excluded
candidates. Existing dry-run/confirmation/backup/Steam-running write gates remain.

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

`--enrich` uses IGDB, RAWG, ProtonDB, PCGW, and Steam Store sources. It
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
- **Short completion** -- effective completion time below threshold (default:
  7200 seconds / 2 hours; manual override, legacy HLTB cache, then IGDB
  `normally` time-to-beat)
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

**Output columns (table):** App ID, Name, Score, signed Reasons and limitations.

Ranking shares the selected account's cached canonical Taste Profile, activity,
achievements and local Story completed/preferences with the GUI. Continuous
affinity and bounded variety replace binary keywords/random relevance noise;
`--seed` changes ties only. Unknown evidence is labeled, not treated as zero
activity or completed story. Relative fit is not probability or a player
percentile. This does not establish session-duration provider rights (#27/#36).

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

Discover uses the selected account's cached canonical profile, progress and local
feedback. Its output includes complete signed reasons, cache coverage and missing
seed evidence. An optional seed only gets a bounded boost from validated cached
IGDB links; no key/metadata still permits eligible owned picks. It does not fetch
provider data or alter Steam files; the generated file is the existing Manual
Playlist type. Use the normal explicit sync workflow to write a Collection.

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

Valid sources: `igdb`, `rawg`, `protondb`, `pcgw`, `steam-store`, `all`.

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
`settings show` and setting confirmations report only `configured`, never key bytes or suffixes. Account overrides are also status-only: table output shows `configured` or `(auto)`, and the JSON `account` field is `"configured"` or `null`. This applies even with `--verbose`. Use `accounts list` for selectable public persona names; it never prints login names or Steam IDs.

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
# Account achievement progress

## Account taste profile

`accounts profile [--format table|json]` derives reproducible Top Tags for the
selected account. This command is always local/cache-only, even without
`--offline`; it does not refresh providers or persist the derived profile.
Output separates long-term/recent strengths, genre/descriptor kind, library-relative
lift (never a population percentile), supporting titles and source/time/coverage
evidence. A bare fixture/no-metadata library produces an honest empty explanation.
The same output includes rule-based playstyle traits (Completion-focused, Deep
diver, Explorer), thresholds and supporting/contrary/unknown titles. JSON retains
eligible title activity provenance even without metadata. Missing story marks
remain unknown; stale achievement counts never support completion-focused.
Account selection follows the same resolved CLI/environment/config choice as scan.

```bash
vapourfly --offline accounts profile
vapourfly --account 76561198000000000 accounts profile --format json
```

## Achievement progress

`accounts achievements <AppID> [--refresh] [--format table|json]` inspects
official progress for an owned Library game. The default and `--offline`
are cache-only. Explicit refresh makes at most two documented Steam user-key
requests and reports unknown/restricted/incomplete separately from zero.
JSON excludes account identifiers and individual unlock history.

`accounts achievement-coverage [--app-id <AppID>] [--limit 1..12]
[--refresh] [--format table|json]` reports the bounded active subset, rather
than whole-library completion. An explicitly selected owned game comes
first, followed by recently played owned games from local play history and
cached account activity. The default reads cache only.

```bash
vapourfly accounts achievement-coverage --format json
vapourfly accounts achievement-coverage --app-id 620 --limit 3 --refresh
vapourfly --offline accounts achievement-coverage --refresh
```

Explicit refresh processes at most 12 games sequentially, with at most one
schema and one player request each (24 total), existing Steam pacing and no
retries. One OS lock per account prevents concurrent batches. Provider-wide
authorization, rate-limit, HTTP or transport failure stops the batch; prior
counts remain stale evidence. Coverage reports available/stale/unavailable/
missing counts, attempted games and pending games left after a stop.
Offline and no-key refreshes inspect cached evidence without requests.

### Recommendation intent and duration

`recommend --minutes 60` plans a **play session**, not an entire campaign. A
40-hour game remains eligible. Cached Arcade/Puzzle/Racing/Sport(s)/Roguelike
concepts give a transparent +0.25 short-loop heuristic only for 1-60 minute
windows; this is not measured session length or a promise of a stopping point.
Missing session evidence is neutral/unknown. No completion-time bonus applies.

Use `recommend --minutes 60 --finish-short-game` to deliberately restrict picks
to positive cached **full completion estimates** within that window. Unknown,
zero and longer estimates are excluded rather than called suitable. Reasons
identify cached IGDB normally, legacy cached HLTB or explicitly supplied manual
provenance. Playtime and achievements are never subtracted to invent remaining
hours. Manual Junk overrides retain their existing Junk-only scope; this flag
adds no override store. Offline/no credentials work without a provider request.

The existing Planner uses the same default and an explicit **Finish a short
game** switch; displayed results identify their generated request even when
controls are subsequently edited. JSON requests add `intent` and human guidance;
old core request JSON without `intent` means `session`. Existing duration-rule
names, Playlist JSON, VF1 codes and explicit Steam-write confirmation are unchanged.
This software distinction does not resolve the external #27 rights disposition.
