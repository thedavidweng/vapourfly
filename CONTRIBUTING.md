# Contributing to Vapourfly

Thank you for your interest in contributing to Vapourfly.

## Clean-Room Policy

Vapourfly is licensed under AGPL-3.0-only, and the maintainer must be able to relicense every line of it (see [License and CLA](#license-and-cla)). **Do not copy code from GPL- or AGPL-licensed projects**, even though their licenses are compatible with Vapourfly's: third-party copyleft code cannot be relicensed. This includes (but is not limited to) Depressurizer, SteamTools, TinyWiiBackupManager, or any other GPL-licensed Steam library managers. Code under permissive licenses (MIT, Apache-2.0, BSD) is acceptable only with its license notice and a mention in the pull request.

If you have previously read GPL-licensed source code for similar functionality, you must disclose this before contributing related code. We may ask you to implement features through a clean-room process: one person describes the behavior (without sharing code), and another person implements it from that description alone.

This policy exists to protect the project's license integrity. Violations will result in rejected PRs and may lead to a contribution ban.

## Development Setup

Vapourfly targets **Rust 2024 edition** with **MSRV 1.99**.

```bash
# Install Rust (via rustup)
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh

# Verify toolchain
rustup show
cargo --version

# Build
cargo build

# Run tests
cargo test

# Run lints
cargo clippy -- -D warnings

# Format
cargo fmt
```

## Development Workflow

Keep changes small, current, and directly tied to user-visible behavior or a clear internal maintenance need.

Before opening a PR, run the checks that match the touched code:

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

HLTB scraping is on by default. When touching external API or cache code, also
check the build with the scrape client disabled:

```bash
cargo check -p vapourfly-api --no-default-features
```

Domain language lives in [CONTEXT.md](CONTEXT.md). Architecture decisions live
in [docs/adr/](docs/adr/). Update [docs/reference/FEATURES.md](docs/reference/FEATURES.md) and
[docs/reference/COMMANDS.md](docs/reference/COMMANDS.md) when a user-facing contract changes.

## Pull Request Guidelines

- One logical change per PR. Keep diffs small and focused.
- Include tests for new functionality.
- Update documentation if behavior changes.
- All CI checks must pass before merge.
- Use conventional commit messages: `feat:`, `fix:`, `docs:`, `test:`, `refactor:`, `chore:`.

## Code Style

- Follow standard Rust conventions (`cargo fmt`, `cargo clippy`).
- Prefer explicit types over inference in public API signatures.
- Document public items with `///` doc comments.
- Use `#[must_use]` when ignoring a returned value would be a likely bug.

## Reporting Issues

Use GitHub Issues for bug reports and feature requests. Include:

- Steps to reproduce (for bugs).
- Expected vs. actual behavior.
- OS and Rust version.
- Relevant log output (redact any Steam credentials or personal paths).

## License and CLA

Vapourfly is licensed under the [GNU AGPL v3.0 only](LICENSE). Before your first pull request can be merged, you must sign the [Contributor License Agreement](CLA.md). The CLA bot comments on your pull request with instructions; reply with the sentence it asks for. You sign once, and it covers all of your contributions.

The CLA lets the maintainer relicense contributions, including under other open source or commercial terms. You keep the copyright in your work.

The Vapourfly name and logo are covered by the [trademark policy](TRADEMARKS.md).
