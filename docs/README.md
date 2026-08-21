# Vapourfly Documentation

Documentation is organized by
[Diátaxis](https://diataxis.fr/): tutorials for learning, how-to guides
for tasks, reference for lookup, explanation for understanding.
Start at the [top-level README](../README.md) for installation.

## Tutorials -- learn by doing

- [Getting started](tutorials/getting-started.md) -- verify your setup,
  read your library, and view collections in a guided first pass. No
  writes.

## How-to guides -- solve a specific problem

- [Purge junk from your library](how-to/purge-junk.md) -- find never-going-to-play games and tag or hide them
- [Plan a Deck session](how-to/plan-deck-session.md) -- get picks that fit the time you have; write them to Steam
- [Share and sync playlists](how-to/share-and-sync-playlists.md) -- curate, share as `VF1:` codes, sync to a Steam collection
- [Work offline](how-to/work-offline.md) -- run everything with zero network
- [Back up and restore Steam files](how-to/back-up-and-restore.md) -- recover from an unwanted write; tune retention
- [Configure API credentials](how-to/configure-api-credentials.md) -- unlock IGDB, RAWG, and instant name resolution

## Reference -- look something up

- [Command reference](reference/COMMANDS.md) -- every command, flag, and output format
- [Feature matrix](reference/FEATURES.md) -- current CLI/GUI capability contract, playlist JSON schema
- [Steam file safety](reference/STEAM_FILE_SAFETY.md) -- write targets, backups, atomicity
- [API sources](reference/API_SOURCES.md) -- each data source, credentials, caching
- [Privacy](reference/PRIVACY.md) -- local-first design, redaction, diagnostics contents

## Explanation -- understand the design

- [How junk classification works](explanation/junk-classification.md) -- signals, modes, confidence
- [How library hydration works](explanation/hydration-model.md) -- local scan → cache → background populate (ADR-0009)
- [Architecture decision records](adr/) -- the decisions themselves

## About the examples

Output blocks in the tutorials and guides were captured from real runs of
vapourfly 0.2.0 on macOS against an 865-game library. Where a capture
contained personal data (home paths, account names, SteamIDs), it is
redacted inline and marked; everything else is verbatim. Commands whose
output is not shown are either read-only with self-evident output or
confirmed writes that were deliberately not executed for the docs.

## For contributors

- [GUI smoke test checklist](gui-smoke-test.md)
- [Contributing guide](../CONTRIBUTING.md) -- keep FEATURES.md and COMMANDS.md current with user-facing changes
