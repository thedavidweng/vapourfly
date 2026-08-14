# Contributing to Vapourfly

Thank you for your interest in contributing to Vapourfly.

## Clean-Room Policy

Vapourfly is a MIT/Apache-2.0 licensed project. **Do not copy code from GPL-licensed projects.** This includes (but is not limited to) Depressurizer, SteamTools, or any other GPL-licensed Steam library managers.

If you have previously read GPL-licensed source code for similar functionality, you must disclose this before contributing related code. We may ask you to implement features through a clean-room process: one person describes the behavior (without sharing code), and another person implements it from that description alone.

This policy exists to protect the project's license integrity. Violations will result in rejected PRs and may lead to a contribution ban.

## Development Setup

Vapourfly targets **Rust 2024 edition** with **MSRV 1.96**.

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
in [docs/adr/](docs/adr/). Update [docs/FEATURES.md](docs/FEATURES.md) and
[docs/CLI.md](docs/CLI.md) when a user-facing contract changes.

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

## License

By contributing, you agree that your contributions will be dual-licensed under MIT and Apache-2.0.
