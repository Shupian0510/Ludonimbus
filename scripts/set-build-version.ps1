# Gives a build that is not a release its own version: the version in the
# tree plus a suffix, e.g. 1.0.0-dev.57 or 1.0.0-rc.3. Prints it.
#
# Only Cargo.toml (what the programs report) and tauri.conf.json (the
# installer's name, and what the updater compares) are changed. Run it right
# before building and don't commit the result.
#
# Such a build is older than the release of the same version
# (1.0.0-dev.57 < 1.0.0): the updater offers it that release, and never an
# older one.
#
# Usage: ./scripts/set-build-version.ps1 -Suffix dev.57
param([Parameter(Mandatory = $true)][string]$Suffix)
$ErrorActionPreference = "Stop"
if ($Suffix -notmatch '^[0-9A-Za-z]+(\.(0|[1-9][0-9]*|[0-9]*[A-Za-z][0-9A-Za-z]*))*$') {
    throw "The suffix must look like dev.57 or rc.3 (got '$Suffix')"
}
$root = Split-Path -Parent $PSScriptRoot
$base = & (Join-Path $PSScriptRoot "check-versions.ps1")
$version = "$base-$Suffix"

function Set-Text($path, $text) {
    [IO.File]::WriteAllText($path, $text, [Text.UTF8Encoding]::new($false))
}

$cargoPath = Join-Path $root "Cargo.toml"
$cargo = Get-Content $cargoPath -Raw
Set-Text $cargoPath ([regex]::new('(?m)^version = "[^"]+"').Replace($cargo, "version = `"$version`"", 1))

$tauriPath = Join-Path $root "app/src-tauri/tauri.conf.json"
$tauri = Get-Content $tauriPath -Raw
Set-Text $tauriPath ([regex]::new('"version":\s*"[^"]+"').Replace($tauri, "`"version`": `"$version`"", 1))

# Cargo.lock records the workspace's own version too (builds use --locked).
Push-Location $root
try {
    cargo metadata --format-version 1 | Out-Null
    if ($LASTEXITCODE -ne 0) { throw "cargo metadata failed" }
} finally { Pop-Location }

$version
