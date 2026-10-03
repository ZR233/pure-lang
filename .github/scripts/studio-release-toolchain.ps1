Set-StrictMode -Version Latest

function Get-StudioReleaseRustToolchain {
    param(
        [Parameter(Mandatory)]
        [ValidatePattern('^[0-9a-f]{40}$')]
        [string] $Commit
    )

    $manifestPath = git ls-tree --name-only $Commit -- rust-toolchain.toml
    if ($LASTEXITCODE -ne 0) {
        throw "Failed to inspect Rust toolchain manifest at $Commit"
    }
    if ($manifestPath -ceq 'rust-toolchain.toml') {
        $manifest = git show "${Commit}:rust-toolchain.toml"
        if ($LASTEXITCODE -ne 0) {
            throw "Failed to read Rust toolchain manifest at $Commit"
        }
        $sections = [regex]::Matches(
            ($manifest -join "`n"),
            '(?ms)^\[toolchain\][ \t]*\r?\n(?<body>.*?)(?=^\[|\z)'
        )
        if ($sections.Count -ne 1) {
            throw "Expected one Rust toolchain section at $Commit"
        }
        $channels = [regex]::Matches(
            $sections[0].Groups['body'].Value,
            '(?m)^[ \t]*channel[ \t]*=[ \t]*"(?<version>[^"\r\n]+)"[ \t]*(?:#.*)?$'
        )
        if ($channels.Count -ne 1) {
            throw "Expected one pinned Rust channel at $Commit"
        }
        $versions = @($channels[0].Groups['version'].Value)
    } else {
        # Existing tags predating the manifest keep their original publisher pins.
        $workflow = git show "${Commit}:.github/workflows/studio-release-publish.yml"
        if ($LASTEXITCODE -ne 0) {
            throw "Release commit $Commit has no Rust toolchain manifest or publisher workflow"
        }
        $pins = [regex]::Matches(
            ($workflow -join "`n"),
            '(?m)^[ \t]+toolchain:[ \t]*(?<version>[^#\r\n]+?)[ \t]*(?:#.*)?$'
        )
        $versions = @($pins | ForEach-Object { $_.Groups['version'].Value })
    }

    if ($versions.Count -eq 0) {
        throw "Release commit $Commit does not pin a Rust toolchain"
    }
    foreach ($version in $versions) {
        if ($version -cnotmatch '^(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)$') {
            throw "Release commit $Commit has a non-exact Rust toolchain: $version"
        }
    }
    $uniqueVersions = @($versions | Sort-Object -Unique)
    if ($uniqueVersions.Count -ne 1) {
        throw "Release commit $Commit has conflicting Rust toolchains: $($uniqueVersions -join ', ')"
    }
    return $uniqueVersions[0]
}
