# The two steps of a release that are done by hand (docs/release-process.md).
#
#   ./scripts/release.ps1        on dev: checks everything, then opens the
#                                pull request dev -> main ("Release X.Y.Z").
#                                Merge it with "Create a merge commit".
#   ./scripts/release.ps1 -Tag   on main, after that pull request (or a
#                                hotfix) is merged: tags main and pushes the
#                                tag. GitHub Actions
#                                (.github/workflows/release.yml) builds,
#                                signs and publishes.
param([switch]$Tag, [switch]$SkipTests)
$ErrorActionPreference = "Stop"
$root = Split-Path -Parent $PSScriptRoot
$expected = if ($Tag) { "main" } else { "dev" }
Push-Location $root
try {
    $branch = (git rev-parse --abbrev-ref HEAD).Trim()
    if ($branch -ne $expected) { throw "Run this on $expected (currently on '$branch')" }
    if (git status --porcelain) { throw "Commit or stash your changes first (working tree is not clean)" }
    git fetch origin --tags --quiet
    if ((git rev-parse HEAD).Trim() -ne (git rev-parse "origin/$expected").Trim()) {
        throw "$expected is not the same as origin/$expected; pull (or push through a pull request) first"
    }

    $version = & (Join-Path $PSScriptRoot "check-versions.ps1")
    $tagName = "v$version"
    if (git tag -l $tagName) { throw "Tag $tagName already exists; set the next version first (scripts/bump-version.ps1)" }
    $notes = (& (Join-Path $PSScriptRoot "changelog-section.ps1") -Version $version) -join "`n"

    if ($Tag) {
        git tag -a $tagName -m "Ludonimbus $version"
        git push origin $tagName
        Write-Host "Pushed $tagName. Follow the build at https://github.com/Shupian0510/Ludonimbus/actions/workflows/release.yml"
        Write-Host "Afterwards: merge main back into dev and set the next version (docs/release-process.md)."
        return
    }

    if (-not $SkipTests) {
        cargo test --workspace --exclude ludonimbus-app
        if ($LASTEXITCODE -ne 0) { throw "Rust tests failed" }
        npm --prefix app run build
        if ($LASTEXITCODE -ne 0) { throw "Frontend build failed" }
    }

    $body = Join-Path ([IO.Path]::GetTempPath()) "ludonimbus-release-$version.md"
    [IO.File]::WriteAllText($body, $notes, [Text.UTF8Encoding]::new($false))
    gh pr create --base main --head dev --title "Release $version" --body-file $body
    if ($LASTEXITCODE -ne 0) { throw "gh pr create failed" }
    Write-Host "Merge it with 'Create a merge commit' once the checks pass, then on main: ./scripts/release.ps1 -Tag"
} finally {
    Pop-Location
}
