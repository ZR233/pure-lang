<#
.SYNOPSIS
  Build the Pure Studio Flutter desktop app in release mode for the current OS.

.DESCRIPTION
  Detects the host operating system and runs the appropriate Flutter release
  build (windows / macos / linux), then collects all artifacts into a single
  directory under dist\pure-studio-flutter-release\.

.PARAMETER Demo
  Set PURE_STUDIO_DEMO=true so the app runs without a native Rust backend.

.PARAMETER NoClean
  Skip removing the existing release output directory before copying.

.EXAMPLE
  .\build-pure-studio-flutter-release.ps1
  .\build-pure-studio-flutter-release.ps1 -Demo
  .\build-pure-studio-flutter-release.ps1 -NoClean
#>

param(
    [switch]$Demo,
    [switch]$NoClean
)

$ErrorActionPreference = "Stop"

# ---------------------------------------------------------------------------
# Path setup
# ---------------------------------------------------------------------------

$Root = Split-Path -Parent $MyInvocation.MyCommand.Path
$AppDir = Join-Path $Root "code\pure-studio-flutter"
$DistDir = Join-Path $Root "dist\pure-studio-flutter-release"

if (-not (Test-Path -LiteralPath $AppDir)) {
    throw "Flutter app directory not found: $AppDir"
}

# ---------------------------------------------------------------------------
# Helper functions
# ---------------------------------------------------------------------------

function Resolve-ToolPath {
    param(
        [Parameter(Mandatory = $true)]
        [string]$Name,
        [string[]]$Candidates = @()
    )

    $Command = Get-Command $Name -ErrorAction SilentlyContinue
    if ($Command) {
        return $Command.Source
    }

    foreach ($Candidate in $Candidates) {
        if ($Candidate -and (Test-Path -LiteralPath $Candidate)) {
            return $Candidate
        }
    }

    return $null
}

function Invoke-Native {
    param(
        [Parameter(Mandatory = $true)]
        [string]$FilePath,
        [string[]]$Arguments = @()
    )

    Write-Host "==> $FilePath $($Arguments -join ' ')" -ForegroundColor DarkGray
    & $FilePath @Arguments
    if ($LASTEXITCODE -ne 0) {
        throw "Command failed with exit code ${LASTEXITCODE}: $FilePath $($Arguments -join ' ')"
    }
}

function Read-OptionalTextFile {
    param(
        [Parameter(Mandatory = $true)]
        [string]$Path
    )

    if (Test-Path -LiteralPath $Path) {
        return [System.IO.File]::ReadAllText($Path)
    }
    return $null
}

function Restore-OptionalTextFile {
    param(
        [Parameter(Mandatory = $true)]
        [string]$Path,
        [AllowNull()]
        [string]$Content
    )

    if ($null -eq $Content) {
        if (Test-Path -LiteralPath $Path) {
            Remove-Item -LiteralPath $Path -Force
        }
        return
    }

    [System.IO.File]::WriteAllText($Path, $Content, [System.Text.UTF8Encoding]::new($false))
}

function Normalize-PubspecLockHostedUrls {
    param(
        [AllowNull()]
        [string]$Content
    )

    if ($null -eq $Content) {
        return $null
    }

    return [regex]::Replace($Content, '(?m)^(\s*url:\s*").*?("\s*)$', '$1<hosted-url>$2')
}

function Get-Platform {
    <#
        Returns one of 'Windows', 'macOS', 'Linux'.
        Works in both Windows PowerShell and PowerShell Core on any OS.
    #>
    if ($PSVersionTable.Platform -eq 'Unix') {
        if ($IsMacOS) { return 'macOS' }
        return 'Linux'
    }
    return 'Windows'
}

function Get-BuildSpec {
    param(
        [Parameter(Mandatory = $true)]
        [ValidateSet('Windows', 'macOS', 'Linux')]
        [string]$Platform
    )

    switch ($Platform) {
        'Windows' {
            return @{
                Target    = 'windows'
                Arguments = @('build', 'windows', '--release')
                Artifact  = Join-Path $AppDir 'build\windows\x64\runner\Release'
            }
        }
        'macOS' {
            return @{
                Target    = 'macos'
                Arguments = @('build', 'macos', '--release')
                Artifact  = Join-Path $AppDir 'build\macos\Build\Products\Release'
            }
        }
        'Linux' {
            return @{
                Target    = 'linux'
                Arguments = @('build', 'linux', '--release')
                Artifact  = Join-Path $AppDir 'build\linux\x64\release\bundle'
            }
        }
    }
}

# ---------------------------------------------------------------------------
# Resolve Flutter
# ---------------------------------------------------------------------------

$IsWindowsHost = (Get-Platform) -eq 'Windows'
$FlutterExe = if ($IsWindowsHost) { 'flutter.bat' } else { 'flutter' }

$FlutterCandidates = @()
if ($env:FLUTTER_ROOT) {
    $FlutterCandidates += Join-Path $env:FLUTTER_ROOT "bin\$FlutterExe"
}
$FlutterCandidates += @(
    "D:\sdk\flutter\bin\$FlutterExe",
    "C:\src\flutter\bin\$FlutterExe",
    "$env:HOME\flutter\bin\$FlutterExe"
)

$Flutter = Resolve-ToolPath -Name $FlutterExe -Candidates $FlutterCandidates
if (-not $Flutter) {
    throw "Flutter is not available on PATH and no local Flutter SDK was found. Set FLUTTER_ROOT or add Flutter to PATH."
}

Write-Host "Flutter: $Flutter" -ForegroundColor Cyan

# ---------------------------------------------------------------------------
# Determine platform and build spec
# ---------------------------------------------------------------------------

$Platform = Get-Platform
$BuildSpec = Get-BuildSpec -Platform $Platform
$FlutterArgs = $BuildSpec.Arguments

Write-Host "Building Pure Studio Flutter for $Platform (release)..." -ForegroundColor Cyan

# ---------------------------------------------------------------------------
# Demo environment variable
# ---------------------------------------------------------------------------

$PreviousDemo = $env:PURE_STUDIO_DEMO
$PubspecLockPath = Join-Path $AppDir 'pubspec.lock'
$OriginalPubspecLock = Read-OptionalTextFile -Path $PubspecLockPath
try {
    if ($Demo) {
        $env:PURE_STUDIO_DEMO = 'true'
        $FlutterArgs += '--dart-define=PURE_STUDIO_DEMO=true'
        Write-Host "PURE_STUDIO_DEMO=true" -ForegroundColor Yellow
    } else {
        Remove-Item Env:\PURE_STUDIO_DEMO -ErrorAction SilentlyContinue
    }

    # -----------------------------------------------------------------------
    # Build
    # -----------------------------------------------------------------------

    Push-Location $AppDir
    try {
        Invoke-Native -FilePath $Flutter -Arguments @('pub', 'get')
        $CurrentPubspecLock = Read-OptionalTextFile -Path $PubspecLockPath
        if ($CurrentPubspecLock -ne $OriginalPubspecLock) {
            $OriginalNormalized = Normalize-PubspecLockHostedUrls -Content $OriginalPubspecLock
            $CurrentNormalized = Normalize-PubspecLockHostedUrls -Content $CurrentPubspecLock
            if ($CurrentNormalized -ne $OriginalNormalized) {
                Restore-OptionalTextFile -Path $PubspecLockPath -Content $OriginalPubspecLock
                throw "flutter pub get changed pubspec.lock beyond hosted source URLs. Run flutter pub get manually and review the lockfile before building release artifacts."
            }
            Write-Host "pubspec.lock hosted URLs changed during pub get; restoring after build." -ForegroundColor DarkGray
        }
        Invoke-Native -FilePath $Flutter -Arguments $FlutterArgs
    } finally {
        Pop-Location
    }
} finally {
    if ($null -eq $PreviousDemo) {
        Remove-Item Env:\PURE_STUDIO_DEMO -ErrorAction SilentlyContinue
    } else {
        $env:PURE_STUDIO_DEMO = $PreviousDemo
    }
    Restore-OptionalTextFile -Path $PubspecLockPath -Content $OriginalPubspecLock
}

# ---------------------------------------------------------------------------
# Collect artifacts
# ---------------------------------------------------------------------------

$ArtifactDir = $BuildSpec.Artifact
if (-not (Test-Path -LiteralPath $ArtifactDir)) {
    throw "Build artifact directory not found: $ArtifactDir"
}

if (-not $NoClean -and (Test-Path -LiteralPath $DistDir)) {
    Write-Host "Cleaning previous release output: $DistDir" -ForegroundColor DarkGray
    Remove-Item -LiteralPath $DistDir -Recurse -Force
}

New-Item -Path $DistDir -ItemType Directory -Force | Out-Null
Copy-Item -Path (Join-Path $ArtifactDir '*') -Destination $DistDir -Recurse -Force

# ---------------------------------------------------------------------------
# Summary
# ---------------------------------------------------------------------------

Write-Host ""
Write-Host "Release build complete." -ForegroundColor Green
Write-Host "Output: $DistDir" -ForegroundColor Green
Write-Host ""

$MainFiles = Get-ChildItem -Path $DistDir -File | Sort-Object Name
if ($MainFiles) {
    Write-Host "Files:" -ForegroundColor Cyan
    foreach ($File in $MainFiles) {
        Write-Host "  $($File.Name)"
    }
}

$SubDirs = Get-ChildItem -Path $DistDir -Directory | Sort-Object Name
if ($SubDirs) {
    Write-Host "Directories:" -ForegroundColor Cyan
    foreach ($Dir in $SubDirs) {
        Write-Host "  $($Dir.Name)/"
    }
}
