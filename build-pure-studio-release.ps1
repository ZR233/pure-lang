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

function Invoke-CheckedCommand {
    param(
        [string]$Command,
        [string[]]$Arguments
    )

    & $Command @Arguments
    if ($LASTEXITCODE -ne 0) {
        throw "$Command failed with exit code $LASTEXITCODE"
    }
}

if (-not (Test-Path $StudioDir)) {
    throw "Pure Studio directory not found: $StudioDir"
}

Sync-ProcessPath
Assert-CommandAvailable "node" "Install Node.js LTS, reopen PowerShell, then run this script again."
Assert-CommandAvailable (Get-NpmCommand) "Install Node.js LTS, reopen PowerShell, then run this script again."
Assert-CommandAvailable "cargo" "Install Rust stable, reopen PowerShell, then run this script again."

$NpmCommand = Get-NpmCommand

Push-Location $StudioDir
$SkipFrontendConfigPath = $null
try {
    if (-not $SkipNpmInstall) {
        if (Test-Path "package-lock.json") {
            Invoke-CheckedCommand $NpmCommand @("ci")
        } else {
            Invoke-CheckedCommand $NpmCommand @("install")
        }
    }

    $tauriArgs = @("run", "tauri:build", "--", "--no-bundle")
    if ($SkipFrontendBuild) {
        if (-not (Test-Path (Join-Path $StudioDir "dist\index.html"))) {
            throw "Frontend dist not found. Remove -SkipFrontendBuild or run npm run build first."
        }
        $SkipFrontendConfigPath = [IO.Path]::ChangeExtension([IO.Path]::GetTempFileName(), ".json")
        Set-Content -LiteralPath $SkipFrontendConfigPath -Value '{"build":{"beforeBuildCommand":null}}' -Encoding UTF8
        $tauriArgs += @("--config", $SkipFrontendConfigPath)
    }

    Invoke-CheckedCommand $NpmCommand $tauriArgs
} finally {
    if ($SkipFrontendConfigPath -and (Test-Path $SkipFrontendConfigPath)) {
        Remove-Item -LiteralPath $SkipFrontendConfigPath -Force
    }
    Pop-Location
}

if (-not (Test-Path $BinaryPath)) {
    throw "Release binary was not produced: $BinaryPath"
}

New-Item -Path $OutputDir -ItemType Directory -Force | Out-Null
$OutputBinaryPath = Join-Path $OutputDir $BinaryName
try {
    Copy-Item -Path $BinaryPath -Destination $OutputBinaryPath -Force
} catch {
    throw "Failed to copy release binary to $OutputBinaryPath. Close any running Pure Studio window using that exe, then run this script again. Original error: $($_.Exception.Message)"
}

Write-Host "Pure Studio release build complete."
Write-Host "Binary: $BinaryPath"
Write-Host "Copied to: $OutputBinaryPath"
