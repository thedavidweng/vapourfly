# Instant-first-paint hydration: cache-only read path, background populate

## Status

Accepted and implemented. **Supersedes ADR-0002's lazy network fetch.**

## Context

ADR-0002 made workflows lazy-fetch missing cache entries over the network.
On a real 865-game library with a cold cache, that meant the GUI's first
library render waited ~86 minutes behind rate-limited enrichment, repeated
daily by 1-day TTLs. Product decision: first paint must land in seconds;
anything that would block it becomes optional or runs in the background.

## Decision

- **The read path never does bulk network work.** `workflow::prepare` is
  scan → bounded name resolution → cache-only hydration (stale entries
  included) → junk classification. Wall-clock is seconds at any library
  size and cache state, verified by a contract test that runs `prepare`
  online with zero network available.
- **Bounded fetches stay on demand**: at most one `GetOwnedGames` request
  for the owned-games name map (only with a configured
  `VAPOURFLY_STEAM_API_KEY`, cache-first, 1-day TTL), and per-missing-entry
  Steam Store prices during Playlist match. Each degrades gracefully.
- **Population is background or explicit.** The GUI auto-starts one
  background enrichment job per launch after the first scan renders
  (missing/stale entries only, visible in Data Sources); when it completes,
  the library re-hydrates and placeholder names/data fill in. The CLI
  populates via `cache refresh` (forced) and `scan --enrich`.
- **Freshness policy moves from fetch-time to TTL + background repopulate**:
  ProtonDB 7d, Steam Store 3d, PCGW/IGDB/RAWG 14d, HLTB 30d. Rate limits
  are per source (steam-store 30/min, protondb/pcgw 120/min, igdb 200/min,
  hltb 30/min, rawg 60/min), sized to each provider's real tolerance.
- **Name resolution chain** (macOS machines commonly lack `appinfo.vdf`):
  appmanifest → librarycache → appinfo.vdf → owned-games map (key-gated,
  instant) → Steam Store per-app backfill (progressive) → `"App <id>"`.
  Name-keyed sources (HLTB, RAWG) skip placeholder-named games.

## Consequences

- Evaluations may run on stale-but-cached data between background
  refreshes. ADR-0002's "always freshest data" rationale is traded away
  deliberately: fewer reason codes now beat an unusable first run.
- `--offline` keeps its strong meaning: no network anywhere, including the
  bounded fetches.
- Without a Steam Web API key, a first-ever launch shows real counts,
  playtime, and collections instantly but placeholder names until the
  background populate covers them (Steam Store pass ≈ 30 min on ~900
  games). With a key, names resolve in the first seconds.
- HLTB moved to its current `bleed` endpoints and is enabled by default;
  the old `/api/search`, PCGW `action=ask`, and Valve's keyless
  `GetAppList` are gone from the codebase (all removed by their providers).
