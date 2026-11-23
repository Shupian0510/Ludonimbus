# Builds the Millennium plugin (plugin-millennium/) and copies it to
#   app/src-tauri/plugin/ludonimbus.star
# where the app bundles it as a resource. Needs Node.js (npm).
#
# Usage: ./scripts/build-plugin.ps1
$ErrorActionPreference = "Stop"
$root = Split-Path -Parent $PSScriptRoot
$dir = Join-Path $root "plugin-millennium"
$out = Join-Path $root "app/src-tauri/plugin"

Push-Location $dir
try {
    # `npm ci` also runs `starlight lsp` (the "prepare" script), which
    # generates the Millennium and backend type definitions.
    npm ci --no-audit --no-fund
    if ($LASTEXITCODE -ne 0) { throw "npm ci failed in plugin-millennium" }
    npx tsc -p .
    if ($LASTEXITCODE -ne 0) { throw "plugin type check failed" }
    New-Item -ItemType Directory -Force (Join-Path $dir "dist") | Out-Null
    npx starlight pack --release
    if ($LASTEXITCODE -ne 0) { throw "starlight pack failed" }
} finally {
    Pop-Location
}

New-Item -ItemType Directory -Force $out | Out-Null
Copy-Item (Join-Path $dir "dist/ludonimbus.star") (Join-Path $out "ludonimbus.star") -Force
Write-Host "Steam plugin ready: $(Join-Path $out 'ludonimbus.star')"
