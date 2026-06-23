param(
    [switch]$Build,
    [switch]$Demo,
    [switch]$DemoFallback
)

$ErrorActionPreference = "Stop"

$Root = Split-Path -Parent $MyInvocation.MyCommand.Path
$AppDir = Join-Path $Root "code\pure-studio-flutter"

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

    & $FilePath @Arguments
    if ($LASTEXITCODE -ne 0) {
        throw "Command failed with exit code ${LASTEXITCODE}: $FilePath $($Arguments -join ' ')"
    }
}

function Invoke-FlutterStudio {
    param(
        [Parameter(Mandatory = $true)]
        [string]$Flutter,
        [Parameter(Mandatory = $true)]
        [bool]$Build,
        [Parameter(Mandatory = $true)]
        [bool]$Demo
    )

    $PreviousDemo = $env:PURE_STUDIO_DEMO
    try {
        $FlutterArgs = @()
        if ($Demo) {
            $env:PURE_STUDIO_DEMO = "true"
            $FlutterArgs += "--dart-define=PURE_STUDIO_DEMO=true"
        } else {
            Remove-Item Env:\PURE_STUDIO_DEMO -ErrorAction SilentlyContinue
        }

        if ($Build) {
            Invoke-Native -FilePath $Flutter -Arguments (@("build", "windows") + $FlutterArgs)
        } else {
            Invoke-Native -FilePath $Flutter -Arguments (@("run", "-d", "windows") + $FlutterArgs)
        }
    } finally {
        if ($null -eq $PreviousDemo) {
            Remove-Item Env:\PURE_STUDIO_DEMO -ErrorAction SilentlyContinue
        } else {
            $env:PURE_STUDIO_DEMO = $PreviousDemo
        }
    }
}

$FlutterCandidates = @()
if ($env:FLUTTER_ROOT) {
    $FlutterCandidates += Join-Path $env:FLUTTER_ROOT "bin\flutter.bat"
}
$FlutterCandidates += @(
    "D:\sdk\flutter\bin\flutter.bat",
    "C:\src\flutter\bin\flutter.bat"
)

$Flutter = Resolve-ToolPath -Name "flutter" -Candidates $FlutterCandidates
if (-not $Flutter) {
    throw "Flutter is not available on PATH and no local Flutter SDK was found."
}

$Codegen = Resolve-ToolPath -Name "flutter_rust_bridge_codegen"

if ($Codegen) {
    $Version = (& $Codegen --version) -join "`n"
    if ($LASTEXITCODE -ne 0) {
        throw "Command failed with exit code ${LASTEXITCODE}: $Codegen --version"
    }
    if ($Version -notmatch "2\.12\.0") {
        Write-Warning "flutter_rust_bridge_codegen should be 2.12.0 for this branch. Current: $Version"
    }
} else {
    Write-Warning "flutter_rust_bridge_codegen is not available. Install version 2.12.0 before regenerating bridge files."
}

Push-Location $AppDir
try {
    Invoke-Native -FilePath $Flutter -Arguments @("pub", "get")
    $UseDemo = $Demo.IsPresent
    if ($UseDemo) {
        Write-Host "Running Pure Studio with PURE_STUDIO_DEMO=true."
    }
    try {
        Invoke-FlutterStudio -Flutter $Flutter -Build $Build.IsPresent -Demo $UseDemo
    } catch {
        if ($UseDemo -or -not $DemoFallback) {
            throw
        }
        Write-Warning "Native Studio build failed. Falling back to PURE_STUDIO_DEMO=true. Original error: $($_.Exception.Message)"
        Remove-Item -LiteralPath (Join-Path $AppDir "build\windows") -Recurse -Force -ErrorAction SilentlyContinue
        Invoke-FlutterStudio -Flutter $Flutter -Build $Build.IsPresent -Demo $true
    }
} finally {
    Pop-Location
}
