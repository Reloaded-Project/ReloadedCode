#!/usr/bin/env bash
# Post-change verification script
# All steps must pass without warnings
# Keep in sync with verify.ps1
#
# Note: reloaded-code-serdesai is async-only.
# Blocking mode is validated for core and models-dev.
# reloaded-code-bubblewrap is Linux-only; all bubblewrap steps
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

echo "Building (async features)..."
run_cmd cargo build -p reloaded-code-core --quiet
run_cmd cargo build -p reloaded-code-agents --quiet
run_cmd cargo build -p reloaded-code-serdesai --quiet
run_cmd cargo build -p reloaded-code-models-dev --quiet

echo "Testing (async features)..."
run_cmd cargo test -p reloaded-code-core --quiet
run_cmd cargo test -p reloaded-code-agents --quiet
run_cmd cargo test -p reloaded-code-serdesai --quiet
run_cmd cargo test -p reloaded-code-models-dev --quiet

echo "Clippy (async features)..."
run_cmd cargo clippy -p reloaded-code-core --quiet -- -D warnings
run_cmd cargo clippy -p reloaded-code-agents --quiet -- -D warnings
run_cmd cargo clippy -p reloaded-code-serdesai --quiet -- -D warnings
run_cmd cargo clippy -p reloaded-code-models-dev --quiet -- -D warnings

echo "Building (blocking feature)..."
run_cmd cargo build -p reloaded-code-core --no-default-features --features blocking --quiet
run_cmd cargo build -p reloaded-code-models-dev --no-default-features --features blocking --quiet

echo "Testing (blocking feature)..."
run_cmd cargo test -p reloaded-code-core --no-default-features --features blocking --quiet
run_cmd cargo test -p reloaded-code-models-dev --no-default-features --features blocking --quiet

echo "Clippy (blocking feature)..."
run_cmd cargo clippy -p reloaded-code-core --no-default-features --features blocking --quiet -- -D warnings
run_cmd cargo clippy -p reloaded-code-models-dev --no-default-features --features blocking --quiet -- -D warnings

echo "Docs..."
DOC_ARGS=(--workspace --document-private-items --no-deps --quiet --exclude reloaded-code-bubblewrap)
run_cmd env RUSTDOCFLAGS="-D warnings" cargo doc "${DOC_ARGS[@]}"

echo "Formatting..."
run_cmd cargo fmt --all --quiet

echo "Publish dry-run..."
run_cmd cargo publish --dry-run --allow-dirty --quiet --workspace

echo "Linux-only feature coverage..."
if [ "$IS_LINUX" = true ]; then
  echo "Building (linux async features)..."
  run_cmd cargo build -p reloaded-code-bubblewrap --quiet
  run_cmd cargo build -p reloaded-code-core --features linux-bubblewrap --quiet
  run_cmd cargo build -p reloaded-code-serdesai --features linux-bubblewrap --quiet

  echo "Testing (linux async features)..."
  run_cmd cargo test -p reloaded-code-bubblewrap --quiet
  run_cmd cargo test -p reloaded-code-core --features linux-bubblewrap --quiet
  run_cmd cargo test -p reloaded-code-serdesai --features linux-bubblewrap --quiet

  echo "Clippy (linux async features)..."
  run_cmd cargo clippy -p reloaded-code-bubblewrap --quiet -- -D warnings
  run_cmd cargo clippy -p reloaded-code-core --features linux-bubblewrap --quiet -- -D warnings
  run_cmd cargo clippy -p reloaded-code-serdesai --features linux-bubblewrap --quiet -- -D warnings

  echo "Building (linux blocking features)..."
  run_cmd cargo build -p reloaded-code-bubblewrap --no-default-features --features blocking --quiet
  run_cmd cargo build -p reloaded-code-core --no-default-features --features blocking,linux-bubblewrap --quiet

  echo "Testing (linux blocking features)..."
  run_cmd cargo test -p reloaded-code-bubblewrap --no-default-features --features blocking --quiet
  run_cmd cargo test -p reloaded-code-core --no-default-features --features blocking,linux-bubblewrap --quiet

  echo "Clippy (linux blocking features)..."
  run_cmd cargo clippy -p reloaded-code-bubblewrap --no-default-features --features blocking --quiet -- -D warnings
  run_cmd cargo clippy -p reloaded-code-core --no-default-features --features blocking,linux-bubblewrap --quiet -- -D warnings

  echo "Docs (linux-only package)..."
  run_cmd env RUSTDOCFLAGS="-D warnings" cargo doc -p reloaded-code-bubblewrap --document-private-items --no-deps --quiet

else
  echo "  (skipped - not Linux)"
fi

echo "All checks passed!"
