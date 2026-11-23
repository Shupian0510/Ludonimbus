# Builds the signed release installer (NSIS) locally, with the Google OAuth
# client baked in. Official releases are built by .github/workflows/release.yml.
#
# Values come from the environment, or from the git-ignored secrets.local.env
# file in the repository root:
#   LUDONIMBUS_GOOGLE_CLIENT_ID=xxxx.apps.googleusercontent.com
#   LUDONIMBUS_GOOGLE_CLIENT_SECRET=GOCSPX-xxxx
#   TAURI_SIGNING_PRIVATE_KEY_PASSWORD=...      (updater signing key password)
# The updater signing key defaults to %USERPROFILE%\.tauri\ludonimbus.key.
#
# Usage: ./scripts/build-release.ps1 [-LudusaviPath C:\path\to\ludusavi.exe]
param([string]$LudusaviPath = "")
$ErrorActionPreference = "Stop"
$root = Split-Path -Parent $PSScriptRoot

$secrets = Join-Path $root "secrets.local.env"
if (Test-Path $secrets) {
    foreach ($line in Get-Content $secrets) {
        if ($line -match '^\s*((?:LUDONIMBUS|TAURI)_[A-Z0-9_]+)\s*=\s*(.+?)\s*$') {
            Set-Item "env:$($Matches[1])" $Matches[2]
        }
    }
}
if ($env:LUDONIMBUS_GOOGLE_CLIENT_ID) {
    Write-Host "Google OAuth client will be built in."
} else {
    Write-Warning "LUDONIMBUS_GOOGLE_CLIENT_ID is not set: users will have to enter their own Client ID."
}

$defaultKey = Join-Path $env:USERPROFILE ".tauri/ludonimbus.key"
if (-not $env:TAURI_SIGNING_PRIVATE_KEY -and (Test-Path $defaultKey)) {
    $env:TAURI_SIGNING_PRIVATE_KEY = $defaultKey
}
if (-not $env:TAURI_SIGNING_PRIVATE_KEY) {
    throw "No updater signing key: set TAURI_SIGNING_PRIVATE_KEY or create $defaultKey (npx tauri signer generate)"
}

# The ids are read at compile time (option_env!), so rebuild the core crate.
cargo clean -p ludonimbus-core --release
if ($LASTEXITCODE -ne 0) { throw "cargo clean failed" }

& (Join-Path $PSScriptRoot "prepare-sidecars.ps1") -Release -LudusaviPath $LudusaviPath

Push-Location (Join-Path $root "app")
try {
    if (-not (Test-Path "node_modules")) { npm ci }
    npx tauri build --config src-tauri/tauri.release.conf.json
    if ($LASTEXITCODE -ne 0) { throw "tauri build failed" }
} finally {
    Pop-Location
}
Get-ChildItem (Join-Path $root "target/release/bundle/nsis") -Filter *.exe | ForEach-Object { Write-Host ("Installer: " + $_.FullName) }
