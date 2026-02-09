# List all recipes
default:
	just --list --unsorted

# cargo compile
cargo-compile:
    cargo test --workspace --no-run --locked

# Clippy check
cargo-clippy-check:
    cargo clippy --no-deps --workspace --locked --tests --benches --examples -- -Dwarnings

# Rustfmt check
cargo-fmt-check:
    cargo fmt --all --check

# Run example
example:
	cargo run --example leaderboard

# Open
open:
	xdg-open http://localhost:8080
