# Checks everything locally, then tags and pushes the release.
# GitHub Actions (.github/workflows/release.yml) builds, signs and publishes it.
#
# Usage:
#   ./scripts/bump-version.ps1 -Version 0.1.3   # then fill in CHANGELOG.md and commit
#   ./scripts/release.ps1
param([switch]$SkipTests)
$ErrorActionPreference = "Stop"
$root = Split-Path -Parent $PSScriptRoot
Push-Location $root
try {
    $branch = (git rev-parse --abbrev-ref HEAD).Trim()
    if ($branch -ne "main") { throw "Releases are made from main (currently on '$branch')" }
    if (git status --porcelain) { throw "Commit or stash your changes first (working tree is not clean)" }
    git fetch origin main --quiet
    $behind = [int](git rev-list --count HEAD..origin/main)
    if ($behind -gt 0) { throw "main is $behind commit(s) behind origin/main; pull first" }

    $version = & (Join-Path $PSScriptRoot "check-versions.ps1")
    $tag = "v$version"
    if (git tag -l $tag) { throw "Tag $tag already exists; bump the version first (scripts/bump-version.ps1)" }
    & (Join-Path $PSScriptRoot "changelog-section.ps1") -Version $version | Out-Null

    if (-not $SkipTests) {
        cargo test --workspace --exclude ludonimbus-app
        if ($LASTEXITCODE -ne 0) { throw "Rust tests failed" }
        npm --prefix app run build
        if ($LASTEXITCODE -ne 0) { throw "Frontend build failed" }
    }

    git tag -a $tag -m "Ludonimbus $version"
    git push origin main
    git push origin $tag
    Write-Host "Pushed $tag. Follow the build at https://github.com/Shupian0510/Ludonimbus/actions/workflows/release.yml"
} finally {
    Pop-Location
}
