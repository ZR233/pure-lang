Set-StrictMode -Version Latest

function Get-StudioSupersededReleases {
    param(
        [Parameter(Mandatory)]
        [string] $WorkspaceRoot
    )

    $path = Join-Path $WorkspaceRoot '.github/studio-superseded-releases.json'
    $records = Get-Content -LiteralPath $path -Raw | ConvertFrom-Json -NoEnumerate
    if ($records -isnot [array]) {
        throw 'Superseded Studio Releases must be a JSON array'
    }

    $superseded = @{}
    foreach ($record in $records) {
        if (
            $record.tag -isnot [string] -or
            $record.tag -cnotmatch '^v(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)$' -or
            $record.sha -isnot [string] -or
            $record.sha -cnotmatch '^[0-9a-f]{40}$' -or
            $record.reason -isnot [string] -or
            [string]::IsNullOrWhiteSpace($record.reason)
        ) {
            throw 'Superseded Studio Release requires a stable tag, exact commit SHA, and reason'
        }
        if ($superseded.ContainsKey($record.tag)) {
            throw "Duplicate superseded Studio Release: $($record.tag)"
        }

        $sha = git -C $WorkspaceRoot rev-list -n 1 "refs/tags/$($record.tag)"
        if ($LASTEXITCODE -ne 0 -or [string]$sha -cne $record.sha) {
            throw "Superseded Studio Release $($record.tag) no longer identifies recorded commit $($record.sha)"
        }
        git -C $WorkspaceRoot merge-base --is-ancestor $record.sha HEAD
        if ($LASTEXITCODE -ne 0) {
            throw "Superseded Studio Release $($record.tag) is not contained in checked-out history"
        }
        $superseded[$record.tag] = $record
    }
    return $superseded
}
