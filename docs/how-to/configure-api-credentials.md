# Configure API Credentials

Vapourfly works with no credentials at all -- ProtonDB, PCGamingWiki, HLTB,
and Steam Store data need none. Credentials unlock the rest: IGDB and RAWG
metadata (genres, ratings, time-to-beat, similar games) and instant
owned-game name resolution via the Steam Web API.

## 1. Get the keys

| Variable / setting | Where to create it | Unlocks |
|---|---|---|
| `VAPOURFLY_IGDB_CLIENT_ID` + `VAPOURFLY_IGDB_CLIENT_SECRET` | [Twitch Developer Console](https://dev.twitch.tv/console) | IGDB genres, ratings, time-to-beat, similar games (feeds Discover) |
| `VAPOURFLY_RAWG_KEY` | [RAWG](https://rawg.io/apidocs) | RAWG genres, tags, ratings |
| `VAPOURFLY_STEAM_API_KEY` or `settings set steam_api_key` | [Steam Web API](https://steamcommunity.com/dev/apikey) (free, any domain works) | All owned-game names resolved in one request per scan |

The full data-source catalog, including what each source feeds, is in
[API sources](../reference/API_SOURCES.md).

## 2. Set them

Environment variables work for both CLI and GUI:

```bash
export VAPOURFLY_IGDB_CLIENT_ID="..."
export VAPOURFLY_IGDB_CLIENT_SECRET="..."
export VAPOURFLY_RAWG_KEY="..."
```

The Steam Web API key can also live in your Vapourfly config:

```bash
vapourfly settings set steam_api_key <your-key>
```

It stays on your machine and is masked in output. Environment variables
take precedence over the config file.

## 3. Verify

```bash
vapourfly doctor
```

```text
Credentials
-----------
IGDB:          not configured
RAWG:          not configured
Steam Web API: configured (instant name resolution)
```

(The capture is from a machine with only the Steam Web API key configured.)
Per-source detail, including cache coverage, comes from:

```bash
vapourfly sources status
```

```text
Source          Credentials     Last Success    Entries  Stale    Cached    
---------------------------------------------------------------------------
steam-store     not required    2026-08-13 10:52 720      720      yes       
igdb            missing         n/a             0        0        no        
protondb        not required    2026-08-13 10:30 865      865      yes       
pcgw            not required    2026-08-13 11:26 705      0        yes       
hltb            not required    2026-08-13 11:50 612      0        yes       
rawg            missing         n/a             0        0        no        
```

`missing` means the credential is absent -- the source contributes nothing
until configured. `not required` sources work as-is.

## 4. Populate the cache

```bash
vapourfly cache refresh --source igdb
vapourfly cache refresh --source all
```

Two things the output makes easy to miss, both observed in real runs:

- **Missing credentials do not error.** With IGDB unset, the refresh
  completes with `Network fetches: 0` and `0 refreshed`. Check
  `sources status` afterwards instead of trusting the exit code.
- **Offline mode refuses to refresh.** `cache refresh --source igdb
  --offline` exits with `Error: Cannot refresh cache in offline mode.`

## What breaks without credentials

Features degrade instead of failing. The clearest example is Discover,
which needs IGDB similar-game data -- without it the generation succeeds
but finds nothing:

```bash
vapourfly playlist discover --seed 10 --count 5
```

```text
Generated Discover playlist: Discover: Counter-Strike
  ID: discover-10
  Games: 0
```

Configure IGDB, run `cache refresh --source igdb`, and the same command
produces real picks.

## Next steps

- Every source, its endpoints, and cache behavior:
  [API sources](../reference/API_SOURCES.md)
- How cached (including stale) data is used:
  [How library hydration works](../explanation/hydration-model.md)
