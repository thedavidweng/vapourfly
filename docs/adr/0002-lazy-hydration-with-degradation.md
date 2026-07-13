# Lazy hydration with graceful degradation in workflows

## Status

Accepted and implemented.

## Decision

Workflow commands (junk, recommend, playlist match, discover, editorial mood)
hydrate external metadata with lazy network fetch by default — when a cache
entry is missing, the workflow fetches it on demand via
`vapourfly_api::workflow::prepare` → `enrich_games` + `hydrate_from_cache`.
`--offline` (CLI) / offline mode (GUI) is the only way to force cache-only
behaviour.

A per-game fetch failure degrades gracefully: that game is evaluated with
whatever data is available, and the workflow never fails overall. The contract
is "always produce a result; missing data just means fewer reason codes" —
mirroring the Spotify-like experience where you always get a recommendation.

## Consequences

- Workflows may be slow on large libraries with cold caches and may hit API
  rate limits.
- Explicit `cache refresh` / `scan --enrich` remain available as bulk populate
  paths; they are not required before evaluation when online.
