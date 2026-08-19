[CmdletBinding()]
param(
    [switch]$Scripted,

    [ValidateSet('None', 'InvalidApiKeyPlanner')]
    [string]$ProviderFailureMode = 'None',

    [switch]$ExerciseRecovery,

    [switch]$ExerciseBudgetRecovery,

    [ValidateSet('Auto', 'RewindTail', 'RebuildThread')]
    [string]$RecoveryMode = 'Auto',

    [ValidateSet('New', 'Observe', 'Resume')]
    [string]$Mode = 'New',

    [ValidateScript({ [System.IO.Path]::IsPathFullyQualified($_) })]
    [string]$Workspace,

    [ValidateScript({ [System.IO.Path]::IsPathFullyQualified($_) })]
    [string]$PromptFile,

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

function Get-GitIdentity {
    param([Parameter(Mandatory = $true)][string]$WorkingDirectory)
    $canonical = [System.IO.Path]::GetFullPath($WorkingDirectory)
    $commonDir = (& git -C $canonical rev-parse --path-format=absolute --git-common-dir).Trim()
    if ($LASTEXITCODE -ne 0) {
        throw "cannot resolve Git common dir for $canonical"
    }
    $branch = (& git -C $canonical rev-parse --abbrev-ref HEAD).Trim()
    if ($LASTEXITCODE -ne 0) {
        throw "cannot resolve Git branch for $canonical"
    }
    $head = (& git -C $canonical rev-parse HEAD).Trim()
    if ($LASTEXITCODE -ne 0) {
        throw "cannot resolve Git HEAD for $canonical"
    }
    return [ordered]@{
        workspace = $canonical
        gitCommonDir = [System.IO.Path]::GetFullPath($commonDir)
        branch = $branch
        head = $head
    }
}

function Write-AcceptanceManifest {
    param(
        [Parameter(Mandatory = $true)][string]$Path,
        [Parameter(Mandatory = $true)][System.Collections.IDictionary]$Manifest
    )
    [System.IO.File]::WriteAllText(
        $Path,
        ($Manifest | ConvertTo-Json -Depth 12)
    )
}

function Read-AcceptanceManifest {
    param([Parameter(Mandatory = $true)][string]$Path)
    if (-not (Test-Path -LiteralPath $Path -PathType Leaf)) {
        throw "acceptance manifest does not exist: $Path"
    }
    return Get-Content -LiteralPath $Path -Raw |
        ConvertFrom-Json -AsHashtable -DateKind String
}

function Test-JsonLogEvent {
    param(
        [Parameter(Mandatory = $true)][string]$Path,
        [Parameter(Mandatory = $true)][string]$Event
    )
    if (-not (Test-Path -LiteralPath $Path -PathType Leaf)) {
        return $false
    }
    foreach ($line in [System.IO.File]::ReadLines($Path)) {
        try {
            $record = $line | ConvertFrom-Json -AsHashtable
            if ($record['event'] -eq $Event) {
                return $true
            }
        }
        catch {
            continue
        }
    }
    return $false
}

function Get-JsonLogEvents {
    param(
        [Parameter(Mandatory = $true)][string]$Path,
        [Parameter(Mandatory = $true)][string]$Event
    )
    $records = [System.Collections.Generic.List[object]]::new()
    if (-not (Test-Path -LiteralPath $Path -PathType Leaf)) {
        return @()
    }
    foreach ($line in [System.IO.File]::ReadLines($Path)) {
        try {
            $record = $line | ConvertFrom-Json -AsHashtable
            if ($record['event'] -eq $Event -or $record['kind'] -eq $Event) {
                $records.Add($record)
            }
        }
        catch {
            continue
        }
    }
    return @($records)
}

function Repair-InterruptedHarnessEvidence {
    param(
        [Parameter(Mandatory = $true)][System.Collections.IDictionary]$Manifest
    )
    $promptSubmissionCount = 0
    foreach ($attempt in @($Manifest['attempts'])) {
        $attemptLogDirectory = [string]$attempt['logDirectory']
        if (-not [string]::IsNullOrWhiteSpace($attemptLogDirectory)) {
            $attemptDriverStdout = Join-Path $attemptLogDirectory 'driver.stdout.log'
            $promptSubmissionCount += @(
                Get-JsonLogEvents -Path $attemptDriverStdout -Event 'originalPromptSubmitted'
            ).Count
        }
        if ([string]$attempt['status'] -eq 'running') {
            $attempt['status'] = 'interrupted'
            $attempt['finishedAt'] = [DateTime]::UtcNow.ToString('O')
            $attempt['failure'] = 'acceptance harness ended before attempt finalization'
        }
    }
    $Manifest['originalPromptSubmissionCount'] = $promptSubmissionCount
}

function Get-RemainingTimeout {
    param(
        [Parameter(Mandatory = $true)][DateTime]$DeadlineUtc,
        [Parameter(Mandatory = $true)][TimeSpan]$Requested,
        [Parameter(Mandatory = $true)][string]$Description
    )
    $remaining = $DeadlineUtc.ToUniversalTime() - [DateTime]::UtcNow
    if ($remaining -le [TimeSpan]::Zero) {
        throw "acceptance global deadline expired before $Description"
    }
    if ($remaining -lt $Requested) {
        return $remaining
    }
    return $Requested
}

function Assert-TaskRecoveryEvidence {
    param(
        [Parameter(Mandatory = $true)][System.Collections.IDictionary]$Event,
        [Parameter(Mandatory = $true)][System.Collections.IDictionary]$BaselineTaskIdentity,
        [ValidateSet('Auto', 'RewindTail', 'RebuildThread')]
        [string]$ExpectedMode = 'Auto'
    )
    $recovery = $Event['recovery']
    $preview = $recovery['preview']
    $result = $recovery['result']
    $task = $Event['task']
    if ($null -eq $preview -or $null -eq $result -or $null -eq $task) {
        throw 'Task recovery event is missing preview, result, or canonical Task state'
    }
    $targets = @($preview['targets'] | Where-Object { $_['threadId'] -eq $result['targetThreadId'] })
    if ($targets.Count -ne 1) {
        throw 'Task recovery result does not identify exactly one Preview target'
    }
    $target = $targets[0]
    $workUnitId = [string]$result['workUnitId']
    $beforeUnits = @($BaselineTaskIdentity['workUnits'] | Where-Object { $_['id'] -eq $workUnitId })
    $afterUnits = @($task['workUnits'] | Where-Object { $_['id'] -eq $workUnitId })
    if ($beforeUnits.Count -ne 1 -or $afterUnits.Count -ne 1) {
        throw "Task recovery changed or lost WorkUnit identity $workUnitId"
    }
    $beforeUnit = $beforeUnits[0]
    $afterUnit = $afterUnits[0]
    foreach ($fact in @(
        @{ Name = 'runId'; Before = $BaselineTaskIdentity['runId']; Preview = $preview['runId']; Result = $result['runId']; After = $task['runId'] },
        @{ Name = 'taskGeneration'; Before = $BaselineTaskIdentity['taskGeneration']; Preview = $preview['taskGeneration']; Result = $preview['taskGeneration']; After = $task['taskGeneration'] },
        @{ Name = 'WorkUnitId'; Before = $beforeUnit['id']; Preview = $target['workUnitId']; Result = $result['workUnitId']; After = $afterUnit['id'] },
        @{ Name = 'executorThread'; Before = $beforeUnit['agentId']; Preview = $target['threadId']; Result = $result['targetThreadId']; After = $afterUnit['agentId'] },
        @{ Name = 'branch'; Before = $beforeUnit['branch']; Preview = $target['branch']; Result = $target['branch']; After = $afterUnit['branch'] },
        @{ Name = 'worktree'; Before = $beforeUnit['worktreePath']; Preview = $target['worktreePath']; Result = $target['worktreePath']; After = $afterUnit['worktreePath'] }
    )) {
        $values = @(
            @(
                [string]$fact.Before,
                [string]$fact.Preview,
                [string]$fact.Result,
                [string]$fact.After
            ) | Select-Object -Unique
        )
        if ($values.Count -ne 1 -or [string]::IsNullOrWhiteSpace($values[0])) {
            throw "Task recovery changed $($fact.Name): $($values -join ', ')"
        }
    }
    if ($null -eq $target['attempt'] -or [int]$target['attempt'] -lt 1) {
        throw 'Task recovery Preview did not preserve the executor attempt identity'
    }
    $previewGit = $target['gitFingerprint'] | ConvertTo-Json -Depth 12 -Compress
    $resultGit = $result['gitFingerprint'] | ConvertTo-Json -Depth 12 -Compress
    if ($previewGit -cne $resultGit) {
        throw 'Task recovery mutated the target Git fingerprint'
    }
    if ([int]$result['removedItemCount'] -gt 0 -and
        $result['beforeTranscriptHash'] -eq $result['afterTranscriptHash']) {
        throw 'Task recovery removed transcript items without changing its hash'
    }
    $expectedProtocolMode = switch ($ExpectedMode) {
        'RewindTail' { 'rewindTail' }
        'RebuildThread' { 'rebuildThread' }
        default { $null }
    }
    if ($null -ne $expectedProtocolMode -and
        [string]$result['mode'] -cne $expectedProtocolMode) {
        throw "Task recovery used mode $($result['mode']) instead of $expectedProtocolMode"
    }
    return [ordered]@{
        attempt = [int]$Event['attempt']
        capturedAt = [string]$Event['capturedAt']
        targetAttempt = [int]$target['attempt']
        target = $target
        preview = $preview
        result = $result
    }
}

function Update-ManifestFromSnapshots {
    param(
        [Parameter(Mandatory = $true)][System.Collections.IDictionary]$Manifest,
        [Parameter(Mandatory = $true)][string]$Path
    )
    if (-not (Test-Path -LiteralPath $Path -PathType Leaf)) {
        return
    }
    $lines = @([System.IO.File]::ReadAllLines($Path))
    [array]::Reverse($lines)
    foreach ($line in $lines) {
        try {
            $record = $line | ConvertFrom-Json -AsHashtable
            $task = $record['task']
            if ($null -ne $task) {
                $Manifest['runId'] = $task['runId']
                $Manifest['taskGeneration'] = $task['taskGeneration']
                $Manifest['latestPhase'] = $task['phase']
                $Manifest['expectedHead'] = $task['expectedHead']
                $Manifest['workUnitIds'] = @($task['workUnits'] | ForEach-Object { $_['id'] })
                $Manifest['latestTaskIdentity'] = [ordered]@{
                    runId = $task['runId']
                    taskGeneration = $task['taskGeneration']
                    phase = $task['phase']
                    workUnits = @($task['workUnits'] | ForEach-Object {
                        [ordered]@{
                            id = $_['id']
                            agentId = $_['agentId']
                            branch = $_['branch']
                            worktreePath = $_['worktreePath']
                        }
                    })
                }
                return
            }
        }
        catch {
            continue
        }
    }
}

$repoRoot = [System.IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..\..\..'))
$appDir = Join-Path $repoRoot 'code\pure-studio'
if ($Mode -eq 'New' -and -not $Scripted -and
    ([string]::IsNullOrWhiteSpace($Workspace) -or [string]::IsNullOrWhiteSpace($PromptFile))) {
    throw 'Real New mode requires -Workspace and -PromptFile'
}
if ([string]::IsNullOrWhiteSpace($DriverHome)) {
    if ($Mode -ne 'New') {
        throw 'Observe and Resume require an existing -DriverHome'
    }
    $DriverHome = Join-Path ([System.IO.Path]::GetTempPath()) "pure-studio-task-driver-$([Guid]::NewGuid().ToString('N'))"
}
$DriverHome = [System.IO.Path]::GetFullPath($DriverHome)
$manifestPath = Join-Path $DriverHome 'acceptance-manifest.json'
$manifest = $null
$isScripted = [bool]$Scripted
$exerciseRecoveryEnabled = [bool]$ExerciseRecovery
$exerciseBudgetRecoveryEnabled = [bool]$ExerciseBudgetRecovery
$recoveryModeValue = $RecoveryMode
$providerFailureModeValue = $ProviderFailureMode
if (-not $isScripted -and $providerFailureModeValue -ne 'None') {
    throw 'ProviderFailureMode requires -Scripted'
}
if ($exerciseRecoveryEnabled -and $providerFailureModeValue -ne 'None') {
    throw 'ProviderFailureMode cannot be combined with ExerciseRecovery'
}
if ($exerciseBudgetRecoveryEnabled -and -not $isScripted) {
    throw 'ExerciseBudgetRecovery requires -Scripted'
}
if ($exerciseBudgetRecoveryEnabled -and
    ($exerciseRecoveryEnabled -or $providerFailureModeValue -ne 'None')) {
    throw 'ExerciseBudgetRecovery cannot be combined with ExerciseRecovery or ProviderFailureMode'
}
if ($Mode -eq 'New' -and (Test-Path -LiteralPath $DriverHome)) {
    $existing = @(Get-ChildItem -LiteralPath $DriverHome -Force)
    if ($existing.Count -ne 0) {
        throw "Driver home must be new or empty: $DriverHome"
    }
}
elseif ($Mode -ne 'New') {
    $manifest = Read-AcceptanceManifest -Path $manifestPath
    if ([int]$manifest['schemaVersion'] -ne 1) {
        throw "unsupported acceptance manifest schema: $($manifest['schemaVersion'])"
    }
    Repair-InterruptedHarnessEvidence -Manifest $manifest
    $Workspace = [string]$manifest['workspace']['workspace']
    $PromptFile = [string]$manifest['promptFile']
    $isScripted = [bool]$manifest['scripted']
    $exerciseRecoveryEnabled = [bool]$manifest['exerciseRecovery']
    $exerciseBudgetRecoveryEnabled = if ($manifest.ContainsKey('exerciseBudgetRecovery')) {
        [bool]$manifest['exerciseBudgetRecovery']
    }
    else {
        $false
    }
    $recoveryModeValue = if ($manifest.ContainsKey('recoveryMode')) {
        [string]$manifest['recoveryMode']
    }
    else {
        'Auto'
    }
    $providerFailureModeValue = if ($manifest.ContainsKey('providerFailureMode')) {
        [string]$manifest['providerFailureMode']
    }
    else {
        'None'
    }
    $deadline = [DateTime]::Parse([string]$manifest['globalDeadlineUtc']).ToUniversalTime()
    if ([DateTime]::UtcNow -ge $deadline) {
        throw "acceptance global deadline already expired: $($deadline.ToString('O'))"
    }
    $currentIdentity = Get-GitIdentity -WorkingDirectory $Workspace
    if ($currentIdentity['workspace'] -ne $manifest['workspace']['workspace'] -or
        $currentIdentity['gitCommonDir'] -ne $manifest['workspace']['gitCommonDir'] -or
        $currentIdentity['branch'] -ne $manifest['workspace']['branch']) {
        throw 'workspace Git identity no longer matches the acceptance manifest'
    }
    if (-not (Test-Path -LiteralPath $PromptFile -PathType Leaf)) {
        throw "acceptance Prompt file no longer exists: $PromptFile"
    }
    $currentPromptHash = (Get-FileHash -LiteralPath $PromptFile -Algorithm SHA256).Hash.ToLowerInvariant()
    if ($currentPromptHash -cne [string]$manifest['promptHash']) {
        throw 'acceptance Prompt hash no longer matches the manifest'
    }
}

$configHome = Join-Path $DriverHome 'home'
$studioHome = Join-Path $DriverHome 'studio'
$localAppData = Join-Path $DriverHome 'localappdata'
$logDir = Join-Path $DriverHome 'logs'
foreach ($path in @($configHome, $studioHome, $localAppData, $logDir)) {
    New-Item -ItemType Directory -Path $path -Force | Out-Null
}
$attemptNumber = if ($null -eq $manifest) { 1 } else { @($manifest['attempts']).Count + 1 }
$attemptDir = Join-Path $logDir ('attempt-{0:D3}' -f $attemptNumber)
New-Item -ItemType Directory -Path $attemptDir -Force | Out-Null

$providerProcess = $null
$guiProcess = $null
$nativeGuiProcess = $null
$driverProcess = $null
$providerStdout = Join-Path $attemptDir 'provider.stdout.log'
$providerStderr = Join-Path $attemptDir 'provider.stderr.log'
$providerRequests = Join-Path $attemptDir 'provider.requests.jsonl'
$guiStdout = Join-Path $attemptDir 'gui.stdout.log'
$guiStderr = Join-Path $attemptDir 'gui.stderr.log'
$driverStdout = Join-Path $attemptDir 'driver.stdout.log'
$driverStderr = Join-Path $attemptDir 'driver.stderr.log'
$snapshots = Join-Path $attemptDir 'driver.snapshots.jsonl'
$processTree = Join-Path $attemptDir 'process-tree-last-alive.json'
$progressState = Join-Path $DriverHome 'progress-state.json'
$attemptRecord = [ordered]@{
    attempt = $attemptNumber
    mode = $Mode
    startedAt = [DateTime]::UtcNow.ToString('O')
    finishedAt = $null
    status = 'running'
    recoveryMode = $recoveryModeValue
    providerFailureMode = $providerFailureModeValue
    exerciseBudgetRecovery = $exerciseBudgetRecoveryEnabled
    logDirectory = $attemptDir
    driverExitCode = $null
    recoveryApplied = $false
    failure = $null
}
$promptEventRecorded = $false
$recoveryEventRecorded = $false
$reconnectEventsRecorded = $false

try {
    if ($Mode -eq 'New' -and $isScripted) {
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
        Copy-Item `
            -LiteralPath (Join-Path $appDir 'test_driver\fixtures\task-mode.prompt') `
            -Destination $PromptFile
    }
    elseif ($Mode -eq 'New') {
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

    if (-not (Test-Path -LiteralPath $Workspace -PathType Container)) {
        throw "workspace does not exist: $Workspace"
    }
    if ($Mode -eq 'New' -and -not (Test-Path -LiteralPath $PromptFile -PathType Leaf)) {
        throw "prompt file does not exist: $PromptFile"
    }
    $workspaceIdentity = Get-GitIdentity -WorkingDirectory $Workspace
    if ($Mode -eq 'New') {
        $promptHash = (Get-FileHash -LiteralPath $PromptFile -Algorithm SHA256).Hash.ToLowerInvariant()
        $manifest = [ordered]@{
            schemaVersion = 1
            createdAt = [DateTime]::UtcNow.ToString('O')
            initialTime = [DateTime]::UtcNow.ToString('O')
            globalDeadlineUtc = [DateTime]::UtcNow.AddSeconds($TaskTimeoutSeconds).ToString('O')
            scripted = $isScripted
            exerciseRecovery = $exerciseRecoveryEnabled
            exerciseBudgetRecovery = $exerciseBudgetRecoveryEnabled
            recoveryMode = $recoveryModeValue
            providerFailureMode = $providerFailureModeValue
            promptFile = $PromptFile
            promptHash = $promptHash
            originalPromptSubmissionCount = 0
            recoveryCount = 0
            recoveries = @()
            driverReconnects = @()
            workspace = $workspaceIdentity
            latestWorkspaceGit = $workspaceIdentity
            runId = $null
            taskGeneration = $null
            latestPhase = $null
            expectedHead = $null
            workUnitIds = @()
            attempts = @()
        }
    }
    $attempts = @($manifest['attempts'])
    $attempts += $attemptRecord
    $manifest['attempts'] = $attempts
    Write-AcceptanceManifest -Path $manifestPath -Manifest $manifest

    if ($isScripted) {
        $providerProcess = Start-LoggedProcess `
            -FilePath 'cargo' `
            -Arguments @(
                'run', '-p', 'pl-studio-runtime',
                '--bin', 'studio_task_driver_provider', '--',
                '--workspace', $Workspace,
                '--config-home', $configHome,
                '--request-log', $providerRequests,
                '--state-file', (Join-Path $DriverHome 'provider-state.json'),
                '--exercise-recovery', $exerciseRecoveryEnabled.ToString().ToLowerInvariant()
                '--exercise-budget-recovery', $exerciseBudgetRecoveryEnabled.ToString().ToLowerInvariant()
                '--failure-mode', $(if ($providerFailureModeValue -eq 'InvalidApiKeyPlanner') { 'invalid-api-key-planner' } else { 'none' })
            ) `
            -WorkingDirectory $repoRoot `
            -StdoutPath $providerStdout `
            -StderrPath $providerStderr
        Wait-ForLogMatch `
            -Process $providerProcess `
            -Paths @($providerStdout, $providerStderr) `
            -Pattern ([regex]'PURE_TASK_PROVIDER_READY\s+(http://\S+)') `
            -Timeout (Get-RemainingTimeout `
                -DeadlineUtc ([DateTime]::Parse([string]$manifest['globalDeadlineUtc'])) `
                -Requested ([TimeSpan]::FromMinutes(3)) `
                -Description 'scripted provider readiness') `
            -Description 'scripted provider readiness' | Out-Null
    }

    $originalUserProfile = $env:USERPROFILE
    $guiEnvironment = @{
        'PURE_STUDIO_HOME' = $studioHome
        'USERPROFILE' = $configHome
        'HOME' = $configHome
        'LOCALAPPDATA' = $localAppData
        'PURE_STUDIO_NATIVE_LIFECYCLE_LOG' = Join-Path $attemptDir 'native-window.log'
        'CARGO_HOME' = if ($env:CARGO_HOME) { $env:CARGO_HOME } else { Join-Path $originalUserProfile '.cargo' }
        'RUSTUP_HOME' = if ($env:RUSTUP_HOME) { $env:RUSTUP_HOME } else { Join-Path $originalUserProfile '.rustup' }
    }
    if ($env:PUB_CACHE) {
        $guiEnvironment['PUB_CACHE'] = $env:PUB_CACHE
    }
    if ($exerciseBudgetRecoveryEnabled) {
        $guiEnvironment['PURE_STUDIO_TASK_DRIVER_EXECUTOR_WALL_CLOCK_MS'] = '0'
        $guiEnvironment['PURE_STUDIO_TASK_DRIVER_COMPACTION_TIMEOUT_MS'] = '250'
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
        -Timeout (Get-RemainingTimeout `
            -DeadlineUtc ([DateTime]::Parse([string]$manifest['globalDeadlineUtc'])) `
            -Requested ([TimeSpan]::FromSeconds($GuiStartupTimeoutSeconds)) `
            -Description 'Flutter VM service') `
        -Description 'Flutter VM service'
    $vmServiceUrl = $vmMatch.Groups[1].Value.TrimEnd('.', ',')
    $nativeGuiProcess = Find-DescendantProcess `
        -RootProcess $guiProcess `
        -Name 'pure_studio.exe' `
        -Timeout (Get-RemainingTimeout `
            -DeadlineUtc ([DateTime]::Parse([string]$manifest['globalDeadlineUtc'])) `
            -Requested ([TimeSpan]::FromSeconds(30)) `
            -Description 'native Studio GUI')

    $driverArguments = @(
        'run', 'test_driver/task_acceptance_driver.dart',
        '--mode', $Mode.ToLowerInvariant(),
        '--vm-service-url', $vmServiceUrl,
        '--workspace', $Workspace,
        '--snapshot-output', $snapshots,
        '--progress-state-output', $progressState,
        '--deadline-utc', [string]$manifest['globalDeadlineUtc'],
        '--attempt', $attemptNumber.ToString(),
        '--recovery-count', ([int]$manifest['recoveryCount']).ToString(),
        '--recovery-mode', $recoveryModeValue,
        '--plan-timeout-seconds', $PlanTimeoutSeconds.ToString(),
        '--task-timeout-seconds', $TaskTimeoutSeconds.ToString(),
        '--stall-timeout-seconds', $StallTimeoutSeconds.ToString()
        '--expected-task-phase', $(if ($providerFailureModeValue -eq 'InvalidApiKeyPlanner') { 'failed' } else { 'completed' })
        '--expect-budget-recovery', $exerciseBudgetRecoveryEnabled.ToString().ToLowerInvariant()
    )
    if ($isScripted -and $Mode -eq 'New') {
        $driverArguments += @('--inject-snapshot-disconnect', 'true')
    }
    if ($Mode -eq 'New' -and $exerciseRecoveryEnabled) {
        $driverArguments += @('--stop-at-recovery-pause', 'true')
    }
    if ($Mode -eq 'New') {
        $driverArguments += @('--prompt-file', $PromptFile)
    }
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
        if (-not $driverExited -and
            [DateTime]::UtcNow -ge [DateTime]::Parse([string]$manifest['globalDeadlineUtc']).ToUniversalTime()) {
            throw 'acceptance global deadline expired while waiting for Flutter Task Driver'
        }
    } while (-not $driverExited)
    $attemptRecord['driverExitCode'] = $driverProcess.ExitCode
    if (Test-JsonLogEvent -Path $driverStdout -Event 'originalPromptSubmitted') {
        $manifest['originalPromptSubmissionCount'] = [int]$manifest['originalPromptSubmissionCount'] + 1
        $promptEventRecorded = $true
    }
    $recoveryEvents = @(Get-JsonLogEvents -Path $driverStdout -Event 'taskRecoveryApplied')
    if ($recoveryEvents.Count -gt 1) {
        throw 'a single Driver attempt applied Task recovery more than once'
    }
    if ($recoveryEvents.Count -eq 1) {
        $recoveryEvidence = Assert-TaskRecoveryEvidence `
            -Event $recoveryEvents[0] `
            -BaselineTaskIdentity $manifest['latestTaskIdentity'] `
            -ExpectedMode $recoveryModeValue
        $recoveries = @($manifest['recoveries'])
        $recoveries += $recoveryEvidence
        $manifest['recoveries'] = $recoveries
        $manifest['recoveryCount'] = [int]$manifest['recoveryCount'] + 1
        $attemptRecord['recoveryApplied'] = $true
        $attemptRecord['recoveryId'] = $recoveryEvidence['result']['recoveryId']
        $recoveryEventRecorded = $true
        if ([int]$manifest['recoveryCount'] -gt 3) {
            throw 'Task recovery loop exceeded three successful recoveries'
        }
    }
    $reconnectEvents = @(Get-JsonLogEvents -Path $snapshots -Event 'driverReconnect')
    if ($reconnectEvents.Count -gt 0) {
        $driverReconnects = @($manifest['driverReconnects'])
        foreach ($event in $reconnectEvents) {
            $driverReconnects += [ordered]@{
                attempt = $attemptNumber
                capturedAt = [string]$event['capturedAt']
                reconnectAttempt = $event['attempt']
                phase = $event['phase']
                reason = if ($null -ne $event['reason']) { $event['reason'] } else { $event['error'] }
            }
        }
        $manifest['driverReconnects'] = $driverReconnects
    }
    $reconnectEventsRecorded = $true
    $budgetRecoveryEvents = @(Get-JsonLogEvents -Path $driverStdout -Event 'budgetRecoveryObserved')
    if ($budgetRecoveryEvents.Count -gt 1) {
        throw 'a single Driver attempt observed budget recovery more than once'
    }
    if ($exerciseBudgetRecoveryEnabled -and $Mode -eq 'New') {
        if ($budgetRecoveryEvents.Count -ne 1) {
            throw 'budget recovery acceptance did not capture NeedsAttention and resumed slice one'
        }
        $manifest['budgetRecovery'] = $budgetRecoveryEvents[0]['evidence']
        $attemptRecord['budgetRecoveryObserved'] = $true
    }
    Update-ManifestFromSnapshots -Manifest $manifest -Path $snapshots
    $manifest['latestWorkspaceGit'] = Get-GitIdentity -WorkingDirectory $Workspace
    if ($driverProcess.ExitCode -ne 0) {
        throw "Flutter Task Driver failed with exit code $($driverProcess.ExitCode)"
    }
    if ([int]$manifest['originalPromptSubmissionCount'] -ne 1) {
        throw "acceptance requires exactly one original Prompt submission; observed $($manifest['originalPromptSubmissionCount'])"
    }
    if (Test-JsonLogEvent -Path $driverStdout -Event 'taskPausedForRecovery') {
        if (-not $exerciseRecoveryEnabled -or $Mode -ne 'New') {
            throw 'Task Driver stopped at a recovery pause outside the scripted New recovery scenario'
        }
        $attemptRecord['status'] = 'paused'
    }
    elseif ($Mode -eq 'Observe') {
        $attemptRecord['status'] = 'observed'
    }
    else {
        $attemptRecord['status'] = 'succeeded'
    }
    $attemptRecord['finishedAt'] = [DateTime]::UtcNow.ToString('O')
    Write-AcceptanceManifest -Path $manifestPath -Manifest $manifest

    if ($attemptRecord['status'] -eq 'paused') {
        Write-Output "Task Driver paused durably for recovery. Continue with -Mode Resume and the same DriverHome."
    }
    elseif ($attemptRecord['status'] -eq 'observed') {
        Write-Output "Task Driver observed durable state successfully."
    }
    else {
        Write-Output "Task Driver completed successfully."
    }
    Write-Output "Artifacts: $DriverHome"
}
catch {
    $failureRecord = $_
    Write-ProcessTreeSnapshot `
        -Path $processTree `
        -GuiProcess $guiProcess `
        -DriverProcess $driverProcess `
        -ProviderProcess $providerProcess
    Write-ProcessDiagnostics `
        -Path (Join-Path $attemptDir 'process-exit.json') `
        -GuiProcess $guiProcess `
        -NativeGuiProcess $nativeGuiProcess `
        -DriverProcess $driverProcess `
        -ProviderProcess $providerProcess
    if ($null -ne $manifest) {
        if (-not $promptEventRecorded -and
            (Test-JsonLogEvent -Path $driverStdout -Event 'originalPromptSubmitted')) {
            $manifest['originalPromptSubmissionCount'] = [int]$manifest['originalPromptSubmissionCount'] + 1
            $promptEventRecorded = $true
        }
        if (-not $recoveryEventRecorded -and
            (Test-JsonLogEvent -Path $driverStdout -Event 'taskRecoveryApplied')) {
            $recoveryEvents = @(Get-JsonLogEvents -Path $driverStdout -Event 'taskRecoveryApplied')
            if ($recoveryEvents.Count -ne 1) {
                throw 'cannot preserve ambiguous Task recovery evidence from failed attempt'
            }
            $recoveryEvidence = Assert-TaskRecoveryEvidence `
                -Event $recoveryEvents[0] `
                -BaselineTaskIdentity $manifest['latestTaskIdentity'] `
                -ExpectedMode $recoveryModeValue
            $recoveries = @($manifest['recoveries'])
            $recoveries += $recoveryEvidence
            $manifest['recoveries'] = $recoveries
            $manifest['recoveryCount'] = [int]$manifest['recoveryCount'] + 1
            $attemptRecord['recoveryApplied'] = $true
            $attemptRecord['recoveryId'] = $recoveryEvidence['result']['recoveryId']
            $recoveryEventRecorded = $true
        }
        if (-not $reconnectEventsRecorded) {
            $reconnectEvents = @(Get-JsonLogEvents -Path $snapshots -Event 'driverReconnect')
            if ($reconnectEvents.Count -gt 0) {
                $driverReconnects = @($manifest['driverReconnects'])
                foreach ($event in $reconnectEvents) {
                    $driverReconnects += [ordered]@{
                        attempt = $attemptNumber
                        capturedAt = [string]$event['capturedAt']
                        reconnectAttempt = $event['attempt']
                        phase = $event['phase']
                        reason = if ($null -ne $event['reason']) { $event['reason'] } else { $event['error'] }
                    }
                }
                $manifest['driverReconnects'] = $driverReconnects
            }
            $reconnectEventsRecorded = $true
        }
        Update-ManifestFromSnapshots -Manifest $manifest -Path $snapshots
        try {
            $manifest['latestWorkspaceGit'] = Get-GitIdentity -WorkingDirectory $Workspace
        }
        catch {
            $manifest['latestWorkspaceGitError'] = $_.Exception.Message
        }
        if ($null -ne $driverProcess -and $driverProcess.HasExited) {
            $attemptRecord['driverExitCode'] = $driverProcess.ExitCode
        }
        $attemptRecord['status'] = 'failed'
        $attemptRecord['finishedAt'] = [DateTime]::UtcNow.ToString('O')
        $attemptRecord['failure'] = $failureRecord.Exception.Message
        Write-AcceptanceManifest -Path $manifestPath -Manifest $manifest
    }
    Write-Error "Task Driver failed. Preserved artifacts: $DriverHome`n$failureRecord"
    throw $failureRecord
}
finally {
    Stop-OwnedProcessTree $driverProcess
    Stop-OwnedProcessTree $guiProcess
    Stop-OwnedProcessTree $providerProcess
}
