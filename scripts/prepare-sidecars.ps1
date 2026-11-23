# Prepares the executables that ship next to the Ludonimbus app (Tauri "externalBin"):
#   app/src-tauri/binaries/ludonimbus-run-<triple>.exe   (Steam launch wrapper, built here)
#   app/src-tauri/binaries/ludusavi-<triple>.exe         (official Ludusavi release, not built here)
#
# Usage:
#   ./scripts/prepare-sidecars.ps1                         # debug build, download Ludusavi if missing
#   ./scripts/prepare-sidecars.ps1 -Release                # release build
#   ./scripts/prepare-sidecars.ps1 -LudusaviPath C:\Downloads\ludusavi.exe
param(
    [switch]$Release,
    [string]$LudusaviPath = "",
    [string]$LudusaviVersion = "0.31.0"
)
$ErrorActionPreference = "Stop"
$root = Split-Path -Parent $PSScriptRoot
$bin = Join-Path $root "app/src-tauri/binaries"
New-Item -ItemType Directory -Force $bin | Out-Null
$triple = (rustc -vV | Select-String "^host: (.+)$").Matches[0].Groups[1].Value
$profile = if ($Release) { "release" } else { "debug" }

Write-Host "Building ludonimbus-run ($profile, $triple)..."
$cargoArgs = @("build", "-p", "ludonimbus-runner")
if ($Release) { $cargoArgs += "--release" }
& cargo @cargoArgs
if ($LASTEXITCODE -ne 0) { throw "cargo build failed" }
Copy-Item (Join-Path $root "target/$profile/ludonimbus-run.exe") (Join-Path $bin "ludonimbus-run-$triple.exe") -Force

$ludusaviOut = Join-Path $bin "ludusavi-$triple.exe"
if ($LudusaviPath) {
    Copy-Item $LudusaviPath $ludusaviOut -Force
} elseif (-not (Test-Path $ludusaviOut)) {
    $url = "https://github.com/mtkennerly/ludusavi/releases/download/v$LudusaviVersion/ludusavi-v$LudusaviVersion-win64.zip"
    $tmp = Join-Path ([IO.Path]::GetTempPath()) ("ludusavi-" + [guid]::NewGuid())
    New-Item -ItemType Directory $tmp | Out-Null
    Write-Host "Downloading Ludusavi v$LudusaviVersion from $url"
    Invoke-WebRequest $url -OutFile (Join-Path $tmp "ludusavi.zip")
    Write-Host ("SHA256: " + (Get-FileHash (Join-Path $tmp "ludusavi.zip") -Algorithm SHA256).Hash)
    Expand-Archive (Join-Path $tmp "ludusavi.zip") -DestinationPath $tmp
    Copy-Item (Join-Path $tmp "ludusavi.exe") $ludusaviOut -Force
    Remove-Item $tmp -Recurse -Force
}
Write-Host "Sidecars ready in $bin"
Get-ChildItem $bin -Filter *.exe | ForEach-Object { Write-Host ("  " + $_.Name) }
