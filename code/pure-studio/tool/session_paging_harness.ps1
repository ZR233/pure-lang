# 会话分页 + write-behind 关机落库的真实 runtime Driver 验收。
#
# 两轮启动同一隔离 PURE_STUDIO_HOME：
#   第一轮：PURE_STUDIO_SEED_FIXTURES=1 + run-gui --driver（真实 SQLite），
#     seeding 40 条会话 → 验收侧栏翻页 → shutdown 阶段序列（含落库 pending=0）；
#   第二轮：重启读取同一数据库，断言落库数据完整（目录数量一致）。
#
# 用法：
#   pwsh tool/session_paging_harness.ps1 [-SeedCount 40]

[CmdletBinding()]
param(
    # Demo 模式：使用 DriverDemoStudioApi 的确定性数据（42 条历史会话）单轮验收。
    [switch]$Demo,

    [string]$ArtifactsDirectory,

    [ValidateRange(60, 1800)]
    [int]$GuiStartupTimeoutSeconds = 900,

    [ValidateRange(60, 600)]
    [int]$DriverTimeoutSeconds = 300,

    [ValidateRange(10, 200)]
    # 默认 64：超过快照首页（50），真实 runtime 下必须触底翻页才能加载完。
    [int]$SeedCount = 64
)

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest

. (Join-Path $PSScriptRoot 'harness_common.ps1')

$repoRoot = [System.IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..\..\..'))
$appDir = Join-Path $repoRoot 'code\pure-studio'
if ([string]::IsNullOrWhiteSpace($ArtifactsDirectory)) {
    $ArtifactsDirectory = Join-Path `
        ([System.IO.Path]::GetTempPath()) `
        "pure-studio-session-paging-$([Guid]::NewGuid().ToString('N'))"
}
$ArtifactsDirectory = [System.IO.Path]::GetFullPath($ArtifactsDirectory)
New-Item -ItemType Directory -Path $ArtifactsDirectory -Force | Out-Null
$studioHome = Join-Path $ArtifactsDirectory 'studio-home'

function Invoke-Round {
    param(
        [Parameter(Mandatory = $true)][string]$Label,
        [Parameter(Mandatory = $true)][bool]$Seed,
        [Parameter(Mandatory = $true)][int]$ExpectedCount
    )
    $guiStdout = Join-Path $ArtifactsDirectory "$Label-gui.stdout.log"
    $guiStderr = Join-Path $ArtifactsDirectory "$Label-gui.stderr.log"
    $driverStdout = Join-Path $ArtifactsDirectory "$Label-driver.stdout.log"
    $driverStderr = Join-Path $ArtifactsDirectory "$Label-driver.stderr.log"
    $guiProcess = $null
    $driverProcess = $null
    try {
        $environment = @{
            'PURE_STUDIO_HOME' = $studioHome
            'PURE_STUDIO_NATIVE_LIFECYCLE_LOG' = Join-Path $ArtifactsDirectory "$Label-native-window.log"
        }
        if ($Seed) {
            $environment['PURE_STUDIO_SEED_FIXTURES'] = '1'
        }
        $guiArguments = @('xtask', 'run-gui', '--driver', '--log-level', 'debug')
        if ($Demo) {
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
            'run', 'test_driver/session_paging_acceptance_driver.dart',
            '--vm-service-url', $vmServiceUrl
        )
        if ($Seed) {
            $driverArguments += @('--seed-threads', "$SeedCount")
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
        $summary = Get-Content -Raw -LiteralPath $driverStdout
        Write-Output "[$Label] driver ok"
        Write-Output $summary
        # driver 触发的 shutdown 只收束 runtime 与落库，不关闭窗口；
        # 进程树由 finally 的 Stop-OwnedProcessTree 兜底（等价 Job Object 语义）。
        Start-Sleep -Seconds 2
        return $summary
    }
    finally {
        Stop-OwnedProcessTree $driverProcess
        Stop-OwnedProcessTree $guiProcess
    }
}

if ($Demo) {
    Invoke-Round -Label 'demo' -Seed $false -ExpectedCount 42 | Out-Null
}
else {
    Invoke-Round -Label 'round-1' -Seed $true -ExpectedCount $SeedCount | Out-Null
    Invoke-Round -Label 'round-2' -Seed $false -ExpectedCount $SeedCount | Out-Null
}
Write-Output 'Session paging + persistence Driver harness completed successfully.'
Write-Output "Artifacts: $ArtifactsDirectory"
