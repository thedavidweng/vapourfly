# Test Fixtures

Sanitized Steam directory structures for Vapourfly tests. All personal data has been replaced with fake values.

## Sanitization Constants

| Field | Value |
|---|---|
| SteamID64 | `76561198000000000` |
| Account name | `vapourfly_fixture_user` |
| Persona name | `Vapourfly Fixture` |
| Paths | Fixture-relative (`./`) |

## Fixture: `steam_minimal/`

A minimal but complete Steam directory with representative data.

**What it tests:**
- Parsing `loginusers.vdf` to discover the active user
- Parsing `libraryfolders.vdf` to find library folders
- Reading playtime data from `localconfig.vdf` (apps 730, 223850, 999)
- Parsing cloud-storage collections JSON (favorites, hidden, deleted, tag-based)
- Reading `sc-version` key from cloud storage
- Parsing ACF app manifests for installed games
- Library cache name lookups

**AppIDs present:**
- `730` -- Counter-Strike 2 (significant playtime, in favorites + Indie tag)
- `223850` -- Factorio (significant playtime, in favorites)
- `999` -- Low playtime (5 min, LastPlayed=0) -- junk/low-playtime filter candidate, also in Indie tag

**Cloud storage entries:**
- `user-collections.favorite` -- has 730 and 223850 in `added`
- `user-collections.hidden` -- empty collection
- `user-collections.deleted-one` -- `is_deleted: true` entry (should be skipped by parser)
- `sc-version` -- metadata key, not a collection
- `user-collections.from-tag-Indie` -- tag-derived collection with 730 and 999

## Fixture: `empty_cloudstorage/`

Same directory structure as `steam_minimal/` but `cloud-storage-namespace-1.json` contains an empty JSON array `[]`.

**What it tests:**
- Graceful handling of a user with no cloud-synced collections
- Parser should return an empty collections map, not error

## Fixture: `malformed_cloudstorage/`

Same directory structure as `steam_minimal/` but `cloud-storage-namespace-1.json` contains invalid JSON (`{invalid json`).

**What it tests:**
- Error handling when cloud storage file is corrupted
- Parser should surface a parse error, not crash
- Downstream code should handle the error gracefully
