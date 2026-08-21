# How Library Hydration Works

Vapourfly commands feel instant even on a large library, and they still
work with the network cable pulled. Both properties come from the same
design decision: local data first, cached metadata second, network last.
This page explains that layering and what it means for what you see.
The architecture decision behind it is
[ADR-0009](../adr/0009-instant-first-paint-hydration.md).

## Three layers, in order

**1. The local scan (always, instantly).**
Every workflow starts by reading files inside your Steam directory:
appmanifests for install state, playtime, and hidden state; cloud storage
for collections. This is fast because it is local disk -- hundreds of
games scan in well under a second -- and it is why `scan` output looks the
same online and off.

**2. Cache hydration (stale entries welcome).**
Workflow commands (`junk`, `recommend`, playlists, dynamic templates,
moods) then merge in external metadata from the local cache: ProtonDB
tiers, HLTB times, ratings, prices, similar-game vectors. Entries are used
even when stale -- a six-week-old HLTB time still beats no HLTB time. This
is the step that resolves game names when a name source is available.

**3. Background populate (network, optional).**
Gaps are filled into the cache by explicit or background fetches:
`scan --enrich`, `cache refresh --source <s>`, or the GUI's post-scan
populate job. Fetch failures degrade gracefully -- a failed IGDB lookup
costs you genres and similar games, not the command.

## What this means for what you see

**Placeholder names are a first-run state, not a bug.** A fresh machine
has an empty cache, so names may appear as `App <id>` until hydration or
a Steam Store refresh fills them:

```text
Warnings:
  [unresolved_names] 865 games have placeholder names (no local name source); names backfill from Steam Store hydration when online
```

With a Steam Web API key configured, names resolve in one bounded request;
without one they backfill progressively from Steam Store hydration.

**Missing data narrows results instead of breaking them.** A junk rule on
ratings cannot match a game with no rating; recommendations from an empty
cache return fewer picks; Discover without IGDB similar-game data returns
zero games. Each case is visible in the output rather than silently
skipped.

**`--offline` freezes layer 3.** It prohibits every network call --
including the name-map request and on-demand price lookups -- and blocks
`cache refresh`. Layers 1 and 2 run unchanged, so offline behavior is
exactly "whatever the cache knows".

## Where the cache lives

`vapourfly doctor` prints the cache root. Its size and coverage per source
are visible in `vapourfly sources status`. Refresh individual sources as
described in [Configure API credentials](../how-to/configure-api-credentials.md).

## For developers

Workflow commands call `vapourfly_api::workflow::prepare`, then re-run
junk classification with the desired mode if it differs from Default.
The current end-to-end data flow is kept current in
[FEATURES.md](../reference/FEATURES.md#current-data-flow).
