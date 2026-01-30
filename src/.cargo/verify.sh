#!/usr/bin/env bash
# Post-change verification script
# All steps must pass without warnings
# Keep in sync with verify.ps1
#
# Note: llm-coding-tools-rig and llm-coding-tools-serdesai are async-only (implement async Tool traits).
# The blocking feature only applies to llm-coding-tools-core.

set -e

run_cmd() {
  echo "$*"
  "$@"
}

ORIGINAL_DIR="$(pwd)"
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
cd "$PROJECT_ROOT"

trap 'cd "$ORIGINAL_DIR"' EXIT

echo "Building..."
run_cmd cargo build -p llm-coding-tools-core --quiet
run_cmd cargo build -p llm-coding-tools-subagents --quiet
run_cmd cargo build -p llm-coding-tools-rig --quiet
run_cmd cargo build -p llm-coding-tools-serdesai --quiet

echo "Testing..."
run_cmd cargo test -p llm-coding-tools-core --quiet
run_cmd cargo test -p llm-coding-tools-subagents --quiet
run_cmd cargo test -p llm-coding-tools-rig --quiet
run_cmd cargo test -p llm-coding-tools-serdesai --quiet

echo "Clippy..."
run_cmd cargo clippy -p llm-coding-tools-core --quiet -- -D warnings
run_cmd cargo clippy -p llm-coding-tools-subagents --quiet -- -D warnings
run_cmd cargo clippy -p llm-coding-tools-rig --quiet -- -D warnings
run_cmd cargo clippy -p llm-coding-tools-serdesai --quiet -- -D warnings

echo "Testing blocking feature..."
run_cmd cargo test -p llm-coding-tools-core --no-default-features --features blocking --quiet

echo "Docs..."
run_cmd env RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --quiet

echo "Formatting..."
run_cmd cargo fmt --all --quiet

echo "Publish dry-run..."
run_cmd cargo publish --dry-run -p llm-coding-tools-core --quiet
run_cmd cargo publish --dry-run -p llm-coding-tools-subagents --quiet
run_cmd cargo publish --dry-run -p llm-coding-tools-rig --quiet
run_cmd cargo publish --dry-run -p llm-coding-tools-serdesai --quiet

echo "All checks passed!"
