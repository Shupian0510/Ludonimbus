# Fails unless Cargo.toml, app/package.json and app/src-tauri/tauri.conf.json
# carry the same version. Prints that version.
$ErrorActionPreference = "Stop"
$root = Split-Path -Parent $PSScriptRoot

$cargo = [regex]::Match((Get-Content (Join-Path $root "Cargo.toml") -Raw), '(?m)^version = "([^"]+)"').Groups[1].Value
$npm = (Get-Content (Join-Path $root "app/package.json") -Raw | ConvertFrom-Json).version
$tauri = (Get-Content (Join-Path $root "app/src-tauri/tauri.conf.json") -Raw | ConvertFrom-Json).version

if (-not $cargo -or $cargo -ne $npm -or $cargo -ne $tauri) {
    throw "Version mismatch: Cargo.toml=$cargo, app/package.json=$npm, tauri.conf.json=$tauri (use scripts/bump-version.ps1)"
}
$cargo
