param(
    [Parameter(Mandatory = $true)]
    [string]$WorkspaceRoot,

    [Parameter(Mandatory = $true)]
    [string]$Configuration,

    [Parameter(Mandatory = $true)]
    [string]$OutputDir
)

$ErrorActionPreference = "Stop"

$WorkspaceRoot = (Resolve-Path -LiteralPath $WorkspaceRoot).Path
$TargetDir = Join-Path $WorkspaceRoot "target"
$env:CARGO_TARGET_DIR = $TargetDir

$CargoArgs = @("build", "-p", "pl-studio-bridge")
$ProfileDir = "debug"
if ($Configuration -eq "Release" -or $Configuration -eq "Profile") {
    $CargoArgs += "--release"
    $ProfileDir = "release"
}

Push-Location $WorkspaceRoot
try {
    & cargo @CargoArgs
    if ($LASTEXITCODE -ne 0) {
        throw "cargo $($CargoArgs -join ' ') failed with exit code $LASTEXITCODE"
    }
} finally {
    Pop-Location
}

$BridgeDll = Join-Path $TargetDir "$ProfileDir\pl_studio_bridge.dll"
if (-not (Test-Path -LiteralPath $BridgeDll)) {
    throw "Rust bridge DLL was not produced: $BridgeDll"
}

New-Item -ItemType Directory -Path $OutputDir -Force | Out-Null
Copy-Item -LiteralPath $BridgeDll -Destination (Join-Path $OutputDir "pl_studio_bridge.dll") -Force

$BridgePdb = Join-Path $TargetDir "$ProfileDir\pl_studio_bridge.pdb"
if (Test-Path -LiteralPath $BridgePdb) {
    Copy-Item -LiteralPath $BridgePdb -Destination (Join-Path $OutputDir "pl_studio_bridge.pdb") -Force
}
