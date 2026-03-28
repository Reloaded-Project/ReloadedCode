# Post-change verification script
# All steps must pass without warnings
# Keep in sync with verify.sh
#
# Note: llm-coding-tools-serdesai is async-only.
# Blocking mode is validated for core and models-dev.
# llm-coding-tools-bubblewrap is Linux-only; all bubblewrap steps
# are skipped on non-Linux platforms.

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

$onLinux = $IsLinux -eq $true

try {
    Write-Host "Building..."
    if ($onLinux) {
        Invoke-LoggedCommand "cargo" @("build", "-p", "llm-coding-tools-bubblewrap", "--quiet")
    }
    Invoke-LoggedCommand "cargo" @("build", "-p", "llm-coding-tools-core", "--quiet")
    Invoke-LoggedCommand "cargo" @("build", "-p", "llm-coding-tools-agents", "--quiet")
    Invoke-LoggedCommand "cargo" @("build", "-p", "llm-coding-tools-serdesai", "--quiet")
    Invoke-LoggedCommand "cargo" @("build", "-p", "llm-coding-tools-models-dev", "--quiet")

    Write-Host "Testing..."
    if ($onLinux) {
        Invoke-LoggedCommand "cargo" @("test", "-p", "llm-coding-tools-bubblewrap", "--quiet")
    }
    Invoke-LoggedCommand "cargo" @("test", "-p", "llm-coding-tools-core", "--quiet")
    Invoke-LoggedCommand "cargo" @("test", "-p", "llm-coding-tools-agents", "--quiet")
    Invoke-LoggedCommand "cargo" @("test", "-p", "llm-coding-tools-serdesai", "--quiet")
    Invoke-LoggedCommand "cargo" @("test", "-p", "llm-coding-tools-models-dev", "--quiet")

    Write-Host "Clippy..."
    if ($onLinux) {
        Invoke-LoggedCommand "cargo" @("clippy", "-p", "llm-coding-tools-bubblewrap", "--quiet", "--", "-D", "warnings")
    }
    Invoke-LoggedCommand "cargo" @("clippy", "-p", "llm-coding-tools-core", "--quiet", "--", "-D", "warnings")
    Invoke-LoggedCommand "cargo" @("clippy", "-p", "llm-coding-tools-agents", "--quiet", "--", "-D", "warnings")
    Invoke-LoggedCommand "cargo" @("clippy", "-p", "llm-coding-tools-serdesai", "--quiet", "--", "-D", "warnings")
    Invoke-LoggedCommand "cargo" @("clippy", "-p", "llm-coding-tools-models-dev", "--quiet", "--", "-D", "warnings")

    Write-Host "Testing linux-bubblewrap feature..."
    if ($onLinux) {
        Invoke-LoggedCommand "cargo" @("test", "-p", "llm-coding-tools-bubblewrap", "--features", "tokio", "--quiet")
        Invoke-LoggedCommand "cargo" @("test", "-p", "llm-coding-tools-bubblewrap", "--no-default-features", "--features", "blocking", "--quiet")
        Invoke-LoggedCommand "cargo" @("test", "-p", "llm-coding-tools-core", "--features", "linux-bubblewrap", "--quiet")
        Invoke-LoggedCommand "cargo" @("test", "-p", "llm-coding-tools-core", "--no-default-features", "--features", "blocking,linux-bubblewrap", "--quiet")
        Invoke-LoggedCommand "cargo" @("test", "-p", "llm-coding-tools-serdesai", "--features", "linux-bubblewrap", "--quiet")
    } else {
        Write-Host "  (skipped - not Linux)"
    }

    Write-Host "Testing blocking feature..."
    Invoke-LoggedCommand "cargo" @("test", "-p", "llm-coding-tools-core", "--no-default-features", "--features", "blocking", "--quiet")
    Invoke-LoggedCommand "cargo" @("test", "-p", "llm-coding-tools-models-dev", "--no-default-features", "--features", "blocking", "--quiet")

    Write-Host "Docs..."
    $docArgs = @("--workspace", "--document-private-items", "--no-deps", "--quiet")
    if (-not $onLinux) {
        $docArgs += "--exclude"
        $docArgs += "llm-coding-tools-bubblewrap"
    }
    $originalRustdocFlags = $env:RUSTDOCFLAGS
    $env:RUSTDOCFLAGS = "-D warnings"
    try {
        Invoke-LoggedCommand "cargo" (@("doc") + $docArgs)
    } finally {
        $env:RUSTDOCFLAGS = $originalRustdocFlags
    }

    Write-Host "Formatting..."
    Invoke-LoggedCommand "cargo" @("fmt", "--all", "--check", "--quiet")

    Write-Host "Publish dry-run..."
    if ($onLinux) {
        Invoke-LoggedCommand "cargo" @("publish", "--dry-run", "--allow-dirty", "-p", "llm-coding-tools-bubblewrap", "--quiet")
    }
    $pkgArgs = @("--workspace", "--allow-dirty", "--quiet")
    if (-not $onLinux) {
        $pkgArgs += "--exclude"
        $pkgArgs += "llm-coding-tools-bubblewrap"
    }
    Invoke-LoggedCommand "cargo" (@("package") + $pkgArgs)

    Write-Host "All checks passed!"
}
finally {
    Set-Location $originalDir
}
