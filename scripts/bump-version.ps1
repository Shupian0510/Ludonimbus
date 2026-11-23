# Sets the version everywhere and adds an empty bilingual CHANGELOG section.
#
# Usage: ./scripts/bump-version.ps1 -Version 0.1.3
param([Parameter(Mandatory = $true)][string]$Version)
$ErrorActionPreference = "Stop"
$Version = $Version.TrimStart("v")
if ($Version -notmatch '^\d+\.\d+\.\d+$') { throw "Version must look like 1.2.3 (got '$Version')" }
$root = Split-Path -Parent $PSScriptRoot

function Set-Text($path, $text) {
    [IO.File]::WriteAllText($path, $text, [Text.UTF8Encoding]::new($false))
}

# Cargo workspace (all crates inherit it).
$cargoPath = Join-Path $root "Cargo.toml"
$cargo = Get-Content $cargoPath -Raw
Set-Text $cargoPath ([regex]::new('(?m)^version = "[^"]+"').Replace($cargo, "version = `"$Version`"", 1))

# Tauri app version (what the updater compares).
$tauriPath = Join-Path $root "app/src-tauri/tauri.conf.json"
$tauri = Get-Content $tauriPath -Raw
Set-Text $tauriPath ([regex]::new('"version":\s*"[^"]+"').Replace($tauri, "`"version`": `"$Version`"", 1))

# Frontend package + lock file.
Push-Location (Join-Path $root "app")
try {
    npm version $Version --no-git-tag-version --allow-same-version | Out-Null
    if ($LASTEXITCODE -ne 0) { throw "npm version failed" }
} finally { Pop-Location }

# Steam plugin: package + lock file, and the version Millennium shows.
Push-Location (Join-Path $root "plugin-millennium")
try {
    npm version $Version --no-git-tag-version --allow-same-version | Out-Null
    if ($LASTEXITCODE -ne 0) { throw "npm version failed in plugin-millennium" }
} finally { Pop-Location }
$tomlPath = Join-Path $root "plugin-millennium/millennium.toml"
$toml = Get-Content $tomlPath -Raw
Set-Text $tomlPath ([regex]::new('(?m)^version = "[^"]+"').Replace($toml, "version = `"$Version`"", 1))

# Refresh Cargo.lock for the new workspace version.
Push-Location $root
try { cargo metadata --format-version 1 | Out-Null } finally { Pop-Location }

# Bilingual CHANGELOG skeleton (\u4E2D\u6587 is the word for Chinese).
$changelogPath = Join-Path $root "CHANGELOG.md"
$changelog = Get-Content $changelogPath -Raw -Encoding utf8
if ($changelog -notmatch "(?m)^## $([regex]::Escape($Version))\s*$") {
    $zh = [regex]::Unescape("\u4E2D\u6587")
    $section = "## $Version`n`n### $zh`n`n- `n`n### English`n`n- `n`n"
    $idx = [regex]::Match($changelog, "(?m)^## ").Index
    if ($idx -le 0) { $changelog = $changelog.TrimEnd() + "`n`n" + $section } else { $changelog = $changelog.Insert($idx, $section) }
    Set-Text $changelogPath $changelog
    Write-Host "Added an empty '## $Version' section to CHANGELOG.md: fill in both languages."
}

& (Join-Path $PSScriptRoot "check-versions.ps1") | Out-Null
Write-Host "Version set to $Version."
