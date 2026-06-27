# API Sources

Vapourfly enriches your Steam library with metadata from external data sources. This document explains each source, what data it provides, whether credentials are required, and how caching works.

## Source Overview

| Source | Credentials | Data Provided | Notes |
|---|---|---|---|
| **IGDB** | Client ID + Secret | Genres, themes, keywords, ratings (0-100), time-to-beat, similar games | Uses Twitch OAuth. Data is authoritative for game metadata. |
| **RAWG** | API key | Genres, tags, ratings (0-5), store availability | Large database with community-sourced data. |
| **ProtonDB** | None | Compatibility tier (Borked/Native/Platinum/Gold/Silver/Bronze), confidence, score | Community-reported Linux/Steam Deck compatibility. |
| **PCGamingWiki** | None | Controller support (Full/Partial/None), Steam Deck notes, fixes URL | MediaWiki-based, queried via the Cargo API. |
| **HLTB** | None | Main story time, main + extras time, completionist time | Optional feature gate (`hltb_scrape`). Default build returns no data. |
| **Steam Store** | None | App name, type, description, developers, publishers, genres, categories, price, platforms, Metacritic score | Public Steam Store API. |

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

IGDB allows 4 requests per second. Vapourfly respects this limit and caches responses locally.

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
**Authentication:** None

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
**Authentication:** None (MediaWiki Cargo query)

### Data Used

- **Controller Support** -- `Full`, `Partial`, `None`, or `Unknown`
- **Steam Deck Notes** -- Specific notes about Steam Deck compatibility
- **Fixes URL** -- Link to the PCGW page with known fixes and workarounds

## HLTB (HowLongToBeat)

**Source:** `howlongtobeat.com`
**Authentication:** None
**Feature Gate:** `hltb_scrape` (not enabled by default)

Time-to-beat data is also available through IGDB when IGDB credentials are
configured; that path does not require the `hltb_scrape` feature gate.

### Data Used

- **Main Story** -- Average time to complete the main story (seconds)
- **Main + Extras** -- Main story plus significant side content
- **Completionist** -- 100% completion time

### Use in Playlists and Junk Detection

```json
{ "op": "HltbMaxMinutes", "args": { "minutes": 600 } }
```

Junk detection uses the main story time to identify short games that the user has not played.

## Steam Store

**Endpoint:** `https://store.steampowered.com/api/appdetails`
**Authentication:** None

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

## Caching

All API responses are cached locally under the Vapourfly cache directory (reported by `vapourfly doctor`). The cache:

- Persists across CLI invocations
- Is keyed by API source and query parameters
- Can be refreshed per-source or globally

```bash
# Refresh a single source
vapourfly cache refresh --source igdb
vapourfly cache refresh --source steam-store

# Refresh all sources
vapourfly cache refresh --source all

# View cache status
vapourfly sources status
```

Cache refresh is blocked in `--offline` mode.

Cache refresh stores source responses locally. Junk, Recommend, Playlist Match,
Playlist Sync, Discover, and Dynamic Template workflows hydrate cached responses
into scanned game records before evaluating rules or scores. Hydration is
cache-only and never makes network requests. Use `scan --enrich --format json`
when you need to inspect enriched game data from the CLI.

## Data Priority

When multiple sources provide the same data, Vapourfly uses this priority order:

- **Ratings:** RAWG (native 0-5) > IGDB (0-100, converted to 0-5) > Manual override
- **Genres:** IGDB + RAWG (union of both)
- **Tags:** RAWG tags + IGDB keywords + IGDB themes + Steam collections (union)
- **Completion time:** Manual override > HLTB (if feature enabled) > IGDB time-to-beat

## Graceful Degradation

Vapourfly works without any API credentials. When external data is unavailable:

- Junk detection uses only Steam playtime data (confidence score reflects missing signals)
- Recommendation scoring falls back to local metadata
- Playlist rules that depend on missing data fail closed (positive predicates return false, negated predicates pass through)
- `vapourfly doctor` and `vapourfly sources status` report which credentials are missing
