$ErrorActionPreference = "Stop"

$StudioDir = Join-Path $PSScriptRoot "code\pure-studio"

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

function Get-ManifestTimestamp {
    $manifestPaths = @("package.json", "package-lock.json") | Where-Object { Test-Path $_ }
    if ($manifestPaths.Count -eq 0) {
        return [DateTime]::MinValue
    }

    return ($manifestPaths | Get-Item | Sort-Object LastWriteTimeUtc -Descending | Select-Object -First 1).LastWriteTimeUtc
}

function Get-DeclaredPackageNames {
    $manifest = Get-Content -Raw -Path "package.json" | ConvertFrom-Json
    $packageNames = @()

    foreach ($section in @("dependencies", "devDependencies")) {
        $dependencies = $manifest.$section
        if ($null -ne $dependencies) {
            $packageNames += $dependencies.PSObject.Properties.Name
        }
    }

    return $packageNames | Select-Object -Unique
}

function Get-PackageDirectory($PackageName) {
    $directorySeparator = [string][IO.Path]::DirectorySeparatorChar
    $packagePath = $PackageName.Replace("/", $directorySeparator)

    return Join-Path "node_modules" $packagePath
}

function Get-MissingDeclaredPackages {
    if (-not (Test-Path "node_modules")) {
        return Get-DeclaredPackageNames
    }

    $missingPackages = @()
    foreach ($packageName in Get-DeclaredPackageNames) {
        $packageDirectory = Get-PackageDirectory $packageName
        if (-not (Test-Path (Join-Path $packageDirectory "package.json"))) {
            $missingPackages += $packageName
        }
    }

    return $missingPackages
}

function Get-NpmInstallReason($InstallStampPath) {
    if (-not (Test-Path "node_modules")) {
        return "node_modules not found"
    }

    $missingPackages = @(Get-MissingDeclaredPackages)
    if ($missingPackages.Count -gt 0) {
        $shownPackages = ($missingPackages | Select-Object -First 8) -join ", "
        if ($missingPackages.Count -gt 8) {
            $shownPackages = "$shownPackages, ..."
        }

        return "missing declared package(s): $shownPackages"
    }

    if (-not (Test-Path $InstallStampPath)) {
        return "install stamp not found"
    }

    $manifestTimestamp = Get-ManifestTimestamp
    $stampTimestamp = (Get-Item $InstallStampPath).LastWriteTimeUtc
    if ($manifestTimestamp -gt $stampTimestamp) {
        return "package manifest changed"
    }

    return $null
}

function Sync-NpmPackages {
    param(
        [string]$NpmCommand,
        [string]$InstallStampPath,
        [string]$Reason
    )

    Write-Host "Syncing npm dependencies ($Reason)..."
    Invoke-CheckedCommand $NpmCommand @("install")
    Set-Content -LiteralPath $InstallStampPath -Value (Get-Date).ToUniversalTime().ToString("O") -Encoding UTF8
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
try {
    $InstallStampPath = Join-Path "node_modules" ".pure-studio-install.stamp"
    $installReason = Get-NpmInstallReason $InstallStampPath
    if ($installReason) {
        Sync-NpmPackages $NpmCommand $InstallStampPath $installReason
    }

    Invoke-CheckedCommand $NpmCommand @("run", "tauri:dev")
} finally {
    Pop-Location
}
