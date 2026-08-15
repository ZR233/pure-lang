# Driver harness 共享的进程与日志辅助。
# 由 tool/ 下各 harness 脚本 dot-source 使用。

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
