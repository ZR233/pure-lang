$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest

. (Join-Path $PSScriptRoot 'studio-release-assets.ps1')

function Assert-Equal {
    param(
        [Parameter(Mandatory)]
        [object] $Expected,

        [Parameter(Mandatory)]
        [object] $Actual,

        [Parameter(Mandatory)]
        [string] $Message
    )

    if (($Expected | ConvertTo-Json -Compress -Depth 10) -cne ($Actual | ConvertTo-Json -Compress -Depth 10)) {
        throw "$Message. Expected '$Expected', got '$Actual'."
    }
}

function Assert-Throws {
    param(
        [Parameter(Mandatory)]
        [scriptblock] $Action,

        [Parameter(Mandatory)]
        [string] $Message
    )

    try {
        & $Action
    } catch {
        return
    }
    throw $Message
}

function New-TestAsset {
    param(
        [Parameter(Mandatory)]
        [string] $Directory,

        [Parameter(Mandatory)]
        [string] $Name
    )

    $path = Join-Path $Directory $Name
    [IO.File]::WriteAllText($path, "verified-$Name", [Text.UTF8Encoding]::new($false))
    $file = Get-Item -LiteralPath $path
    return [pscustomobject]@{
        name = $Name
        state = 'uploaded'
        size = $file.Length
        digest = "sha256:$((Get-FileHash -LiteralPath $path -Algorithm SHA256).Hash.ToLowerInvariant())"
    }
}

$testRoot = Join-Path ([IO.Path]::GetTempPath()) "pure-studio-assets-$PID-$([Guid]::NewGuid().ToString('N'))"
New-Item -ItemType Directory -Path $testRoot | Out-Null
try {
    $version = '1.2.3'
    $names = @(Get-StudioReleaseAssetNames -Version $version)
    $assets = @($names | ForEach-Object { New-TestAsset -Directory $testRoot -Name $_ })

    $draftRelease = [pscustomobject]@{
        draft = $true
        html_url = 'https://github.com/ZR233/pure-lang/releases/tag/untagged-0123456789abcdef'
    }
    Assert-StudioReleasePageUrl `
        -Release $draftRelease `
        -Repository 'ZR233/pure-lang' `
        -Tag "v$version"
    $publishedRelease = [pscustomobject]@{
        draft = $false
        html_url = "https://github.com/ZR233/pure-lang/releases/tag/v$version"
    }
    Assert-StudioReleasePageUrl `
        -Release $publishedRelease `
        -Repository 'ZR233/pure-lang' `
        -Tag "v$version"
    Assert-Throws `
        -Action {
            Assert-StudioReleasePageUrl `
                -Release $draftRelease `
                -Repository 'other/repository' `
                -Tag "v$version"
        } `
        -Message 'A draft URL from another repository must be rejected'
    $draftWithPublishedUrl = [pscustomobject]@{
        draft = $true
        html_url = "https://github.com/ZR233/pure-lang/releases/tag/v$version"
    }
    Assert-Throws `
        -Action {
            Assert-StudioReleasePageUrl `
                -Release $draftWithPublishedUrl `
                -Repository 'ZR233/pure-lang' `
                -Tag "v$version"
        } `
        -Message 'A draft must use its GitHub untagged URL'
    $publishedWithDraftUrl = [pscustomobject]@{
        draft = $false
        html_url = $draftRelease.html_url
    }
    Assert-Throws `
        -Action {
            Assert-StudioReleasePageUrl `
                -Release $publishedWithDraftUrl `
                -Repository 'ZR233/pure-lang' `
                -Tag "v$version"
        } `
        -Message 'A published Release must use its version tag URL'

    $emptyDraft = [pscustomobject]@{ draft = $true; assets = @() }
    $emptyPlan = Get-StudioReleaseAssetPlan -Release $emptyDraft -Version $version -AssetDirectory $testRoot
    Assert-Equal -Expected $names -Actual $emptyPlan.MissingNames -Message 'Empty draft must upload every asset'

    $partialDraft = [pscustomobject]@{ draft = $true; assets = @($assets[0], $assets[1]) }
    $partialPlan = Get-StudioReleaseAssetPlan -Release $partialDraft -Version $version -AssetDirectory $testRoot
    Assert-Equal -Expected $names[2..($names.Count - 1)] -Actual $partialPlan.MissingNames -Message 'Partial draft must upload only missing assets'

    $completeDraft = [pscustomobject]@{ draft = $true; assets = $assets }
    $completePlan = Get-StudioReleaseAssetPlan -Release $completeDraft -Version $version -AssetDirectory $testRoot
    Assert-Equal -Expected @() -Actual $completePlan.MissingNames -Message 'Complete draft must not upload assets again'
    $repeatPlan = Get-StudioReleaseAssetPlan -Release $completeDraft -Version $version -AssetDirectory $testRoot
    Assert-Equal -Expected @() -Actual $repeatPlan.MissingNames -Message 'Repeated reconciliation must be idempotent'

    $conflictingAssets = @($assets | ForEach-Object { $_.PSObject.Copy() })
    $conflictingAssets[$conflictingAssets.Count - 1].digest = "sha256:$('0' * 64)"
    $conflictingDraft = [pscustomobject]@{ draft = $true; assets = $conflictingAssets }
    Assert-Throws `
        -Action { Get-StudioReleaseAssetPlan -Release $conflictingDraft -Version $version -AssetDirectory $testRoot } `
        -Message 'Digest conflicts must be rejected'

    $publishedPartial = [pscustomobject]@{ draft = $false; assets = @($assets[0]) }
    Assert-Throws `
        -Action { Get-StudioReleaseAssetPlan -Release $publishedPartial -Version $version -AssetDirectory $testRoot } `
        -Message 'Published partial Releases must be rejected'

    $publishedComplete = [pscustomobject]@{ draft = $false; assets = $assets }
    $publishedPlan = Get-StudioReleaseAssetPlan -Release $publishedComplete -Version $version -AssetDirectory $testRoot
    Assert-Equal -Expected $true -Actual $publishedPlan.Complete -Message 'Published complete Release must be accepted'

    Write-Output 'Studio Release asset reconciliation tests passed.'
} finally {
    if (Test-Path -LiteralPath $testRoot) {
        Remove-Item -LiteralPath $testRoot -Recurse -Force
    }
}
