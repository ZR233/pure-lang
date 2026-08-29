Set-StrictMode -Version Latest

function Assert-StudioReleasePageUrl {
    param(
        [Parameter(Mandatory)]
        [object] $Release,

        [Parameter(Mandatory)]
        [string] $Repository,

        [Parameter(Mandatory)]
        [string] $Tag
    )

    $tagBaseUrl = "https://github.com/$Repository/releases/tag/"
    if ([bool]$Release.draft) {
        $draftUrlPattern = '^' + [regex]::Escape($tagBaseUrl) + 'untagged-[0-9a-f]+$'
        if ([string]$Release.html_url -cnotmatch $draftUrlPattern) {
            throw "Draft Release has an unexpected GitHub URL: $($Release.html_url)"
        }
        return
    }

    if ([string]$Release.html_url -cne "$tagBaseUrl$Tag") {
        throw "Published Release has an unexpected GitHub URL: $($Release.html_url)"
    }
}

function Get-StudioReleaseAssetNames {
    param(
        [Parameter(Mandatory)]
        [string] $Version
    )

    if ($Version -notmatch '^(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)$') {
        throw "Studio release version must be stable x.y.z SemVer: $Version"
    }

    $setup = "Pure-Studio-$Version-windows-x86_64-setup.exe"
    $portable = "Pure-Studio-$Version-windows-x86_64-portable.zip"
    $aarch64Helper = "Pure-Remote-Helper-$Version-aarch64-unknown-linux-musl"
    $x86Helper = "Pure-Remote-Helper-$Version-x86_64-unknown-linux-musl"
    return @(
        $setup
        "$setup.minisig"
        $portable
        "$portable.minisig"
        $aarch64Helper
        "$aarch64Helper.sha256"
        "$aarch64Helper.minisig"
        $x86Helper
        "$x86Helper.sha256"
        "$x86Helper.minisig"
        'latest.json'
        'SHA256SUMS.txt'
    )
}

function Assert-StudioReleaseRemoteAssets {
    param(
        [Parameter(Mandatory)]
        [object] $Release,

        [Parameter(Mandatory)]
        [string[]] $ExpectedNames,

        [switch] $RequireComplete
    )

    $assets = @($Release.assets)
    $names = @($assets | ForEach-Object name)
    $duplicateNames = @($names | Group-Object | Where-Object Count -gt 1 | ForEach-Object Name)
    if ($duplicateNames.Count -ne 0) {
        throw "Release contains duplicate assets: $($duplicateNames -join ', ')"
    }

    $unknownNames = @($names | Where-Object { $_ -notin $ExpectedNames })
    if ($unknownNames.Count -ne 0) {
        throw "Release contains unknown assets: $($unknownNames -join ', ')"
    }

    $invalidAssets = @(
        $assets | Where-Object {
            $_.state -ne 'uploaded' -or
            [long]$_.size -le 0 -or
            $_.digest -notmatch '^sha256:[0-9a-f]{64}$'
        }
    )
    if ($invalidAssets.Count -ne 0) {
        throw "Release contains incomplete assets: $($invalidAssets.name -join ', ')"
    }

    if ($RequireComplete -and ($names.Count -ne $ExpectedNames.Count)) {
        $missingNames = @($ExpectedNames | Where-Object { $_ -notin $names })
        throw "Published Release is missing assets: $($missingNames -join ', ')"
    }
}

function Get-StudioReleaseAssetPlan {
    param(
        [Parameter(Mandatory)]
        [object] $Release,

        [Parameter(Mandatory)]
        [string] $Version,

        [Parameter(Mandatory)]
        [string] $AssetDirectory
    )

    $expectedNames = @(Get-StudioReleaseAssetNames -Version $Version)
    Assert-StudioReleaseRemoteAssets `
        -Release $Release `
        -ExpectedNames $expectedNames `
        -RequireComplete:(-not [bool]$Release.draft)

    $assets = @($Release.assets)
    $missingNames = [Collections.Generic.List[string]]::new()
    foreach ($name in $expectedNames) {
        $path = Join-Path $AssetDirectory $name
        if (-not (Test-Path -LiteralPath $path -PathType Leaf)) {
            throw "Local release asset is missing: $path"
        }

        $file = Get-Item -LiteralPath $path
        if ($file.Length -le 0) {
            throw "Local release asset is empty: $path"
        }
        $localDigest = "sha256:$((Get-FileHash -LiteralPath $path -Algorithm SHA256).Hash.ToLowerInvariant())"
        $remote = @($assets | Where-Object name -eq $name)
        if ($remote.Count -eq 0) {
            $missingNames.Add($name)
            continue
        }
        if ([long]$remote[0].size -ne $file.Length) {
            throw "Release asset size differs from the verified artifact: $name"
        }
        if ($remote[0].digest -cne $localDigest) {
            throw "Release asset digest differs from the verified artifact: $name"
        }
    }

    if (-not [bool]$Release.draft -and $missingNames.Count -ne 0) {
        throw "Published Release cannot be resumed with missing assets: $($missingNames -join ', ')"
    }

    return [pscustomobject]@{
        ExpectedNames = $expectedNames
        MissingNames = @($missingNames)
        Complete = $missingNames.Count -eq 0
    }
}
