# Documentation Changelog

## 2026-06-24 v0.2开工版

- Unified GUI stack to `egui/eframe`.
- Moved roadmap to CLI-first.
- Defined `cloud-storage-namespace-1.json` as the only Steam collection write target.
- Defined `localconfig.vdf` as read-only playtime/per-app data source.
- Added `CloudEntry`, `CollectionValue`, hidden collection, write plan, backup and rollback details.
- Added complete IGDB plan: Twitch OAuth, token cache, headers, rate limits, Steam AppID mapping, game details and time-to-beat.
- Added RAWG, ProtonDB, PCGamingWiki and HLTB fallback strategies.
- Added Junk signal model and conservative missing-data behavior.
- Added CLI command surface.
- Updated dependency plan and eframe/egui version.
- Added license/clean-room policy and implementation gates.

## 2026-06-24 - Coding agent execution plan

- Added `CODING_AGENT_EXECUTION_PLAN.md` as the implementation authority for phase order, acceptance stamps, stop conditions, release hardening, and publish-level gates.
