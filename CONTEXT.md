# Vapourfly — Domain Glossary

This is the ubiquitous language for Vapourfly. It is a glossary only — no
implementation details, no specs, no decisions-in-progress. When a term is
resolved during design, it lands here immediately.

Architectural decisions that are hard to reverse, surprising without context,
and the result of a real trade-off live in `docs/adr/`.

## Status of this document

The original PRD (Python CLI, GUI as non-goal, writes to `localconfig.vdf`)
was **stale**. The Rust codebase is the source of truth. This glossary
describes the project as it actually is. The PRD has been rewritten as
[PRD.md](PRD.md) to match the codebase and the decisions recorded here.

## Core artifacts

### Playlist

A Vapourfly-owned, shareable JSON artifact (`vapourfly.playlist.v1`) with an
`id`, `name`, `description`, and `content`. Content is either **Manual** (an
explicit list of AppIDs) or **Rules** (a boolean expression over game
metadata). A Playlist is the source artifact that can be imported, exported,
shared as a share code, matched against a library, and synced to a Steam
Collection.

A Playlist is *not* a Steam Collection. It lives in Vapourfly's local playlist
store, not in Steam's files.

### Steam Collection

A named set of AppIDs that lives in Steam's cloud storage
(`userdata/<account>/config/cloudstorage/cloud-storage-namespace-1.json`), not
in `localconfig.vdf`. A Steam Collection is the **sync target** for a Playlist.
Steam also has a special hidden collection (`user-collections.hidden`), and
membership in it *is* how modern Steam hides a game from the library.

"Collection" never refers to a Vapourfly-side artifact. If something is on the
Vapourfly side, it is a Playlist.

**Write surface decision:** `cloud-storage-namespace-1.json` is the *only* file
Vapourfly will ever modify. `localconfig.vdf` is read-only forever (parsed for
playtime and per-app state, never written). Vapourfly stays within the PRD's
feature scope; editing per-app Steam settings (tags, launch options, controller
configs) is explicitly out of scope. See [ADR-0001](docs/adr/0001-cloud-storage-only-write-surface.md).

### Dynamic Template

A built-in, **transparent** playlist factory that compiles against the current
library to produce a Playlist. The user sees and can understand the rules
behind each template. The canonical templates are `deck-session` and
`finish-it`. `deck-session` emits a rule Playlist (Installed + NotHidden +
NotJunk + ProtonAtLeast Gold + ControllerSupportFull + HltbMaxMinutes);
`finish-it` evaluates the library and emits an explicit-AppID Playlist (games
whose playtime is 0.5–1.25× HLTB main story).

**`playlist-radio` is removed.** It was a strictly weaker version of
Discover-with-seed. Discover now owns the entire "seed-based similar picks"
surface. This is a breaking change: the `collections dynamic playlist-radio`
CLI command and the corresponding GUI entry are removed. See
[ADR-0005](docs/adr/0005-discover-absorbs-playlist-radio.md).

**`mood` is removed as a Dynamic Template.** It is replaced by Editorial Mood
(see below) — a fundamentally different concept. The old mood template was a
transparent tag/genre filter; Editorial Moods are named, curated playlists
with hidden selection criteria, like Spotify's editorial playlists. See
[ADR-0004](docs/adr/0004-editorial-mood-replaces-tag-filter.md).

A Dynamic Template is not itself a Playlist or a Collection. It is a generator.

### Editorial Mood

A named, curated playlist with **hidden selection criteria** — the user sees
the name and description, but not the rules behind it. This is the Spotify
editorial playlist model: "Today's Biggest Hits" or "Friday Party" are
evocative names; the underlying logic is Vapourfly's editorial judgment, not a
user-configured filter.

The seven canonical Editorial Moods, each backed by criteria computable from
Vapourfly's available data (Steam Store, IGDB, RAWG, ProtonDB, PCGW, HLTB,
local playtime). Names are canonical English; localized display names (e.g.
Chinese) are a UI/localization concern, not part of the domain model:

| Canonical name | Hidden criteria |
|---|---|
| Today's Biggest Hits | Owned games with a recent popularity surge — on sale (discount_percent > 0) and/or rising current player count and/or rising recent review activity. Helps the user find owned games with active player communities right now (e.g. a Battlefield title that just went on sale). May require fetching current player count data not yet cached. |
| Indie Rising | Indie (IGDB theme / Steam Store type) + high rating + recent |
| Friday Party | Steam Store categories: Co-op / Local Multiplayer / Party |
| Deck Guardians | ProtonDB Platinum/Gold + full controller + short HLTB |
| Unopened Treasures | Unplayed + high rating + not junk |
| Weekend Marathon | Unplayed + long HLTB + high rating |
| Quick Round | Unplayed + short HLTB + not junk |

An Editorial Mood compiles to a Playlist (manual AppID list, evaluated against
the current library). It is a generator, like a Dynamic Template, but the
criteria are opaque to the user. The old `collections dynamic mood` CLI command
is replaced by `collections mood <name>` (or equivalent), listing available
moods and compiling the selected one.

### Discover

A playlist factory that produces a Playlist from taste similarity, optionally
seeded by an AppID. Builds a taste vector from the user's high-playtime games
(genre/theme/keyword overlap, log-scaled by playtime), filters to unplayed
non-junk non-hidden games (excluding Application/Tool/DLC), and scores by:
seed IGDB-similar membership (+5.0) + taste overlap (normalized) + high rating
(+0.25). Output is a Manual Playlist with `created_by: "vapourfly"`.

Discover owns the entire "similar picks" surface — there is no separate
Playlist Radio concept. Same shape as a Dynamic Template: a generator, not an
artifact.

### Generator playlist slot

A fixed playlist-store identity used by the GUI for the latest output of a
generator (Discover, a Dynamic Template, or an Editorial Mood). Regenerating
overwrites that slot rather than creating a new Playlist id each time. The
slot is a presentation/store convenience for “latest generation”; it is not a
separate domain artifact type. See [ADR-0007](docs/adr/0007-generator-playlist-slots.md).

## Library and games

### Game

A single Steam application **owned by the user** as Vapourfly models it:
AppID, name, type, install state, playtime, Steam collection membership,
hidden flag, junk flag, and optional enriched external data (HLTB, IGDB,
RAWG, ProtonDB, PCGW, Steam Store).

**Unowned games are not first-class entities.** Vapourfly's scope is the
user's own library. Games the user does not own appear only in the playlist
match context (match report shows missing games and their Steam Store prices
for completion cost calculation). Recommendations, Discover, and Editorial
Moods only operate on owned games. A toggle to show/hide owned games in
arbitrary views is **not** in scope — unowned games are visible only in the
playlist match report.

### Junk

A classification applied to a Game indicating the user is unlikely to play it.
Evaluated from three signals — **playtime**, **completion time** (HLTB),
**rating** (RAWG/IGDB) — under one of three modes:

- **Default**: low playtime + at least one other low signal + at least
  `min_available_signals` data points.
- **Strict**: low playtime + every *available* signal low + at least
  `min_available_signals` data points. Playtime is the one first-party
  signal, so junk always requires it to be present and low; Strict is a
  strict subset of Default.
- **Aggressive**: low playtime + at least one other low signal, no minimum
  data count.

Default is the canonical mode. The PRD's original "hard AND of all three
signals" model is **dropped** — it fails to classify most games because HLTB
and RAWG coverage is incomplete. Manual overrides (`force_include`,
`force_exclude`, manual HLTB, manual rating) take precedence over signals.
A Junk decision is explainable: it carries matched signals, missing signals,
and a confidence score (fraction of signals available). A Game's junk flag is
a derived, explainable decision — not a permanent property of the game itself.

### Recommendation

A scored suggestion of a Game to play next, produced by the recommendation
engine from the current library. Seven weighted signals combine additively:

- `low_playtime` (+2.0): playtime under 120 min.
- `deck_compatible` (Native +2.0 / Platinum +1.5 / Gold +1.0, deck mode only).
- `time_match` (+1.5): HLTB main story fits the requested session length.
- `high_rating` (+1.0): RAWG ≥ 4.0 or IGDB ≥ 80.
- `taste_similarity` (+1.0): keyword overlap with the user's taste vector > 5%.
- `recently_played_penalty` (−1.0): played within 14 days.
- `likely_finished_penalty` (−0.5): playtime exceeds 1.5× HLTB main story.

Junk and hidden games are filtered out before scoring, not penalised. Optional
seed makes selection deterministic via SplitMix64 perturbation.

**Weights are fixed** — internal constants, not user-tunable. Users control
only external parameters (available minutes, count, deck mode, seed, excluded
collections). The contract is "Vapourfly knows how to score; you pick the
session shape." A Recommendation carries human-readable reason codes so the
user can see *why* each game scored the way it did.

### Hydration

The process of loading external metadata (HLTB, IGDB, RAWG, ProtonDB, PCGW,
Steam Store) onto Game records before a workflow evaluates them. Two phases
exist in the code:

- **Populate** — `scan --enrich` or `cache refresh --source <src>` fetches from
  the network and writes to the local disk cache (`enrich_games`).
- **Hydrate** — apply cache entries (including stale) onto Game records
  (`hydrate_from_cache`).

**Default (ADR-0002):** workflows (junk / recommend / playlist / discover /
dynamic / editorial mood) go through `workflow::prepare`, which **lazy-fetches
missing cache entries over the network** unless `--offline` / offline mode is
set. Offline is the only way to force cache-only behaviour.

Rationale: recommendations and junk decisions should always use the freshest
data; requiring the user to remember a separate refresh step produces silently
stale results. Trade-off: workflows may be slow on large libraries with cold
caches, and may hit API rate limits. See
[ADR-0002](docs/adr/0002-lazy-hydration-with-degradation.md).

**Failure semantics:** a per-game fetch failure degrades gracefully — that
game is evaluated with whatever data is available (equivalent to cache-only
for that game). A workflow **never** fails overall because an external API
failed. The contract is "always produce a result; missing data just means
fewer reason codes." This mirrors the Spotify-like experience: you always get
a recommendation, it just may be less informed.

### Share Code

A compact, copy-pasteable string encoding a Playlist for sharing. Format:
`VF1:<compressed-binary-payload>`. The payload carries the playlist's
`content` (manual AppID list or rules tree) plus `name` and `description`,
encoded as a compact binary format with compression — not the previous
base64url(JSON).

**No backward compatibility.** The previous `VF1:<base64url(JSON)>` format is
replaced outright. Existing VF1 codes will fail to decode under the new
decoder. Accepted because v0.1.0 has few users. The `VF1:` prefix is retained
(the `1` is now the format generation, not the JSON-encoding version). See
[ADR-0003](docs/adr/0003-compact-binary-share-codes.md).

A Share Code decodes back into a PlaylistFile that can be imported, matched,
and synced like any other playlist.
