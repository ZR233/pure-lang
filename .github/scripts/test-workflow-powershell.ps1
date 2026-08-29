$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest

$workflowRoot = Join-Path (Split-Path -Parent $PSScriptRoot) 'workflows'
$workflowFiles = @(Get-ChildItem -LiteralPath $workflowRoot -Filter '*.yml' -File)
$parsedBlocks = 0

foreach ($workflow in $workflowFiles) {
    $lines = @(Get-Content -LiteralPath $workflow.FullName)
    for ($index = 0; $index -lt $lines.Count; $index++) {
        $match = [regex]::Match($lines[$index], '^(?<indent>\s*)run:\s*\|\s*$')
        if (-not $match.Success) {
            continue
        }

        $baseIndent = $match.Groups['indent'].Value.Length
        $explicitShell = $null
        for ($cursor = $index - 1; $cursor -ge 0; $cursor--) {
            $line = $lines[$cursor]
            if ($line.Trim().Length -eq 0) {
                continue
            }
            $lineIndent = $line.Length - $line.TrimStart().Length
            if ($lineIndent -lt $baseIndent) {
                break
            }
            if ($lineIndent -eq $baseIndent) {
                $shellMatch = [regex]::Match($line, '^\s*shell:\s*(?<shell>\S.*?)\s*$')
                if ($shellMatch.Success) {
                    $explicitShell = $shellMatch.Groups['shell'].Value
                    break
                }
            }
        }
        if ($null -ne $explicitShell -and $explicitShell -notmatch '^(pwsh|powershell)(\s|$)') {
            continue
        }

        $block = [Collections.Generic.List[string]]::new()
        for ($cursor = $index + 1; $cursor -lt $lines.Count; $cursor++) {
            $line = $lines[$cursor]
            if ($line.Trim().Length -ne 0) {
                $lineIndent = $line.Length - $line.TrimStart().Length
                if ($lineIndent -le $baseIndent) {
                    break
                }
            }
            $remove = [Math]::Min($baseIndent + 2, $line.Length)
            $block.Add($line.Substring($remove))
        }

        $script = $block -join "`n"
        $script = [regex]::Replace($script, '\$\{\{.*?\}\}', 'github_expression')
        try {
            [void][ScriptBlock]::Create($script)
        } catch {
            throw "Invalid PowerShell block in $($workflow.Name) after line $($index + 1): $($_.Exception.Message)"
        }
        $parsedBlocks++
    }
}

if ($parsedBlocks -eq 0) {
    throw 'No multiline workflow PowerShell blocks were found'
}
Write-Output "Parsed $parsedBlocks workflow PowerShell blocks."
