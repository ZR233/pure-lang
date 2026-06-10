param(
    [switch]$SkipNpmInstall,
    [switch]$SkipFrontendBuild,
    [string]$OutputDir = (Join-Path $PSScriptRoot "dist\pure-studio-release")
)

$ErrorActionPreference = "Stop"

$RepoRoot = $PSScriptRoot
$StudioDir = Join-Path $RepoRoot "code\pure-studio"
$TargetDir = Join-Path $RepoRoot "target\release"
$BinaryName = if ($IsWindows -or $env:OS -eq "Windows_NT") { "pure-studio.exe" } else { "pure-studio" }
$BinaryPath = Join-Path $TargetDir $BinaryName

function Sync-ProcessPath {
    $pathItems = @()
    $pathSeparator = [IO.Path]::PathSeparator

    foreach ($pathValue in @(
        $env:Path,
        [Environment]::GetEnvironmentVariable("Path", "Machine"),
        [Environment]::GetEnvironmentVariable("Path", "User")
    )) {
        if (-not [string]::IsNullOrWhiteSpace($pathValue)) {
            $pathItems += $pathValue -split [regex]::Escape($pathSeparator)
        }
    }

    if (Test-Path "C:\Program Files\nodejs") {
        $pathItems += "C:\Program Files\nodejs"
    }

    $CargoBin = Join-Path $HOME ".cargo\bin"
    if (Test-Path $CargoBin) {
        $pathItems += $CargoBin
    }

    $env:Path = ($pathItems | Where-Object { -not [string]::IsNullOrWhiteSpace($_) } | Select-Object -Unique) -join $pathSeparator
}

function Assert-CommandAvailable($Name, $InstallHint) {
    if (-not (Get-Command $Name -ErrorAction SilentlyContinue)) {
        throw "$Name was not found. $InstallHint"
    }
}

function Get-NpmCommand {
    if (Get-Command "npm.cmd" -ErrorAction SilentlyContinue) {
        return "npm.cmd"
    }

    return "npm"
}

if (-not (Test-Path $StudioDir)) {
    throw "Pure Studio directory not found: $StudioDir"
}

Sync-ProcessPath
Assert-CommandAvailable "node" "Install Node.js LTS, reopen PowerShell, then run this script again."
Assert-CommandAvailable (Get-NpmCommand) "Install Node.js LTS, reopen PowerShell, then run this script again."
Assert-CommandAvailable "cargo" "Install Rust stable, reopen PowerShell, then run this script again."

$NpmCommand = Get-NpmCommand

if (-not $SkipFrontendBuild) {
    Push-Location $StudioDir
    try {
        if (-not $SkipNpmInstall) {
            if (Test-Path "package-lock.json") {
                & $NpmCommand ci
            } else {
                & $NpmCommand install
            }
        }

        & $NpmCommand run build
    } finally {
        Pop-Location
    }
}

Push-Location $RepoRoot
try {
    cargo build -p pure-studio --release
} finally {
    Pop-Location
}

if (-not (Test-Path $BinaryPath)) {
    throw "Release binary was not produced: $BinaryPath"
}

New-Item -Path $OutputDir -ItemType Directory -Force | Out-Null
Copy-Item -Path $BinaryPath -Destination (Join-Path $OutputDir $BinaryName) -Force

Write-Host "Pure Studio release build complete."
Write-Host "Binary: $BinaryPath"
Write-Host "Copied to: $(Join-Path $OutputDir $BinaryName)"
