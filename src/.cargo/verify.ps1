# Post-change verification script
# All steps must pass without warnings
# Keep in sync with verify.sh
#
# Note: llm-coding-tools-rig and llm-coding-tools-serdesai are async-only (implement async Tool traits).
# The blocking feature only applies to llm-coding-tools-core.

$ErrorActionPreference = "Stop"

$originalDir = Get-Location
$scriptDir = Split-Path -Parent $MyInvocation.MyCommand.Path
$projectRoot = Join-Path $scriptDir ".."
Set-Location $projectRoot

trap { Set-Location $originalDir }

Write-Host "Building..."
cargo build -p llm-coding-tools-core --quiet
cargo build -p llm-coding-tools-subagents --quiet
cargo build -p llm-coding-tools-rig --quiet
cargo build -p llm-coding-tools-serdesai --quiet

Write-Host "Testing..."
cargo test -p llm-coding-tools-core --quiet
cargo test -p llm-coding-tools-subagents --quiet
cargo test -p llm-coding-tools-rig --quiet
cargo test -p llm-coding-tools-serdesai --quiet

Write-Host "Clippy..."
cargo clippy -p llm-coding-tools-core --quiet -- -D warnings
cargo clippy -p llm-coding-tools-subagents --quiet -- -D warnings
cargo clippy -p llm-coding-tools-rig --quiet -- -D warnings
cargo clippy -p llm-coding-tools-serdesai --quiet -- -D warnings

Write-Host "Testing blocking feature..."
cargo test -p llm-coding-tools-core --no-default-features --features blocking --quiet

Write-Host "Docs..."
$env:RUSTDOCFLAGS = "-D warnings"
cargo doc --workspace --no-deps --quiet

Write-Host "Formatting..."
cargo fmt --all --quiet

Write-Host "Publish dry-run..."
cargo publish --dry-run -p llm-coding-tools-core --quiet
cargo publish --dry-run -p llm-coding-tools-subagents --quiet
cargo publish --dry-run -p llm-coding-tools-rig --quiet
cargo publish --dry-run -p llm-coding-tools-serdesai --quiet

Write-Host "All checks passed!"
