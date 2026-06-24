fmt-check:
    cargo fmt --all -- --check

clippy:
    cargo clippy --workspace --all-targets -- -D warnings

test:
    cargo test --workspace

check-all: fmt-check clippy test

fixture-sain:
    cargo run -p vapourfly-cli -- doctor --fixtures data/fixtures/steam_minimal

release-check: check-all
    cargo deny check
