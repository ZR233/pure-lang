$ErrorActionPreference = "Stop"

$StudioDir = Join-Path $PSScriptRoot "code\pure-studio"

function Sync-ProcessPath {
    $pathItems = @()
    foreach ($pathValue in @(
        $env:Path,
        [Environment]::GetEnvironmentVariable("Path", "Machine"),
        [Environment]::GetEnvironmentVariable("Path", "User")
    )) {
        if (-not [string]::IsNullOrWhiteSpace($pathValue)) {
            $pathItems += $pathValue -split ";"
        }
    }

    if (Test-Path "C:\Program Files\nodejs") {
        $pathItems += "C:\Program Files\nodejs"
    }

    $env:Path = ($pathItems | Where-Object { -not [string]::IsNullOrWhiteSpace($_) } | Select-Object -Unique) -join ";"
}

function Assert-CommandAvailable($Name, $InstallHint) {
    if (-not (Get-Command $Name -ErrorAction SilentlyContinue)) {
        throw "$Name was not found. $InstallHint"
    }
}

if (-not (Test-Path $StudioDir)) {
    throw "Pure Studio directory not found: $StudioDir"
}

Sync-ProcessPath
Assert-CommandAvailable "node" "Install Node.js LTS, reopen PowerShell, then run this script again."
Assert-CommandAvailable "npm.cmd" "Install Node.js LTS, reopen PowerShell, then run this script again."

Push-Location $StudioDir
try {
    if (-not (Test-Path "node_modules")) {
        npm.cmd install
    }

    npm.cmd run tauri:dev
} finally {
    Pop-Location
}
