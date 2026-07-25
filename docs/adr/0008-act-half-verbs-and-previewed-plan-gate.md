# Act-half workflow verbs and the PreviewedPlan gate

## Status

Accepted and implemented.

## Decision

The act half of every Steam-write workflow — evaluate → disposition → cloud
read → preview — lives behind one verb per workflow in `vapourfly_core::actions`
(`preview_junk_apply`, `preview_junk_hide`, `preview_recommend_collection`,
`preview_playlist_sync`), mirroring how `vapourfly_api::workflow::prepare`
owns the read half. The rule-Playlist → owned-AppID resolution for sync is
owned by the sync verb, not by frontends. The two-pass Playlist match with
missing-entry store details is `vapourfly_api::workflow::match_playlist_full`
(it needs cache + network, so it lives in `api`; the other verbs are pure
core). CLI and GUI call the verbs; they no longer assemble
`disposition` + `read_cloud_storage` + `write::preview` themselves.

The confirmation gate is enforced in the write module's interface:
`write::preview` returns a `PreviewedPlan` — a newtype with no public
constructor — and `write::commit` / `commit_with_retention` accept only that
type. A commit that skipped preview does not compile. Showing the diff and
obtaining consent remains each frontend's half (CLI `--dry-run`/`--confirm`
flags, GUI confirm dialog); the GUI's legacy path that could commit a
re-derived junk plan whose diff was never displayed is removed — only
backup restore (which has no plan diff) executes without a stored
`PreviewedPlan`.

Enrichment sources sit behind one adapter seam: `enrich_source` binds a
per-source adapter (cache key + TTL + Game field + fetch) and a single
generic state-machine runs the cache/offline/fetch/stale-fallback protocol
for all six sources. Cache key derivation is owned by the enrichment module
(writers and `hydrate_from_cache` share it); credentials are resolved once
at the seam (`SourceCredentials`); `HttpClient` is cheaply cloneable
(shared backend + rate limiter) so one client — real or mock — serves every
source.

## Consequences

- Workflow semantics (which WriteOp, rule resolution, preview-then-commit
  ordering) change in one place and are tested at the verb interface;
  frontend tests shrink to presentation concerns.
- `workflow.rs` and all six enrichment wirings are covered by tests through
  injected HTTP/credentials/cache-root seams.
- Generator playlist slots stay GUI-owned per ADR-0007 — the verbs cover
  Steam writes and matching, not slot persistence.
- The `Ok(None)` fetch outcome ("source has no data") is uniformly counted
  as skipped for every source; previously only HLTB/RAWG/IGDB did this.
