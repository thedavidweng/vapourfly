# Vapourfly — Domain Glossary

This is the ubiquitous language for Vapourfly. It is a glossary only — no
implementation details, no specs, no decisions-in-progress. When a term is
resolved during design, it lands here immediately.

Architectural decisions that are hard to reverse, surprising without context,
and the result of a real trade-off live in `docs/adr/`.

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
playtime and per-app state, never written). Editing per-app Steam settings
(tags, launch options, controller configs) is explicitly out of scope. See
[ADR-0001](docs/adr/0001-cloud-storage-only-write-surface.md).

### Dynamic Template

A built-in, **transparent** playlist factory that compiles against the current
library to produce a Playlist. The user sees and can understand the rules
behind each template. The canonical templates are `deck-session` and
`finish-it`. `deck-session` emits a rule Playlist (Installed + NotHidden +
NotJunk + ProtonAtLeast Gold + ControllerSupportFull + HltbMaxMinutes);
`finish-it` evaluates the library and emits an explicit-AppID Playlist (games
whose playtime is 0.5–1.25× the effective completion time).

A Dynamic Template is not itself a Playlist or a Collection. It is a generator.

### Editorial Mood

A named, curated playlist with **hidden selection criteria** — the user sees
the name and description, but not the rules behind it. This is the Spotify
editorial playlist model: "Today's Biggest Hits" or "Friday Party" are
evocative names; the underlying logic is Vapourfly's editorial judgment, not a
user-configured filter.

The seven canonical Editorial Moods, each backed by criteria computable from
Vapourfly's available data (Steam Store, IGDB, RAWG, ProtonDB, PCGW,
legacy HLTB cache, local playtime). Names are canonical English; localized display names (e.g.
Chinese) are a UI/localization concern, not part of the domain model:

| Canonical name | Hidden criteria |
|---|---|
| Today's Biggest Hits | Owned games currently on sale (`discount_percent > 0`). Games without Steam Store price data are excluded. |
| Indie Rising | Indie (IGDB theme / Steam Store type) + high rating + recent |
| Friday Party | Steam Store categories: Co-op / Local Multiplayer / Party |
| Deck Guardians | ProtonDB Platinum/Gold + full controller + short completion time |
| Unopened Treasures | Unplayed + high rating + not junk |
| Weekend Marathon | Unplayed + long completion time + high rating |
| Quick Round | Unplayed + short completion time + not junk |

An Editorial Mood compiles to a Playlist (manual AppID list, evaluated against
the current library). It is a generator, like a Dynamic Template, but the
criteria are opaque to the user. The CLI surface is `collections mood [name]`.
See [ADR-0004](docs/adr/0004-editorial-mood-replaces-tag-filter.md).

### Discover

A playlist factory that produces a Playlist from taste similarity, optionally
seeded by an AppID. Filters to unplayed non-junk non-hidden games (excluding
Application/Tool/DLC) and ranks them with the shared canonical Taste Profile
attribution; see "Personal Discover ranking". Output is a Manual Playlist with
`created_by: "vapourfly"`.

Discover owns the entire "similar picks" surface. Same shape as a Dynamic
Template: a generator, not an artifact. See
[ADR-0005](docs/adr/0005-discover-absorbs-playlist-radio.md).

### Generator playlist slot

A fixed playlist-store identity used by the GUI for the latest output of a
generator (Discover, a Dynamic Template, or an Editorial Mood). Regenerating
overwrites that slot rather than creating a new Playlist id each time. The
slot is a presentation/store convenience for “latest generation”; it is not a
separate domain artifact type. See [ADR-0007](docs/adr/0007-generator-playlist-slots.md).

### Taste Profile / Top Tags

A derived, account-scoped summary of long-term and recent engagement with
normalized game genres and descriptors, supported by cached/local activity and
explicit Interested preferences. Top Tags are its inspectable presentation.
Strength describes weighted engagement within this library; library-relative
lift compares it to this library's descriptor coverage, never to other players.
A Taste Profile is not a separate personal database or a Playlist ranker.

### Player-style traits

Original descriptive rules derived from the same eligible account evidence as
Top Tags plus existing local story marks and fresh cached achievement counts.
Completion-focused, Deep diver and Explorer are transparent threshold labels,
not personality scores or comparisons with other players. Every rule exposes
supporting, contrary and unknown titles; an absent story mark is unknown, not
proof of an unfinished story. Ownership alone never supports a trait.

## Library and games

### Game

A single Steam application **owned by the user** as Vapourfly models it:
AppID, name, type, install state, playtime, Steam collection membership,
hidden flag, junk flag, and optional enriched external data (completion time,
IGDB, RAWG, ProtonDB, PCGW, Steam Store). Existing HLTB values are legacy
cache records and retain their original provenance.

**Unowned games are not first-class entities.** Vapourfly's scope is the
user's own library. Games the user does not own appear only in the playlist
match context (match report shows missing games and their Steam Store prices
for completion cost calculation). Recommendations, Discover, and Editorial
Moods only operate on owned games. A toggle to show/hide owned games in
arbitrary views is **not** in scope — unowned games are visible only in the
playlist match report.

### Junk

A classification applied to a Game indicating the user is unlikely to play it.
Evaluated from three signals — **playtime**, **effective completion time**,
**rating** (RAWG/IGDB) — under one of three modes:

- **Default**: low playtime + at least one other low signal + at least
  `min_available_signals` data points.
- **Strict**: low playtime + every *available* signal low + at least
  `min_available_signals` data points. Playtime is the one first-party
  signal, so junk always requires it to be present and low; Strict is a
  strict subset of Default.
- **Aggressive**: low playtime + at least one other low signal, no minimum
  data count.

Default is the canonical mode. A hard AND of all three signals is not used —
it fails to classify most games because completion-time and RAWG coverage is
incomplete. Manual overrides (`force_include`, `force_exclude`, manual
completion time, manual rating)
take precedence over signals. A Junk decision is explainable: it carries
matched signals, missing signals, and a confidence score (fraction of signals
available). A Game's junk flag is a derived, explainable decision — not a
permanent property of the game itself.

### Recommendation

A scored suggestion of a Game to play next, produced by the recommendation
engine from the current selected-account library and cache-only canonical Taste
Profile. Signed, bounded contributions are shared by CLI and GUI:

- `low_playtime` (+2): known lifetime playtime under 120 minutes; unknown is not zero.
- `deck_compatible` (Native +2 / Platinum +1.5 / Gold +1, Deck mode only).
- `time_match` (+1.5): legacy cached whole-story estimate fits the window; this
  is not remaining time, session shape, or proof of provider rights (#27/#36).
- `high_rating` (0..1): continuous above-midpoint maximum of normalized cached RAWG/IGDB quality; low and missing ratings are neutral.
- `taste_similarity` (0..3) and `recent_taste` (0..0.5): fraction of canonical
  profile mass covered by a candidate's cached concepts, not a binary bonus.
- `recently_played_penalty` (-1..0): linearly decays over the last 14 days;
  future timestamps are not recent play.
- `story_completed` (-1.25) or fresh authoritative `perfected` (-1), otherwise
  `achievement_progress` (0..0.5) for fresh partial counts. Neither achievements
  nor HLTB/playtime infer narrative completion.
- `local_interested` (+0.5) and explicit Steam `favorite` (+0.25).
- `diversity` (-0.35..0): maximum canonical-concept Jaccard overlap with any
  earlier pick, greedily reranked after eligibility and relevance scoring.

Unknown activity/progress and cache coverage limitations carry zero weight.
Hidden/junk/non-game, installed-only, excluded collections and active Not
interested/Not now are filtered first. Existing Deck scoring is retained;
Recommendations do not add a new hard controller filter. The optional seed
changes ties only, keyed by AppID, so input ordering cannot affect selection.
Relative fit compares this result set, not probability or population percentiles.

**Weights are fixed** — internal constants, not user-tunable. Users control
only external parameters (available minutes, count, deck mode, seed, excluded
collections). The contract is "Vapourfly knows how to score; you pick the
session shape." A Recommendation carries human-readable reason codes so the
user can see *why* each game scored the way it did.

### Hydration

The process of loading external metadata (IGDB, RAWG, ProtonDB, PCGW, Steam
Store, and legacy HLTB cache records) onto Game records before a workflow
evaluates them. Two phases
exist in the code:

- **Populate** — fetches from the network and writes to the local disk cache
  (`enrich_games`). Runs as the GUI's automatic background job after first
  paint, or explicitly via `scan --enrich` / `cache refresh --source <src>`.
- **Hydrate** — apply cache entries (including stale) onto Game records
  (`hydrate_from_cache`).

**Default (ADR-0009):** workflows (junk / recommend / playlist / discover /
dynamic / editorial mood) and the GUI library scan go through
`workflow::prepare`, which is **cache-only plus bounded fetches** — it never
does bulk network work, so results render in seconds at any library size. The
bounded fetches are the owned-games name map (one request, only with a
configured Steam Web API key) and missing Playlist entry prices during match.
Freshness comes from TTLs plus background/explicit repopulation, not from
fetch-at-evaluation. `--offline` / offline mode still forces zero network,
including the bounded fetches. See
[ADR-0009](docs/adr/0009-instant-first-paint-hydration.md).

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
encoded as a compact binary format with compression.

The previous `VF1:<base64url(JSON)>` format is not accepted. The `VF1:`
prefix is retained; the `1` is the format generation. See
[ADR-0003](docs/adr/0003-compact-binary-share-codes.md).

A Share Code decodes back into a PlaylistFile that can be imported, matched,
and synced like any other playlist.

### Personal Discover ranking

Discover shares canonical personal attribution and deterministic diversity with
Recommendations, but owns its existing owned/unplayed eligibility and Manual
Playlist factory. It does not consume a session duration or Deck planner request.
Optional seed relatedness is +0.75 only for two confirmed cached IGDB Steam
mappings linked by the seed's similar IDs; missing seed evidence stays neutral.
Known-lifetime novelty is bounded by +0.25, unknown activity is not zero. Keep
eligible cold-start picks even at non-positive scores. CLI/GUI expose all signed
reasons; story/preference changes invalidate late results without rewriting saved
playlists. Existing generator slots, share format and Steam guards are unchanged.
