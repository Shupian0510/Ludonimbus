# Fails unless Cargo.toml, app/package.json, app/src-tauri/tauri.conf.json and
# the Steam plugin (plugin-millennium/package.json, millennium.toml) carry the
# same version. Prints that version.
$ErrorActionPreference = "Stop"
$root = Split-Path -Parent $PSScriptRoot

$cargo = [regex]::Match((Get-Content (Join-Path $root "Cargo.toml") -Raw), '(?m)^version = "([^"]+)"').Groups[1].Value
$npm = (Get-Content (Join-Path $root "app/package.json") -Raw | ConvertFrom-Json).version
$tauri = (Get-Content (Join-Path $root "app/src-tauri/tauri.conf.json") -Raw | ConvertFrom-Json).version
$pluginNpm = (Get-Content (Join-Path $root "plugin-millennium/package.json") -Raw | ConvertFrom-Json).version
$pluginToml = [regex]::Match((Get-Content (Join-Path $root "plugin-millennium/millennium.toml") -Raw), '(?m)^version = "([^"]+)"').Groups[1].Value

if (-not $cargo -or $cargo -ne $npm -or $cargo -ne $tauri -or $cargo -ne $pluginNpm -or $cargo -ne $pluginToml) {
    throw "Version mismatch: Cargo.toml=$cargo, app/package.json=$npm, tauri.conf.json=$tauri, plugin package.json=$pluginNpm, millennium.toml=$pluginToml (use scripts/bump-version.ps1)"
}
$cargo
