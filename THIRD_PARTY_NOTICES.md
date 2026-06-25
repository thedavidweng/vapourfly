# Third-Party Notices

This project includes reference materials from the following third-party sources under `reference/`. These files are used for development reference only and are **not** incorporated into Vapourfly releases. No code from these reference projects is copied, translated, or linked into Vapourfly binaries.

## Acknowledgments

Vapourfly's design was informed by studying the following open-source projects. We gratefully acknowledge their authors for the ideas, format documentation, and architectural inspiration they provided:

### Depressurizer

Source: [Depressurizer](https://github.com/rallion/depressurizer)
License: GPLv3
Reference files: VDF parser, GameData, GameDB, AutoCat, Profile, URL constants
Used for: Understanding Text/Binary VDF formats, Steam collection model, Steam Store scraping patterns, SteamID conversion logic

### Gameloop.Vdf

Source: [Gameloop.Vdf](https://github.com/BeyondDimension/Gameloop.Vdf)
License: MIT
Reference files: Text VDF tokenizer, parser, writer, format constants
Used for: Understanding Text VDF token-level parsing (quotes, escapes, comments, conditional tokens)

### SteamTools / BD.SteamClient (Watt Toolkit)

Source: [SteamTools / BD.SteamClient](https://github.com/BeyondDimension/SteamClient)
License: See repository for current license
Reference files: VDF parsers, Steam path detection, SteamApp model, appinfo.vdf parsing, Steamworks IPC
Used for: Cross-platform Steam directory detection, appinfo.vdf binary format, librarycache data layout

### TinyWiiBackupManager

Source: [TinyWiiBackupManager](https://github.com/mq1/TinyWiiBackupManager)
License: GPL-3.0
Reference files: Rust workspace structure, game model, HTTP client, UI patterns
Used for: Rust workspace architecture patterns (core/gui separation), egui state management, ureq HTTP client patterns, portable data directory design

## Reference vs. Production

All reference code resides exclusively under `reference/`. Vapourfly's production code in `crates/` is independently written in Rust. The reference materials served as:

- Format specification documents (VDF syntax, appinfo binary layout, cloud-storage JSON schema)
- Behavioral reference (how Steam reads/writes collections, path conventions, error handling patterns)
- Architectural inspiration (workspace layout, separation of concerns)

No GPL-licensed code is compiled into, linked against, or distributed with Vapourfly.
