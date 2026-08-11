[CmdletBinding()]
param(
    [string]$ArtifactsDirectory,

    [ValidateRange(60, 1800)]
    [int]$GuiStartupTimeoutSeconds = 900,

    [ValidateRange(30, 300)]
    [int]$DriverTimeoutSeconds = 120
)

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest

function ConvertTo-NativeArgument {
    param([Parameter(Mandatory = $true)][string]$Value)
    if ($Value -notmatch '[\s"]') {
        return $Value
    }
    return '"' + $Value.Replace('"', '\"') + '"'
}

function Start-LoggedProcess {
    param(
        [Parameter(Mandatory = $true)][string]$FilePath,
        [Parameter(Mandatory = $true)][string[]]$Arguments,
        [Parameter(Mandatory = $true)][string]$WorkingDirectory,
        [Parameter(Mandatory = $true)][string]$StdoutPath,
        [Parameter(Mandatory = $true)][string]$StderrPath,
        [hashtable]$Environment = @{}
    )
    $argumentLine = ($Arguments | ForEach-Object { ConvertTo-NativeArgument $_ }) -join ' '
    return Start-Process `
        -FilePath $FilePath `
        -ArgumentList $argumentLine `
        -WorkingDirectory $WorkingDirectory `
        -RedirectStandardOutput $StdoutPath `
        -RedirectStandardError $StderrPath `
        -Environment $Environment `
        -WindowStyle Hidden `
        -PassThru
}

function Wait-ForLogMatch {
    param(
        [Parameter(Mandatory = $true)][System.Diagnostics.Process]$Process,
        [Parameter(Mandatory = $true)][string[]]$Paths,
        [Parameter(Mandatory = $true)][regex]$Pattern,
        [Parameter(Mandatory = $true)][TimeSpan]$Timeout
    )
    $deadline = [DateTime]::UtcNow.Add($Timeout)
    while ([DateTime]::UtcNow -lt $deadline) {
        foreach ($path in $Paths) {
            if (-not (Test-Path -LiteralPath $path -PathType Leaf)) {
                continue
            }
            $stream = [System.IO.FileStream]::new(
                $path,
                [System.IO.FileMode]::Open,
                [System.IO.FileAccess]::Read,
                [System.IO.FileShare]::ReadWrite
            )
            try {
                $reader = [System.IO.StreamReader]::new($stream)
                try {
                    $match = $Pattern.Match($reader.ReadToEnd())
                    if ($match.Success) {
                        return $match
                    }
                }
                finally {
                    $reader.Dispose()
                }
            }
            finally {
                $stream.Dispose()
            }
        }
        if ($Process.HasExited) {
            throw "GUI launcher exited with code $($Process.ExitCode)"
        }
        Start-Sleep -Milliseconds 200
    }
    throw 'timed out waiting for Flutter VM service'
}

function Stop-OwnedProcessTree {
    param([System.Diagnostics.Process]$Process)
    if ($null -eq $Process) {
        return
    }
    try {
        if (-not $Process.HasExited) {
            $Process.Kill($true)
            $Process.WaitForExit(30000) | Out-Null
        }
    }
    finally {
        $Process.Dispose()
    }
}

$repoRoot = [System.IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..\..\..'))
$appDir = Join-Path $repoRoot 'code\pure-studio'
if ([string]::IsNullOrWhiteSpace($ArtifactsDirectory)) {
    $ArtifactsDirectory = Join-Path `
        ([System.IO.Path]::GetTempPath()) `
        "pure-studio-demo-driver-$([Guid]::NewGuid().ToString('N'))"
}
$ArtifactsDirectory = [System.IO.Path]::GetFullPath($ArtifactsDirectory)
New-Item -ItemType Directory -Path $ArtifactsDirectory -Force | Out-Null

$guiStdout = Join-Path $ArtifactsDirectory 'gui.stdout.log'
$guiStderr = Join-Path $ArtifactsDirectory 'gui.stderr.log'
$driverStdout = Join-Path $ArtifactsDirectory 'driver.stdout.log'
$driverStderr = Join-Path $ArtifactsDirectory 'driver.stderr.log'
$snapshots = Join-Path $ArtifactsDirectory 'driver.snapshots.jsonl'
$screenshot = Join-Path $ArtifactsDirectory 'session-state.png'
$prompt = Join-Path $appDir 'test_driver\fixtures\normal-turn.prompt'
$guiProcess = $null
$driverProcess = $null

try {
    $guiProcess = Start-LoggedProcess `
        -FilePath 'cargo' `
        -Arguments @('xtask', 'run-gui', '--demo', '--driver', '--log-level', 'debug') `
        -WorkingDirectory $repoRoot `
        -StdoutPath $guiStdout `
        -StderrPath $guiStderr `
        -Environment @{
            'PURE_STUDIO_HOME' = Join-Path $ArtifactsDirectory 'studio-home'
            'PURE_STUDIO_NATIVE_LIFECYCLE_LOG' = Join-Path $ArtifactsDirectory 'native-window.log'
        }
    $vmMatch = Wait-ForLogMatch `
        -Process $guiProcess `
        -Paths @($guiStdout, $guiStderr) `
        -Pattern ([regex]'(?i)(?:The Dart VM service is listening on|A Dart VM Service on .*? is available at:)\s+(https?://[^\s]+)') `
        -Timeout ([TimeSpan]::FromSeconds($GuiStartupTimeoutSeconds))
    $vmServiceUrl = $vmMatch.Groups[1].Value.TrimEnd('.', ',')

    $driverProcess = Start-LoggedProcess `
        -FilePath 'dart' `
        -Arguments @(
            'run', 'test_driver/session_state_acceptance_driver.dart',
            '--vm-service-url', $vmServiceUrl,
            '--prompt-file', $prompt,
            '--snapshot-output', $snapshots,
            '--screenshot-output', $screenshot
        ) `
        -WorkingDirectory $appDir `
        -StdoutPath $driverStdout `
        -StderrPath $driverStderr
    if (-not $driverProcess.WaitForExit($DriverTimeoutSeconds * 1000)) {
        throw "session-state Driver exceeded $DriverTimeoutSeconds seconds"
    }
    if ($driverProcess.ExitCode -ne 0) {
        $failure = if (Test-Path -LiteralPath $driverStderr) {
            Get-Content -Raw -LiteralPath $driverStderr
        }
        else {
            'Driver stderr was not created'
        }
        throw "session-state Driver failed with code $($driverProcess.ExitCode): $failure"
    }
    Write-Output 'Deterministic Studio session-state Driver completed successfully.'
    Write-Output "Artifacts: $ArtifactsDirectory"
}
finally {
    Stop-OwnedProcessTree $driverProcess
    Stop-OwnedProcessTree $guiProcess
}
