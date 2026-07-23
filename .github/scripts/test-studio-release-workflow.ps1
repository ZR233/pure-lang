$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest

$workflowRoot = Join-Path (Split-Path -Parent $PSScriptRoot) 'workflows'
$orchestrator = Get-Content -Raw -LiteralPath (Join-Path $workflowRoot 'studio-release.yml')
$publisher = Get-Content -Raw -LiteralPath (Join-Path $workflowRoot 'studio-release-publish.yml')

function Assert-Contains {
    param(
        [Parameter(Mandatory)]
        [string]$Content,
        [Parameter(Mandatory)]
        [string]$Expected,
        [Parameter(Mandatory)]
        [string]$Message
    )

    if (-not $Content.Contains($Expected, [StringComparison]::Ordinal)) {
        throw $Message
    }
}

function Assert-NotContains {
    param(
        [Parameter(Mandatory)]
        [string]$Content,
        [Parameter(Mandatory)]
        [string]$Forbidden,
        [Parameter(Mandatory)]
        [string]$Message
    )

    if ($Content.Contains($Forbidden, [StringComparison]::Ordinal)) {
        throw $Message
    }
}

function Assert-OccurrenceCount {
    param(
        [Parameter(Mandatory)]
        [string]$Content,
        [Parameter(Mandatory)]
        [string]$Expected,
        [Parameter(Mandatory)]
        [int]$Count,
        [Parameter(Mandatory)]
        [string]$Message
    )

    $actual = [regex]::Matches($Content, [regex]::Escape($Expected)).Count
    if ($actual -ne $Count) {
        throw "$Message (expected $Count, found $actual)"
    }
}

Assert-Contains `
    -Content $orchestrator `
    -Expected '  push:' `
    -Message 'Studio Release must use the official main push trigger'
Assert-Contains `
    -Content $orchestrator `
    -Expected '  workflow_dispatch:' `
    -Message 'Studio Release must retain its manual refresh entrypoint'
Assert-Contains `
    -Content $orchestrator `
    -Expected 'releaseCreated: ${{ steps.release.outputs.release_created }}' `
    -Message 'Studio Release must expose the official release_created output'
Assert-Contains `
    -Content $orchestrator `
    -Expected 'if: needs.inspect.outputs.hasDraft != ''true''' `
    -Message 'Release Please must not create a new release while a stable draft exists'
Assert-Contains `
    -Content $orchestrator `
    -Expected 'if: needs.release-please.outputs.releaseCreated == ''true''' `
    -Message 'A newly created Release must synchronously invoke the publisher'
Assert-Contains `
    -Content $orchestrator `
    -Expected 'if: needs.inspect.outputs.hasDraft == ''true''' `
    -Message 'An existing stable draft must resume the publisher'
Assert-OccurrenceCount `
    -Content $orchestrator `
    -Expected 'uses: ./.github/workflows/studio-release-publish.yml' `
    -Count 2 `
    -Message 'Studio Release must call the local reusable publisher'
Assert-OccurrenceCount `
    -Content $orchestrator `
    -Expected 'secrets: inherit' `
    -Count 2 `
    -Message 'The reusable publisher must receive release secrets from its caller'
Assert-Contains `
    -Content $orchestrator `
    -Expected 'gh api user --jq ''.login''' `
    -Message 'Release Please must verify that its token is a user PAT'
Assert-NotContains `
    -Content $orchestrator `
    -Forbidden 'gh workflow run studio-release-publish.yml' `
    -Message 'Studio Release must not dispatch the publisher through the GitHub API'
Assert-NotContains `
    -Content $orchestrator `
    -Forbidden 'actions: write' `
    -Message 'Studio Release no longer needs permission to dispatch workflows'
Assert-Contains `
    -Content $publisher `
    -Expected 'workflow_call:' `
    -Message 'The Studio publisher must expose workflow_call'
Assert-Contains `
    -Content $publisher `
    -Expected 'workflow_dispatch:' `
    -Message 'The Studio publisher must retain its manual recovery entrypoint'

if ($orchestrator -match '(?m)^\s{2}(pull_request_target|release|workflow_run):') {
    throw 'Studio Release must use the official main push flow instead of secondary event triggers'
}

Write-Output 'Studio Release workflow control-flow tests passed.'
