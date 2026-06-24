# Vapourfly

A local-first CLI tool for managing Steam game libraries like Spotify playlists.

Vapourfly helps you organize, categorize, and curate your Steam library from the command line. Define collections with expressive queries, bulk-rename or tag games, and keep your library tidy -- all without touching Steam's UI.

## Status

CLI-first. Early development. Expect breaking changes until v1.0.

## Supported Platforms

- macOS
- Linux
- Windows

## Safety Model

Vapourfly modifies your Steam configuration files. To protect your data:

- **All write operations default to dry-run.** Use `--dry-run` to preview changes without modifying files.
- **Explicit confirmation required.** Use `--confirm` to apply changes, or set `VAPOURFLY_AUTO_CONFIRM=1` to skip prompts (not recommended).
- **Backups before writes.** Vapourfly creates timestamped backups of any file it modifies.
- **No Steam process interference.** Vapourfly will refuse to run if Steam is detected as actively writing to its config files.

## Installation

```bash
cargo install vapourfly
```

Or build from source:

```bash
git clone https://github.com/vapourfly/vapourfly.git
cd vapourfly
cargo build --release
```

## Usage

```bash
# List all games in your library
vapourfly list

# Show games matching a filter
vapourfly list --genre "RPG" --played

# Create a collection from a query
vapourfly collection create "Backlog RPGs" --genre "RPG" --unplayed

# Preview what a command would change
vapourfly collection add "Backlog RPGs" --game "Skyrim" --dry-run

# Apply the change
vapourfly collection add "Backlog RPGs" --game "Skyrim" --confirm
```

## License

Licensed under either of:

- MIT license ([LICENSE-MIT](LICENSE-MIT))
- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE))

at your option.

## Contributing

See [CONTRIBUTING.md](CONTRIBUTING.md) for guidelines.

## Security

See [SECURITY.md](SECURITY.md) for the vulnerability reporting policy.
