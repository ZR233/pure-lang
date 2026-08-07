[CmdletBinding(DefaultParameterSetName = 'Real')]
param(
    [Parameter(ParameterSetName = 'Scripted', Mandatory = $true)]
    [switch]$Scripted,

    [Parameter(ParameterSetName = 'Real', Mandatory = $true)]
    [ValidateScript({ [System.IO.Path]::IsPathFullyQualified($_) })]
    [string]$Workspace,

    [Parameter(ParameterSetName = 'Real', Mandatory = $true)]
    [ValidateScript({ [System.IO.Path]::IsPathFullyQualified($_) })]
    [string]$PromptFile,

    [Parameter(ParameterSetName = 'Real')]
    [ValidateScript({ [System.IO.Path]::IsPathFullyQualified($_) })]
    [string]$ConfigFile = (Join-Path $env:USERPROFILE '.pure\config.toml'),

    [ValidateScript({ [System.IO.Path]::IsPathFullyQualified($_) })]
    [string]$DriverHome,

    [ValidateRange(300, 3600)]
    [int]$GuiStartupTimeoutSeconds = 1800,

    [ValidateRange(30, 7200)]
    [int]$PlanTimeoutSeconds = 300,

    [ValidateRange(60, 21600)]
    [int]$TaskTimeoutSeconds = 3600,

    [ValidateRange(30, 3600)]
    [int]$StallTimeoutSeconds = 300
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
        [Parameter(Mandatory = $true)][TimeSpan]$Timeout,
        [Parameter(Mandatory = $true)][string]$Description
    )
    $deadline = [DateTime]::UtcNow.Add($Timeout)
    while ([DateTime]::UtcNow -lt $deadline) {
        foreach ($path in $Paths) {
            if (Test-Path -LiteralPath $path) {
                $stream = [System.IO.FileStream]::new(
                    $path,
                    [System.IO.FileMode]::Open,
                    [System.IO.FileAccess]::Read,
                    [System.IO.FileShare]::ReadWrite
                )
                try {
                    $reader = [System.IO.StreamReader]::new($stream)
                    try {
                        $content = $reader.ReadToEnd()
                    }
                    finally {
                        $reader.Dispose()
                    }
                }
                finally {
                    $stream.Dispose()
                }
                $match = $Pattern.Match($content)
                if ($match.Success) {
                    return $match
                }
            }
        }
        if ($Process.HasExited) {
            throw "$Description process exited with code $($Process.ExitCode)"
        }
        Start-Sleep -Milliseconds 200
    }
    throw "timed out waiting for $Description"
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
    catch {
        Write-Warning "failed to terminate owned process tree $($Process.Id): $_"
    }
    finally {
        $Process.Dispose()
    }
}

function Find-DescendantProcess {
    param(
        [Parameter(Mandatory = $true)][System.Diagnostics.Process]$RootProcess,
        [Parameter(Mandatory = $true)][string]$Name,
        [Parameter(Mandatory = $true)][TimeSpan]$Timeout
    )
    $deadline = [DateTime]::UtcNow.Add($Timeout)
    while ([DateTime]::UtcNow -lt $deadline) {
        $allProcesses = @(Get-CimInstance Win32_Process)
        $queue = [System.Collections.Generic.Queue[int]]::new()
        $queue.Enqueue($RootProcess.Id)
        $seen = @{}
        while ($queue.Count -gt 0) {
            $parentPid = $queue.Dequeue()
            if ($seen.ContainsKey($parentPid)) {
                continue
            }
            $seen[$parentPid] = $true
            foreach ($child in @($allProcesses | Where-Object ParentProcessId -eq $parentPid)) {
                if ($child.Name -eq $Name) {
                    return [System.Diagnostics.Process]::GetProcessById($child.ProcessId)
                }
                $queue.Enqueue([int]$child.ProcessId)
            }
        }
        if ($RootProcess.HasExited) {
            throw "GUI launcher exited before $Name appeared"
        }
        Start-Sleep -Milliseconds 200
    }
    throw "timed out waiting for GUI descendant $Name"
}

function Write-ProcessDiagnostics {
    param(
        [Parameter(Mandatory = $true)][string]$Path,
        [System.Diagnostics.Process]$GuiProcess,
        [System.Diagnostics.Process]$NativeGuiProcess,
        [System.Diagnostics.Process]$DriverProcess,
        [System.Diagnostics.Process]$ProviderProcess
    )
    $records = foreach ($entry in @(
        @{ Label = 'gui'; Process = $GuiProcess },
        @{ Label = 'nativeGui'; Process = $NativeGuiProcess },
        @{ Label = 'driver'; Process = $DriverProcess },
        @{ Label = 'provider'; Process = $ProviderProcess }
    )) {
        $process = $entry.Process
        if ($null -eq $process) {
            continue
        }
        try {
            $process.Refresh()
            $hasExited = $process.HasExited
            [ordered]@{
                label = $entry.Label
                pid = $process.Id
                hasExited = $hasExited
                exitCode = if ($hasExited) { $process.ExitCode } else { $null }
                capturedAt = [DateTime]::UtcNow.ToString('O')
            }
        }
        catch {
            [ordered]@{
                label = $entry.Label
                pid = $process.Id
                diagnosticError = $_.Exception.Message
                capturedAt = [DateTime]::UtcNow.ToString('O')
            }
        }
    }
    [System.IO.File]::WriteAllText(
        $Path,
        ($records | ConvertTo-Json -Depth 4)
    )
}

function Write-ProcessTreeSnapshot {
    param(
        [Parameter(Mandatory = $true)][string]$Path,
        [System.Diagnostics.Process]$GuiProcess,
        [System.Diagnostics.Process]$DriverProcess,
        [System.Diagnostics.Process]$ProviderProcess
    )
    $allProcesses = @(Get-CimInstance Win32_Process)
    $queue = [System.Collections.Generic.Queue[object]]::new()
    foreach ($entry in @(
        @{ Label = 'gui'; Process = $GuiProcess },
        @{ Label = 'driver'; Process = $DriverProcess },
        @{ Label = 'provider'; Process = $ProviderProcess }
    )) {
        if ($null -ne $entry.Process) {
            $queue.Enqueue([pscustomobject]@{
                Label = $entry.Label
                Pid = $entry.Process.Id
                Depth = 0
            })
        }
    }

    $seen = @{}
    $records = @()
    while ($queue.Count -gt 0) {
        $entry = $queue.Dequeue()
        $key = "$($entry.Label):$($entry.Pid)"
        if ($seen.ContainsKey($key)) {
            continue
        }
        $seen[$key] = $true
        $current = $allProcesses |
            Where-Object ProcessId -eq $entry.Pid |
            Select-Object -First 1
        if ($null -eq $current) {
            continue
        }
        $records += [ordered]@{
            owner = $entry.Label
            pid = [int]$current.ProcessId
            parentPid = [int]$current.ParentProcessId
            depth = [int]$entry.Depth
            name = $current.Name
            createdAt = $current.CreationDate
            commandLine = $current.CommandLine
            workingSetBytes = [int64]$current.WorkingSetSize
            virtualSizeBytes = [int64]$current.VirtualSize
            pageFileKib = [int64]$current.PageFileUsage
            peakPageFileKib = [int64]$current.PeakPageFileUsage
        }
        foreach ($child in @($allProcesses | Where-Object ParentProcessId -eq $entry.Pid)) {
            $queue.Enqueue([pscustomobject]@{
                Label = $entry.Label
                Pid = [int]$child.ProcessId
                Depth = $entry.Depth + 1
            })
        }
    }

    [System.IO.File]::WriteAllText(
        $Path,
        ([ordered]@{
            capturedAt = [DateTime]::UtcNow.ToString('O')
            processes = $records
        } | ConvertTo-Json -Depth 6)
    )
}

function Invoke-Git {
    param(
        [Parameter(Mandatory = $true)][string]$WorkingDirectory,
        [Parameter(Mandatory = $true)][string[]]$Arguments
    )
    & git -C $WorkingDirectory @Arguments
    if ($LASTEXITCODE -ne 0) {
        throw "git failed in $WorkingDirectory`: $($Arguments -join ' ')"
    }
}

$repoRoot = [System.IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..\..\..'))
$appDir = Join-Path $repoRoot 'code\pure-studio'
if ([string]::IsNullOrWhiteSpace($DriverHome)) {
    $DriverHome = Join-Path ([System.IO.Path]::GetTempPath()) "pure-studio-task-driver-$([Guid]::NewGuid().ToString('N'))"
}
$DriverHome = [System.IO.Path]::GetFullPath($DriverHome)
if (Test-Path -LiteralPath $DriverHome) {
    $existing = @(Get-ChildItem -LiteralPath $DriverHome -Force)
    if ($existing.Count -ne 0) {
        throw "Driver home must be new or empty: $DriverHome"
    }
}

$configHome = Join-Path $DriverHome 'home'
$studioHome = Join-Path $DriverHome 'studio'
$localAppData = Join-Path $DriverHome 'localappdata'
$logDir = Join-Path $DriverHome 'logs'
foreach ($path in @($configHome, $studioHome, $localAppData, $logDir)) {
    New-Item -ItemType Directory -Path $path -Force | Out-Null
}

$providerProcess = $null
$guiProcess = $null
$nativeGuiProcess = $null
$driverProcess = $null
$providerStdout = Join-Path $logDir 'provider.stdout.log'
$providerStderr = Join-Path $logDir 'provider.stderr.log'
$providerRequests = Join-Path $logDir 'provider.requests.jsonl'
$guiStdout = Join-Path $logDir 'gui.stdout.log'
$guiStderr = Join-Path $logDir 'gui.stderr.log'
$driverStdout = Join-Path $logDir 'driver.stdout.log'
$driverStderr = Join-Path $logDir 'driver.stderr.log'
$snapshots = Join-Path $logDir 'driver.snapshots.jsonl'
$processTree = Join-Path $logDir 'process-tree-last-alive.json'

try {
    if ($Scripted) {
        $Workspace = Join-Path $DriverHome 'workspace'
        New-Item -ItemType Directory -Path $Workspace -Force | Out-Null
        Invoke-Git -WorkingDirectory $Workspace -Arguments @('init', '--initial-branch=main')
        [System.IO.File]::WriteAllText((Join-Path $Workspace 'README.md'), "# Flutter Driver Task Fixture`n")
        Invoke-Git -WorkingDirectory $Workspace -Arguments @('add', '--', 'README.md')
        Invoke-Git -WorkingDirectory $Workspace -Arguments @(
            '-c', 'user.name=Pure Studio',
            '-c', 'user.email=pure-studio@local',
            'commit', '-m', 'test: initialize driver fixture'
        )
        $PromptFile = Join-Path $DriverHome 'prompt.txt'
        [System.IO.File]::WriteAllText($PromptFile, "Execute the deterministic offline Task Driver fixture.`n")
        $providerProcess = Start-LoggedProcess `
            -FilePath 'cargo' `
            -Arguments @(
                'run', '-p', 'pl-studio-runtime',
                '--bin', 'studio_task_driver_provider', '--',
                '--workspace', $Workspace,
                '--config-home', $configHome,
                '--request-log', $providerRequests
            ) `
            -WorkingDirectory $repoRoot `
            -StdoutPath $providerStdout `
            -StderrPath $providerStderr
        Wait-ForLogMatch `
            -Process $providerProcess `
            -Paths @($providerStdout, $providerStderr) `
            -Pattern ([regex]'PURE_TASK_PROVIDER_READY\s+(http://\S+)') `
            -Timeout ([TimeSpan]::FromMinutes(3)) `
            -Description 'scripted provider readiness' | Out-Null
    }
    else {
        $Workspace = [System.IO.Path]::GetFullPath($Workspace)
        $PromptFile = [System.IO.Path]::GetFullPath($PromptFile)
        $ConfigFile = [System.IO.Path]::GetFullPath($ConfigFile)
        if (-not (Test-Path -LiteralPath $Workspace -PathType Container)) {
            throw "workspace does not exist: $Workspace"
        }
        if (-not (Test-Path -LiteralPath $PromptFile -PathType Leaf)) {
            throw "prompt file does not exist: $PromptFile"
        }
        if (-not (Test-Path -LiteralPath $ConfigFile -PathType Leaf)) {
            throw "Studio config does not exist: $ConfigFile"
        }
        $configDir = Join-Path $configHome '.pure'
        New-Item -ItemType Directory -Path $configDir -Force | Out-Null
        Copy-Item -LiteralPath $ConfigFile -Destination (Join-Path $configDir 'config.toml')
    }

    $originalUserProfile = $env:USERPROFILE
    $guiEnvironment = @{
        'PURE_STUDIO_HOME' = $studioHome
        'USERPROFILE' = $configHome
        'HOME' = $configHome
        'LOCALAPPDATA' = $localAppData
        'PURE_STUDIO_NATIVE_LIFECYCLE_LOG' = Join-Path $logDir 'native-window.log'
        'CARGO_HOME' = if ($env:CARGO_HOME) { $env:CARGO_HOME } else { Join-Path $originalUserProfile '.cargo' }
        'RUSTUP_HOME' = if ($env:RUSTUP_HOME) { $env:RUSTUP_HOME } else { Join-Path $originalUserProfile '.rustup' }
    }
    if ($env:PUB_CACHE) {
        $guiEnvironment['PUB_CACHE'] = $env:PUB_CACHE
    }
    $guiProcess = Start-LoggedProcess `
        -FilePath 'cargo' `
        -Arguments @('xtask', 'run-gui', '--driver', '--log-level', 'debug') `
        -WorkingDirectory $repoRoot `
        -StdoutPath $guiStdout `
        -StderrPath $guiStderr `
        -Environment $guiEnvironment

    $vmMatch = Wait-ForLogMatch `
        -Process $guiProcess `
        -Paths @($guiStdout, $guiStderr) `
        -Pattern ([regex]'(?i)(?:The Dart VM service is listening on|A Dart VM Service on .*? is available at:)\s+(https?://[^\s]+)') `
        -Timeout ([TimeSpan]::FromSeconds($GuiStartupTimeoutSeconds)) `
        -Description 'Flutter VM service'
    $vmServiceUrl = $vmMatch.Groups[1].Value.TrimEnd('.', ',')
    $nativeGuiProcess = Find-DescendantProcess `
        -RootProcess $guiProcess `
        -Name 'pure_studio.exe' `
        -Timeout ([TimeSpan]::FromSeconds(30))

    $driverArguments = @(
        'run', 'test_driver/task_acceptance_driver.dart',
        '--vm-service-url', $vmServiceUrl,
        '--workspace', $Workspace,
        '--prompt-file', $PromptFile,
        '--snapshot-output', $snapshots,
        '--plan-timeout-seconds', $PlanTimeoutSeconds.ToString(),
        '--task-timeout-seconds', $TaskTimeoutSeconds.ToString(),
        '--stall-timeout-seconds', $StallTimeoutSeconds.ToString()
    )
    $driverProcess = Start-LoggedProcess `
        -FilePath 'dart' `
        -Arguments $driverArguments `
        -WorkingDirectory $appDir `
        -StdoutPath $driverStdout `
        -StderrPath $driverStderr
    do {
        Write-ProcessTreeSnapshot `
            -Path $processTree `
            -GuiProcess $guiProcess `
            -DriverProcess $driverProcess `
            -ProviderProcess $providerProcess
        $driverExited = $driverProcess.WaitForExit(5000)
        if (-not $driverExited -and $nativeGuiProcess.HasExited) {
            $nativeGuiProcess.WaitForExit()
            throw "native Studio GUI exited with code $($nativeGuiProcess.ExitCode)"
        }
    } while (-not $driverExited)
    if ($driverProcess.ExitCode -ne 0) {
        throw "Flutter Task Driver failed with exit code $($driverProcess.ExitCode)"
    }

    Write-Output "Task Driver completed successfully."
    Write-Output "Artifacts: $DriverHome"
}
catch {
    Write-ProcessTreeSnapshot `
        -Path $processTree `
        -GuiProcess $guiProcess `
        -DriverProcess $driverProcess `
        -ProviderProcess $providerProcess
    Write-ProcessDiagnostics `
        -Path (Join-Path $logDir 'process-exit.json') `
        -GuiProcess $guiProcess `
        -NativeGuiProcess $nativeGuiProcess `
        -DriverProcess $driverProcess `
        -ProviderProcess $providerProcess
    Write-Error "Task Driver failed. Preserved artifacts: $DriverHome`n$_"
    throw
}
finally {
    Stop-OwnedProcessTree $driverProcess
    Stop-OwnedProcessTree $guiProcess
    Stop-OwnedProcessTree $providerProcess
}
