#!/usr/bin/env bash
# Post-change verification script
# All steps must pass without warnings
# Keep in sync with verify.ps1
#
# Note: llm-coding-tools-rig and llm-coding-tools-serdesai are async-only (implement async Tool traits).
# The blocking feature only applies to llm-coding-tools-core.

set -e

ORIGINAL_DIR="$(pwd)"
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
cd "$PROJECT_ROOT"

trap 'cd "$ORIGINAL_DIR"' EXIT

echo "Building..."
cargo build -p llm-coding-tools-core --quiet
cargo build -p llm-coding-tools-subagents --quiet
cargo build -p llm-coding-tools-rig --quiet
cargo build -p llm-coding-tools-serdesai --quiet

echo "Testing..."
cargo test -p llm-coding-tools-core --quiet
cargo test -p llm-coding-tools-subagents --quiet
cargo test -p llm-coding-tools-rig --quiet
cargo test -p llm-coding-tools-serdesai --quiet

echo "Clippy..."
cargo clippy -p llm-coding-tools-core --quiet -- -D warnings
cargo clippy -p llm-coding-tools-subagents --quiet -- -D warnings
cargo clippy -p llm-coding-tools-rig --quiet -- -D warnings
cargo clippy -p llm-coding-tools-serdesai --quiet -- -D warnings

echo "Testing blocking feature..."
cargo test -p llm-coding-tools-core --no-default-features --features blocking --quiet

echo "Docs..."
RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --quiet

echo "Formatting..."
cargo fmt --all --quiet

echo "Publish dry-run..."
cargo publish --dry-run -p llm-coding-tools-core --quiet
cargo publish --dry-run -p llm-coding-tools-subagents --quiet
cargo publish --dry-run -p llm-coding-tools-rig --quiet
cargo publish --dry-run -p llm-coding-tools-serdesai --quiet

echo "All checks passed!"
