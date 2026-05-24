$ErrorActionPreference = "Stop"

$StudioDir = Join-Path $PSScriptRoot "code\pure-studio"

if (-not (Test-Path $StudioDir)) {
    throw "Pure Studio directory not found: $StudioDir"
}

Set-Location $StudioDir
npx tauri dev
