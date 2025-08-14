# List all recipes
default:
	just --list --unsorted

# Run example
example:
	cargo run --example leaderboard

# Open
open:
	xdg-open http://localhost:8080
