#!/usr/bin/env bash
# Post-change verification script
# All steps must pass without warnings
# Keep in sync with verify.ps1
#
# Note: llm-coding-tools-serdesai is async-only.
# Blocking mode is validated for core and models-dev.
# llm-coding-tools-bubblewrap is Linux-only; all bubblewrap steps
# are skipped on non-Linux platforms.

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

IS_LINUX=false
if [ "$(uname -s)" = "Linux" ]; then
  IS_LINUX=true
fi

echo "Building..."
if [ "$IS_LINUX" = true ]; then
  run_cmd cargo build -p llm-coding-tools-bubblewrap --quiet
fi
run_cmd cargo build -p llm-coding-tools-core --quiet
run_cmd cargo build -p llm-coding-tools-agents --quiet
run_cmd cargo build -p llm-coding-tools-serdesai --quiet
run_cmd cargo build -p llm-coding-tools-models-dev --quiet

echo "Testing..."
if [ "$IS_LINUX" = true ]; then
  run_cmd cargo test -p llm-coding-tools-bubblewrap --quiet
fi
run_cmd cargo test -p llm-coding-tools-core --quiet
run_cmd cargo test -p llm-coding-tools-agents --quiet
run_cmd cargo test -p llm-coding-tools-serdesai --quiet
run_cmd cargo test -p llm-coding-tools-models-dev --quiet

echo "Clippy..."
if [ "$IS_LINUX" = true ]; then
  run_cmd cargo clippy -p llm-coding-tools-bubblewrap --quiet -- -D warnings
fi
run_cmd cargo clippy -p llm-coding-tools-core --quiet -- -D warnings
run_cmd cargo clippy -p llm-coding-tools-agents --quiet -- -D warnings
run_cmd cargo clippy -p llm-coding-tools-serdesai --quiet -- -D warnings
run_cmd cargo clippy -p llm-coding-tools-models-dev --quiet -- -D warnings

echo "Testing linux-bubblewrap feature..."
if [ "$IS_LINUX" = true ]; then
  run_cmd cargo test -p llm-coding-tools-bubblewrap --features tokio --quiet
  run_cmd cargo test -p llm-coding-tools-bubblewrap --no-default-features --features blocking --quiet
  run_cmd cargo test -p llm-coding-tools-core --features linux-bubblewrap --quiet
  run_cmd cargo test -p llm-coding-tools-core --no-default-features --features blocking,linux-bubblewrap --quiet
  run_cmd cargo test -p llm-coding-tools-serdesai --features linux-bubblewrap --quiet
else
  echo "  (skipped — not Linux)"
fi

echo "Testing blocking feature..."
run_cmd cargo test -p llm-coding-tools-core --no-default-features --features blocking --quiet
run_cmd cargo test -p llm-coding-tools-models-dev --no-default-features --features blocking --quiet

echo "Docs..."
DOC_ARGS=(--workspace --document-private-items --no-deps --quiet)
if [ "$IS_LINUX" = false ]; then
  DOC_ARGS+=(--exclude llm-coding-tools-bubblewrap)
fi
run_cmd env RUSTDOCFLAGS="-D warnings" cargo doc "${DOC_ARGS[@]}"

echo "Formatting..."
run_cmd cargo fmt --all --check --quiet

echo "Publish dry-run..."
if [ "$IS_LINUX" = true ]; then
  run_cmd cargo publish --dry-run --allow-dirty -p llm-coding-tools-bubblewrap --quiet
fi
PKG_ARGS=(--workspace --allow-dirty --quiet)
if [ "$IS_LINUX" = false ]; then
  PKG_ARGS+=(--exclude llm-coding-tools-bubblewrap)
fi
run_cmd cargo package "${PKG_ARGS[@]}"

echo "All checks passed!"
