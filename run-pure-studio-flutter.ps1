param(
    [switch]$Build,
    [switch]$Demo
)

$ErrorActionPreference = "Stop"

$Root = Split-Path -Parent $MyInvocation.MyCommand.Path
$AppDir = Join-Path $Root "code\pure-studio-flutter"

if (-not (Get-Command flutter -ErrorAction SilentlyContinue)) {
    throw "Flutter is not available on PATH."
}

$Codegen = Get-Command flutter_rust_bridge_codegen -ErrorAction SilentlyContinue
if ($Codegen) {
    $Version = (& flutter_rust_bridge_codegen --version) -join "`n"
    if ($Version -notmatch "2\.12\.0") {
        Write-Warning "flutter_rust_bridge_codegen should be 2.12.0 for this branch. Current: $Version"
    }
} else {
    Write-Warning "flutter_rust_bridge_codegen is not available. Install version 2.12.0 before regenerating bridge files."
}

Push-Location $AppDir
try {
    flutter pub get
    $FlutterArgs = @()
    if ($Demo) {
        $FlutterArgs += "--dart-define=PURE_STUDIO_DEMO=true"
    }
    if ($Build) {
        flutter build windows @FlutterArgs
    } else {
        flutter run -d windows @FlutterArgs
    }
} finally {
    Pop-Location
}
