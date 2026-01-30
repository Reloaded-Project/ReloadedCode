# Post-change verification script
# All steps must pass without warnings
# Keep in sync with verify.sh
#
# Note: llm-coding-tools-rig and llm-coding-tools-serdesai are async-only (implement async Tool traits).
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
}

$originalDir = Get-Location
$scriptDir = Split-Path -Parent $MyInvocation.MyCommand.Path
$projectRoot = Join-Path $scriptDir ".."
Set-Location $projectRoot

trap { Set-Location $originalDir }

Write-Host "Building..."
Invoke-LoggedCommand "cargo" @("build", "-p", "llm-coding-tools-core", "--quiet")
Invoke-LoggedCommand "cargo" @("build", "-p", "llm-coding-tools-subagents", "--quiet")
Invoke-LoggedCommand "cargo" @("build", "-p", "llm-coding-tools-rig", "--quiet")
Invoke-LoggedCommand "cargo" @("build", "-p", "llm-coding-tools-serdesai", "--quiet")

Write-Host "Testing..."
Invoke-LoggedCommand "cargo" @("test", "-p", "llm-coding-tools-core", "--quiet")
Invoke-LoggedCommand "cargo" @("test", "-p", "llm-coding-tools-subagents", "--quiet")
Invoke-LoggedCommand "cargo" @("test", "-p", "llm-coding-tools-rig", "--quiet")
Invoke-LoggedCommand "cargo" @("test", "-p", "llm-coding-tools-serdesai", "--quiet")

Write-Host "Clippy..."
Invoke-LoggedCommand "cargo" @("clippy", "-p", "llm-coding-tools-core", "--quiet", "--", "-D", "warnings")
Invoke-LoggedCommand "cargo" @("clippy", "-p", "llm-coding-tools-subagents", "--quiet", "--", "-D", "warnings")
Invoke-LoggedCommand "cargo" @("clippy", "-p", "llm-coding-tools-rig", "--quiet", "--", "-D", "warnings")
Invoke-LoggedCommand "cargo" @("clippy", "-p", "llm-coding-tools-serdesai", "--quiet", "--", "-D", "warnings")

Write-Host "Testing blocking feature..."
Invoke-LoggedCommand "cargo" @("test", "-p", "llm-coding-tools-core", "--no-default-features", "--features", "blocking", "--quiet")

Write-Host "Docs..."
$env:RUSTDOCFLAGS = "-D warnings"
Invoke-LoggedCommand "cargo" @("doc", "--workspace", "--no-deps", "--quiet")

Write-Host "Formatting..."
Invoke-LoggedCommand "cargo" @("fmt", "--all", "--quiet")

Write-Host "Publish dry-run..."
Invoke-LoggedCommand "cargo" @("publish", "--dry-run", "-p", "llm-coding-tools-core", "--quiet")
Invoke-LoggedCommand "cargo" @("publish", "--dry-run", "-p", "llm-coding-tools-subagents", "--quiet")
Invoke-LoggedCommand "cargo" @("publish", "--dry-run", "-p", "llm-coding-tools-rig", "--quiet")
Invoke-LoggedCommand "cargo" @("publish", "--dry-run", "-p", "llm-coding-tools-serdesai", "--quiet")

Write-Host "All checks passed!"
