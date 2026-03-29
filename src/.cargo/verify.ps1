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
    Write-Host "Building (async features)..."
    Invoke-LoggedCommand "cargo" @("build", "-p", "llm-coding-tools-core", "--quiet")
    Invoke-LoggedCommand "cargo" @("build", "-p", "llm-coding-tools-agents", "--quiet")
    Invoke-LoggedCommand "cargo" @("build", "-p", "llm-coding-tools-serdesai", "--quiet")
    Invoke-LoggedCommand "cargo" @("build", "-p", "llm-coding-tools-models-dev", "--quiet")

    Write-Host "Testing (async features)..."
    Invoke-LoggedCommand "cargo" @("test", "-p", "llm-coding-tools-core", "--quiet")
    Invoke-LoggedCommand "cargo" @("test", "-p", "llm-coding-tools-agents", "--quiet")
    Invoke-LoggedCommand "cargo" @("test", "-p", "llm-coding-tools-serdesai", "--quiet")
    Invoke-LoggedCommand "cargo" @("test", "-p", "llm-coding-tools-models-dev", "--quiet")

    Write-Host "Clippy (async features)..."
    Invoke-LoggedCommand "cargo" @("clippy", "-p", "llm-coding-tools-core", "--quiet", "--", "-D", "warnings")
    Invoke-LoggedCommand "cargo" @("clippy", "-p", "llm-coding-tools-agents", "--quiet", "--", "-D", "warnings")
    Invoke-LoggedCommand "cargo" @("clippy", "-p", "llm-coding-tools-serdesai", "--quiet", "--", "-D", "warnings")
    Invoke-LoggedCommand "cargo" @("clippy", "-p", "llm-coding-tools-models-dev", "--quiet", "--", "-D", "warnings")

    Write-Host "Building (blocking feature)..."
    Invoke-LoggedCommand "cargo" @("build", "-p", "llm-coding-tools-core", "--no-default-features", "--features", "blocking", "--quiet")
    Invoke-LoggedCommand "cargo" @("build", "-p", "llm-coding-tools-models-dev", "--no-default-features", "--features", "blocking", "--quiet")

    Write-Host "Testing (blocking feature)..."
    Invoke-LoggedCommand "cargo" @("test", "-p", "llm-coding-tools-core", "--no-default-features", "--features", "blocking", "--quiet")
    Invoke-LoggedCommand "cargo" @("test", "-p", "llm-coding-tools-models-dev", "--no-default-features", "--features", "blocking", "--quiet")

    Write-Host "Clippy (blocking feature)..."
    Invoke-LoggedCommand "cargo" @("clippy", "-p", "llm-coding-tools-core", "--no-default-features", "--features", "blocking", "--quiet", "--", "-D", "warnings")
    Invoke-LoggedCommand "cargo" @("clippy", "-p", "llm-coding-tools-models-dev", "--no-default-features", "--features", "blocking", "--quiet", "--", "-D", "warnings")

    Write-Host "Docs..."
    $docArgs = @("--workspace", "--document-private-items", "--no-deps", "--quiet", "--exclude", "llm-coding-tools-bubblewrap")
    $originalRustdocFlags = $env:RUSTDOCFLAGS
    $env:RUSTDOCFLAGS = "-D warnings"
    try {
        Invoke-LoggedCommand "cargo" (@("doc") + $docArgs)
    } finally {
        $env:RUSTDOCFLAGS = $originalRustdocFlags
    }

    Write-Host "Formatting..."
    Invoke-LoggedCommand "cargo" @("fmt", "--all", "--quiet")

    Write-Host "Linux-only feature coverage..."
    if ($onLinux) {
        Write-Host "Building (linux async features)..."
        Invoke-LoggedCommand "cargo" @("build", "-p", "llm-coding-tools-bubblewrap", "--quiet")
        Invoke-LoggedCommand "cargo" @("build", "-p", "llm-coding-tools-core", "--features", "linux-bubblewrap", "--quiet")
        Invoke-LoggedCommand "cargo" @("build", "-p", "llm-coding-tools-serdesai", "--features", "linux-bubblewrap", "--quiet")

        Write-Host "Testing (linux async features)..."
        Invoke-LoggedCommand "cargo" @("test", "-p", "llm-coding-tools-bubblewrap", "--quiet")
        Invoke-LoggedCommand "cargo" @("test", "-p", "llm-coding-tools-core", "--features", "linux-bubblewrap", "--quiet")
        Invoke-LoggedCommand "cargo" @("test", "-p", "llm-coding-tools-serdesai", "--features", "linux-bubblewrap", "--quiet")

        Write-Host "Clippy (linux async features)..."
        Invoke-LoggedCommand "cargo" @("clippy", "-p", "llm-coding-tools-bubblewrap", "--quiet", "--", "-D", "warnings")
        Invoke-LoggedCommand "cargo" @("clippy", "-p", "llm-coding-tools-core", "--features", "linux-bubblewrap", "--quiet", "--", "-D", "warnings")
        Invoke-LoggedCommand "cargo" @("clippy", "-p", "llm-coding-tools-serdesai", "--features", "linux-bubblewrap", "--quiet", "--", "-D", "warnings")

        Write-Host "Building (linux blocking features)..."
        Invoke-LoggedCommand "cargo" @("build", "-p", "llm-coding-tools-bubblewrap", "--no-default-features", "--features", "blocking", "--quiet")
        Invoke-LoggedCommand "cargo" @("build", "-p", "llm-coding-tools-core", "--no-default-features", "--features", "blocking,linux-bubblewrap", "--quiet")

        Write-Host "Testing (linux blocking features)..."
        Invoke-LoggedCommand "cargo" @("test", "-p", "llm-coding-tools-bubblewrap", "--no-default-features", "--features", "blocking", "--quiet")
        Invoke-LoggedCommand "cargo" @("test", "-p", "llm-coding-tools-core", "--no-default-features", "--features", "blocking,linux-bubblewrap", "--quiet")

        Write-Host "Clippy (linux blocking features)..."
        Invoke-LoggedCommand "cargo" @("clippy", "-p", "llm-coding-tools-bubblewrap", "--no-default-features", "--features", "blocking", "--quiet", "--", "-D", "warnings")
        Invoke-LoggedCommand "cargo" @("clippy", "-p", "llm-coding-tools-core", "--no-default-features", "--features", "blocking,linux-bubblewrap", "--quiet", "--", "-D", "warnings")

        Write-Host "Docs (linux-only package)..."
        $linuxRustdocFlags = $env:RUSTDOCFLAGS
        $env:RUSTDOCFLAGS = "-D warnings"
        try {
            Invoke-LoggedCommand "cargo" @("doc", "-p", "llm-coding-tools-bubblewrap", "--document-private-items", "--no-deps", "--quiet")
        } finally {
            $env:RUSTDOCFLAGS = $linuxRustdocFlags
        }
    } else {
        Write-Host "  (skipped - not Linux)"
    }

    Write-Host "All checks passed!"
}
finally {
    Set-Location $originalDir
}
