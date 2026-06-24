# Contributing to Vapourfly

Thank you for your interest in contributing to Vapourfly.

## Clean-Room Policy

Vapourfly is a MIT/Apache-2.0 licensed project. **Do not copy code from GPL-licensed projects.** This includes (but is not limited to) Depressurizer, SteamTools, or any other GPL-licensed Steam library managers.

If you have previously read GPL-licensed source code for similar functionality, you must disclose this before contributing related code. We may ask you to implement features through a clean-room process: one person describes the behavior (without sharing code), and another person implements it from that description alone.

This policy exists to protect the project's license integrity. Violations will result in rejected PRs and may lead to a contribution ban.

## Development Setup

Vapourfly targets **Rust 2024 edition** with **MSRV 1.88**.

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

## Phase-Gate Workflow

Development is organized into phases. Each phase has a defined scope and must be completed before the next phase begins.

- **Phase 0**: Project scaffolding, CI, documentation, test infrastructure.
- **Phase 1**: Steam file parsing (VDF, shortcuts, collections).
- **Phase 2**: Collection management (create, rename, delete, assign games).
- **Phase 3**: Discovery and metadata (Steam store API, local metadata cache).
- **Phase 4**: Advanced operations (bulk rename, tagging, import/export).

To contribute to a specific phase:

1. Check the project board for open issues tagged with the current phase.
2. Pick an unassigned issue and request assignment.
3. Create a feature branch from `main` named `phase-N/short-description`.
4. Implement, test, and open a PR against `main`.

PRs for work outside the current phase will be tagged `future-phase` and deferred.

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
- Use `#[must_error]` on fallible functions that callers must handle.

## Reporting Issues

Use GitHub Issues for bug reports and feature requests. Include:

- Steps to reproduce (for bugs).
- Expected vs. actual behavior.
- OS and Rust version.
- Relevant log output (redact any Steam credentials or personal paths).

## License

By contributing, you agree that your contributions will be dual-licensed under MIT and Apache-2.0.
