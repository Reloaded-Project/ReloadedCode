# Post-change verification script
# All steps must pass without warnings
# Keep in sync with verify.sh
#
# Note: llm-coding-tools-serdesai is async-only (implements async Tool traits).
# The blocking feature only applies to llm-coding-tools-core.

$ErrorActionPreference = "Stop"

function Invoke-LoggedCommand {
    param(
        [string]$Command,
        [string[]]$Arguments
    )

    if ($Arguments.Count -gt 0) {
        Write-Host ($Command + " " + ($Arguments -join " "))
    } else {
        Write-Host $Command
    }

    & $Command @Arguments
    if ($LASTEXITCODE -ne 0) {
        throw "Command '$Command' failed with exit code $LASTEXITCODE"
    }
}

$originalDir = Get-Location
$scriptDir = Split-Path -Parent $MyInvocation.MyCommand.Path
$projectRoot = Join-Path $scriptDir ".."
Set-Location $projectRoot

try {
    Write-Host "Building..."
Invoke-LoggedCommand "cargo" @("build", "-p", "llm-coding-tools-core", "--quiet")
Invoke-LoggedCommand "cargo" @("build", "-p", "llm-coding-tools-agents", "--quiet")
Invoke-LoggedCommand "cargo" @("build", "-p", "llm-coding-tools-serdesai", "--quiet")

Write-Host "Testing..."
Invoke-LoggedCommand "cargo" @("test", "-p", "llm-coding-tools-core", "--quiet")
Invoke-LoggedCommand "cargo" @("test", "-p", "llm-coding-tools-agents", "--quiet")
Invoke-LoggedCommand "cargo" @("test", "-p", "llm-coding-tools-serdesai", "--quiet")

Write-Host "Clippy..."
Invoke-LoggedCommand "cargo" @("clippy", "-p", "llm-coding-tools-core", "--quiet", "--", "-D", "warnings")
Invoke-LoggedCommand "cargo" @("clippy", "-p", "llm-coding-tools-agents", "--quiet", "--", "-D", "warnings")
Invoke-LoggedCommand "cargo" @("clippy", "-p", "llm-coding-tools-serdesai", "--quiet", "--", "-D", "warnings")

Write-Host "Testing blocking feature..."
Invoke-LoggedCommand "cargo" @("test", "-p", "llm-coding-tools-core", "--no-default-features", "--features", "blocking", "--quiet")

Write-Host "Docs..."
$originalRustdocFlags = $env:RUSTDOCFLAGS
$env:RUSTDOCFLAGS = "-D warnings"
try {
    Invoke-LoggedCommand "cargo" @("doc", "--workspace", "--no-deps", "--quiet")
} finally {
    $env:RUSTDOCFLAGS = $originalRustdocFlags
}

Write-Host "Formatting..."
Invoke-LoggedCommand "cargo" @("fmt", "--all", "--check", "--quiet")

Write-Host "Publish dry-run..."
Invoke-LoggedCommand "cargo" @("publish", "--dry-run", "--allow-dirty", "-p", "llm-coding-tools-core", "--quiet")
Invoke-LoggedCommand "cargo" @("publish", "--dry-run", "--allow-dirty", "-p", "llm-coding-tools-agents", "--quiet")
Invoke-LoggedCommand "cargo" @("publish", "--dry-run", "--allow-dirty", "-p", "llm-coding-tools-serdesai", "--quiet")

Write-Host "All checks passed!"
}
finally {
    Set-Location $originalDir
}
