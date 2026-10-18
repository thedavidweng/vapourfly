# API Sources

Vapourfly enriches your Steam library with metadata from external data sources. This document explains each source, what data it provides, whether credentials are required, and how caching works.

## Source Overview

| Source | Credentials | Data Provided | Notes |
|---|---|---|---|
| **IGDB** | Client ID + Secret | Genres, themes, keywords, ratings (0-100), time-to-beat, similar games | Uses Twitch OAuth. Provider metadata; estimates are not proof of story completion. |
| **RAWG** | API key | Genres, tags, ratings (0-5), store availability | Large database with community-sourced data. |
| **ProtonDB** | None | Compatibility tier (Borked/Native/Platinum/Gold/Silver/Bronze), confidence, score | Legacy cache only; summary endpoint permission unverified. |
| **PCGamingWiki** | None | Controller support (Full/Partial/None), fixes URL | Legacy cache only; Cargo now requires bot login. |
| **Legacy HLTB cache** | None | Previously cached main story, main + extras, and completionist times | Read-only compatibility data; never fetched or refreshed. |
| **Steam Store** | None | App name, type, description, developers, publishers, genres, categories, price, platforms, Metacritic score | Legacy cache only; published interface permission unverified. |
| **Steam Web API** | User-created key | Owned/recent activity, playtime and AppID → name map | Explicit account refresh makes at most one `GetOwnedGames` and one `GetRecentlyPlayedGames` request; snapshots are partitioned by SteamID64. |

## Access and licensing audit (2026-10-18)

A reachable URL does not establish permission. This inventory separates a
published interface from a data license. No provider key is bundled with
Vapourfly. Local scanning and existing caches remain usable without keys.
The production HTTP backend rejects the three disabled legacy integrations
before network I/O; parser tests may still use mock responses.

| Source / actual endpoints | Published access and eligibility | License / quota evidence | Current decision |
|---|---|---|---|
| IGDB `/v4/external_games`, `/v4/games`, `/v4/game_time_to_beats`; Twitch `/oauth2/token` | [IGDB documentation](https://api-docs.igdb.com/): Twitch developer account, 2FA, own client ID/secret, client-credentials OAuth | Noncommercial use under Twitch Developer Service Agreement; commercial partnership and user-facing attribution required for commercial products. Local caching explicitly permitted. 4 req/s, 8 open requests. | Enabled only with user credentials; 3 req/s, one in-flight request per shared client. Commercial redistribution requires the provider arrangement, not merely possession of a key. |
| RAWG `/api/games` search and game details | [RAWG API/pricing/terms](https://rawg.io/apidocs): own API key on every request | Free personal/hobby plan is noncommercial, 20,000/month with backlinks wherever data is used; business plans have separate eligibility/quotas. No unrestricted data/image redistribution license is inferred. | Credential-gated. User must choose an eligible plan; local pacing is not monthly quota enforcement. |
| Steam Web `/IPlayerService/GetOwnedGames/v1/` (and planned recent/achievement interfaces) | [IPlayerService](https://partner.steamgames.com/doc/webapi/IPlayerService), [key classes](https://partner.steamgames.com/doc/webapi_overview/auth): normal user key and profile/game-details visibility; publisher key is not required for these player interfaces | [Valve terms](https://steamcommunity.com/dev/apiterms): personal-use Steam Data subject to terms, key secrecy, privacy, 100,000 calls/day. No privacy bypass or permission to scrape other sites. | Optional user key. Cache account-specific responses by SteamID64. No embedded key or Steam-client key extraction. |
| Steam Store `/api/appdetails` | No Valve published API contract for this endpoint was established by this audit. Steam Web API documentation is not evidence for Store endpoint permission. | No published Store-specific rate/license entitlement verified. Previous 200/5min claim was an observation, not policy. | **Live disabled, cache reads retained.** Restore only after an official interface/permission is established. |
| ProtonDB `/api/v1/reports/summaries/{appid}.json` | [Provider data-export repository](https://github.com/bdefore/protondb-data) publishes exports, not a supported summary API contract | Export ODbL does not itself authorize arbitrary live endpoints or transfer artwork rights; summary API rate/access policy unverified. | **Live disabled, cache reads retained.** A separately licensed export importer or provider authorization is needed before re-enabling. |
| PCGamingWiki `/w/api.php?action=cargoquery` | [Official API policy](https://www.pcgamingwiki.com/wiki/PCGamingWiki:API): since August 2026 Cargo requires bot-password login; existing anonymous client does not implement it | [Copyrights](https://www.pcgamingwiki.com/wiki/PCGamingWiki:Copyrights): CC Attribution Non-Commercial Share Alike unless noted; 60 requests/minute, identifying user-agent; permission errors must not become empty data. | **Live disabled, cache reads retained.** A documented login implementation, eligible credentials and attribution are prerequisites to restore. |
| HLTB | Undocumented website interfaces removed in #54 | No new live access or redistribution entitlement claimed | **Live removed.** Existing local records retain provenance; no published asset/cache rewrite. |

Re-enable prerequisites are explicit engineering/provider work, not a reason
to block local-first Steam activity implementation. Do not replace disabled
interfaces with scraping, reverse engineered tokens or hidden fallbacks.
Data/cover-image rights are separate from source-code licenses; no newly
bundled third-party dataset or artwork is included by this change.

## IGDB

**Endpoint:** `https://api.igdb.com/v4`
**Authentication:** Twitch OAuth (client credentials flow)

### Setup

1. Go to the [Twitch Developer Console](https://dev.twitch.tv/console).
2. Register an application (category: "Application Integration").
3. Copy the Client ID and Client Secret.
4. Set environment variables:

```bash
export VAPOURFLY_IGDB_CLIENT_ID="your_client_id"
export VAPOURFLY_IGDB_CLIENT_SECRET="your_client_secret"
```

### Data Used

- **Genres** -- e.g., "Role-playing (RPG)", "Shooter", "Strategy"
- **Themes** -- e.g., "Horror", "Science fiction"
- **Keywords** -- e.g., "open world", "roguelike", "souls-like"
- **Rating** -- IGDB rating on a 0-100 scale (converted to 0-5 for internal use)
- **Total Rating** -- Weighted average of external ratings on a 0-100 scale
- **Time to Beat** -- Hastily, normally, and completely completion times
- **Similar Games** -- List of related IGDB game IDs
- **Steam App ID confirmation** -- Whether the IGDB entry is confirmed to match the Steam AppID

### Rate Limiting

IGDB documents 4 requests per second and at most 8 open requests. Vapourfly paces requests at 3 per second without an initial burst and serializes IGDB requests across clones of one client. Separate application processes do not share a limiter. Responses are cached locally.

## RAWG

**Endpoint:** `https://api.rawg.io/api`
**Authentication:** API key in query parameter

### Setup

1. Go to [rawg.io/apidocs](https://rawg.io/apidocs).
2. Sign up and get an API key.
3. Set the environment variable:

```bash
export VAPOURFLY_RAWG_KEY="your_api_key"
```

### Data Used

- **Genres** -- e.g., "Action", "RPG", "Indie"
- **Tags** -- e.g., "roguelike", "co-op", "pixel graphics"
- **Rating** -- Community rating on a 0-5 scale (native format, no conversion needed)
- **Ratings Count** -- Number of community ratings
- **Store Availability** -- Which stores carry the game

### Rate Limiting

RAWG allows 20,000 requests per month on the free tier. Vapourfly caches responses to minimize API calls.

## ProtonDB

**Endpoint:** `https://www.protondb.com/api/v1/reports/summaries/{appid}.json`
**Authentication:** None in the legacy adapter; live requests disabled.

### Data Used

- **Tier** -- Compatibility rating: `Native`, `Platinum`, `Gold`, `Silver`, `Bronze`, `Borked`, or `Unknown`
- **Confidence** -- How confident the community is in the rating (e.g., "high", "low")
- **Score** -- Numerical confidence score

A 404 or empty response maps to `Unknown` tier.

### Use in Playlists

ProtonDB data is useful for Steam Deck and Linux compatibility filtering:

```json
{ "op": "ProtonAtLeast", "args": { "tier": "Gold" } }
```

## PCGamingWiki

**Endpoint:** `https://www.pcgamingwiki.com/w/api.php`
**Authentication:** Bot login required for Cargo since August 2026; not implemented. Live requests disabled.

### Data Used

- **Controller Support** -- `Full`, `Partial`, `None`, or `Unknown`
- **Fixes URL** -- Link to the PCGW page with known fixes and workarounds

Steam Deck compatibility is taken from ProtonDB, not PCGW.

## Completion time

Vapourfly's supported live completion-time source is the documented IGDB
`game_time_to_beats` endpoint, available when IGDB credentials are configured.
The `normally` value is used for time-dependent workflows and remains marked
as `IgdbGameTimeToBeat` in the model. Explicit manual overrides take priority.

Previously cached HLTB records remain readable under the `hltb` cache namespace
and retain `HltbScrape` provenance. They are legacy compatibility data: the
application never calls HLTB endpoints, does not refresh that namespace, and
does not list it in CLI or GUI source controls. If no cached legacy value or
documented IGDB value exists, completion-time workflows treat the signal as
unknown rather than translating another duration field. While a legacy record
exists, it remains the preferred completion value; removing that local cache
record allows the documented IGDB value to be used on the next refresh without
rewriting or silently reinterpreting the HLTB record.

### Use in Playlists and Junk Detection

```json
{ "op": "HltbMaxMinutes", "args": { "minutes": 600 } }
```

The completion-time rule uses the supported precedence above. Legacy HLTB data
is still useful when present, while new live data is explicitly attributed to
IGDB.

## Steam Store

**Endpoint:** `https://store.steampowered.com/api/appdetails`
**Authentication:** None in the legacy adapter; live requests disabled.

### Data Used

- **Name** -- Official game title
- **Type** -- game, dlc, demo, application, etc.
- **Short Description** -- Store description text
- **Header Image** -- Store header image URL
- **Developers / Publishers** -- Company names
- **Genres / Categories** -- Steam's own genre and category tags
- **Release Date** -- When the game was released
- **Metacritic Score** -- Aggregated review score
- **Platforms** -- Windows, macOS, Linux support
- **Price Overview** -- Current price, discount percentage
- **Coming Soon** -- Whether the game is not yet released

## Steam Web API

**Endpoint:** `https://api.steampowered.com/IPlayerService/GetOwnedGames/v1/`
**Authentication:** User-created Steam Web API key

Used only to resolve `"App <id>"` placeholder names when `appinfo.vdf` is
missing (common on macOS). One request, cached for one day.

### Setup

1. Create a key at [steamcommunity.com/dev/apikey](https://steamcommunity.com/dev/apikey) (any domain works, e.g. `localhost`).
2. Store it locally — never share it:

```bash
vapourfly settings set steam_api_key <your-key>
# or
export VAPOURFLY_STEAM_API_KEY="your-key"
```

The environment variable overrides the config file. `settings show` and
`doctor` print the key masked. Per Valve's terms the key is personal and is
never bundled with the app.

Without a key, existing Steam Store caches may resolve names; fresh Store fetching is disabled.

### Account activity snapshots

`accounts activity refresh` is an explicit, bounded operation. It uses the
documented `IPlayerService/GetOwnedGames` and
`IPlayerService/GetRecentlyPlayedGames` user-key interfaces, never extracts a
key from Steam files, and never blocks `workflow::prepare` or Library first
paint. Each selected SteamID64 has an independent `steam-activity/<id>` cache
record. A refresh makes no more than one request to each endpoint; `--offline`
and a missing key make zero requests and leave the last snapshot unchanged.

The snapshot distinguishes available data, a confirmed empty response,
restricted/private visibility, unauthorized keys, rate limiting, malformed
payloads, incomplete payloads and transport/HTTP failures. Failed refreshes
retain the last valid game rows and fetched timestamp while reporting the
failure state. Request URLs and diagnostics never retain or print the key.

Inspect without network access:

```bash
vapourfly accounts activity status --account <steamid64>
vapourfly accounts activity refresh --account <steamid64>
```

No real key or device sign-in is required by tests or local cache workflows;
provider permission and private-profile behavior remain real-account
acceptance boundaries.
The remaining name-keyed source (RAWG) skips placeholder-named games.

## Caching

All API responses are cached locally under the Vapourfly cache directory (reported by `vapourfly doctor`). The cache:

- Persists across CLI invocations
- Is keyed by API source and query parameters
- Can be refreshed per-source or globally

```bash
# Refresh a single source
vapourfly cache refresh --source igdb

# Refresh all sources
vapourfly cache refresh --source all

# View cache status
vapourfly sources status
```

Cache refresh is blocked in `--offline` mode.

`workflow::prepare` (junk, recommend, playlist, discover, editorial mood,
dynamic template, GUI library scan) hydrates from this cache only — including
stale entries — plus at most one bounded name-map request when a Steam Web
API key is configured (ADR-0009). Playlist match can use cached
Steam Store prices; uncached prices remain unknown. For enabled sources, populate missing or stale entries with
`cache refresh`, `scan --enrich`, or the GUI's background populate job.
`--offline` blocks every network call. Per-game fetch failures degrade
gracefully — the workflow always returns a result.

## Data Priority

When multiple sources provide the same data, Vapourfly uses this priority order:

- **Ratings:** RAWG (native 0-5) > IGDB (0-100, converted to 0-5) > Manual override
- **Genres:** IGDB + RAWG (union of both)
- **Tags:** RAWG tags + IGDB keywords + IGDB themes + Steam collections (union)
- **Completion time:** Manual override > cached legacy HLTB > IGDB `normally` time-to-beat

## Graceful Degradation

Vapourfly works without any API credentials. When external data is unavailable:

- Junk detection uses only Steam playtime data (confidence score reflects missing signals)
- Recommendation scoring falls back to local metadata
- Playlist rules that depend on missing data fail closed (positive predicates return false, negated predicates pass through)
- Names stay as `"App <id>"` until local/cached metadata or the optional owned-games map fills them
- `vapourfly doctor` and `vapourfly sources status` report which credentials are missing
