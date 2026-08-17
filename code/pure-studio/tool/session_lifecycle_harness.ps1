#requires -Version 7.0

# 会话生命周期 Flutter Driver GUI 验收。
#
# Demo：起始页草稿 -> 首次发送建会话 -> 相邻归档 -> 归档到零。
# 真实 Bridge：隔离 home 打开空 Project -> seed -> 归档到零 -> 同 home 重启仍为零。

[CmdletBinding()]
param(
    [switch]$Demo,

    [string]$ArtifactsDirectory,

    [ValidateRange(60, 1800)]
    [int]$GuiStartupTimeoutSeconds = 900,

    [ValidateRange(60, 900)]
    [int]$DriverTimeoutSeconds = 480,

    [ValidateRange(2, 10)]
    [int]$SeedCount = 3
)

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest

. (Join-Path $PSScriptRoot 'harness_common.ps1')

$repoRoot = [System.IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..\..\..'))
$appDir = Join-Path $repoRoot 'code\pure-studio'
if ([string]::IsNullOrWhiteSpace($ArtifactsDirectory)) {
    $ArtifactsDirectory = Join-Path `
        ([System.IO.Path]::GetTempPath()) `
        "pure-studio-session-lifecycle-$([Guid]::NewGuid().ToString('N'))"
}
$ArtifactsDirectory = [System.IO.Path]::GetFullPath($ArtifactsDirectory)
New-Item -ItemType Directory -Path $ArtifactsDirectory -Force | Out-Null
$studioHome = Join-Path $ArtifactsDirectory 'studio-home'
$workspace = Join-Path $ArtifactsDirectory 'workspace'
New-Item -ItemType Directory -Path $workspace -Force | Out-Null

function Invoke-LifecycleRound {
    param(
        [Parameter(Mandatory = $true)][string]$Label,
        [Parameter(Mandatory = $true)][string]$Mode,
        [Parameter(Mandatory = $true)][bool]$UseDemo,
        [Parameter(Mandatory = $true)][bool]$EnableSeed
    )

    $roundArtifacts = Join-Path $ArtifactsDirectory $Label
    New-Item -ItemType Directory -Path $roundArtifacts -Force | Out-Null
    $guiStdout = Join-Path $roundArtifacts 'gui.stdout.log'
    $guiStderr = Join-Path $roundArtifacts 'gui.stderr.log'
    $driverStdout = Join-Path $roundArtifacts 'driver.stdout.log'
    $driverStderr = Join-Path $roundArtifacts 'driver.stderr.log'
    $guiProcess = $null
    $driverProcess = $null
    try {
        $environment = @{
            'PURE_STUDIO_HOME' = $studioHome
            'PURE_STUDIO_NATIVE_LIFECYCLE_LOG' = Join-Path $roundArtifacts 'native-window.log'
        }
        if ($EnableSeed) {
            $environment['PURE_STUDIO_SEED_FIXTURES'] = '1'
        }
        $guiArguments = @('xtask', 'run-gui', '--driver', '--log-level', 'debug')
        if ($UseDemo) {
            $guiArguments = @('xtask', 'run-gui', '--demo', '--driver', '--log-level', 'debug')
        }
        $guiProcess = Start-LoggedProcess `
            -FilePath 'cargo' `
            -Arguments $guiArguments `
            -WorkingDirectory $repoRoot `
            -StdoutPath $guiStdout `
            -StderrPath $guiStderr `
            -Environment $environment
        $vmMatch = Wait-ForLogMatch `
            -Process $guiProcess `
            -Paths @($guiStdout, $guiStderr) `
            -Pattern ([regex]'(?i)(?:The Dart VM service is listening on|A Dart VM Service on .*? is available at:)\s+(https?://[^\s]+)') `
            -Timeout ([TimeSpan]::FromSeconds($GuiStartupTimeoutSeconds))
        $vmServiceUrl = $vmMatch.Groups[1].Value.TrimEnd('.', ',')

        $driverArguments = @(
            'run', 'test_driver/session_lifecycle_acceptance_driver.dart',
            '--vm-service-url', $vmServiceUrl,
            '--mode', $Mode,
            '--artifacts', $roundArtifacts,
            '--seed-count', "$SeedCount"
        )
        if ($Mode -eq 'runtime-seed') {
            $driverArguments += @('--workspace', $workspace)
        }
        $driverProcess = Start-LoggedProcess `
            -FilePath 'dart' `
            -Arguments $driverArguments `
            -WorkingDirectory $appDir `
            -StdoutPath $driverStdout `
            -StderrPath $driverStderr
        if (-not $driverProcess.WaitForExit($DriverTimeoutSeconds * 1000)) {
            throw "$Label Driver exceeded $DriverTimeoutSeconds seconds"
        }
        if ($driverProcess.ExitCode -ne 0) {
            $failure = if (Test-Path -LiteralPath $driverStderr) {
                Get-Content -Raw -LiteralPath $driverStderr
            }
            else {
                'Driver stderr was not created'
            }
            throw "$Label Driver failed with code $($driverProcess.ExitCode): $failure"
        }
        Write-Output "[$Label] $(Get-Content -Raw -LiteralPath $driverStdout)"
    }
    finally {
        Stop-OwnedProcessTree $driverProcess
        Stop-OwnedProcessTree $guiProcess
    }
}

if ($Demo) {
    Invoke-LifecycleRound -Label 'demo' -Mode 'demo' -UseDemo $true -EnableSeed $false
}
else {
    Invoke-LifecycleRound -Label 'round-1' -Mode 'runtime-seed' -UseDemo $false -EnableSeed $true
    Invoke-LifecycleRound -Label 'round-2' -Mode 'runtime-restart' -UseDemo $false -EnableSeed $false
}

Write-Output 'Session lifecycle Driver harness completed successfully.'
Write-Output "Artifacts: $ArtifactsDirectory"
