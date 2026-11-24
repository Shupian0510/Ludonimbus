<#
.SYNOPSIS
  Screenshots of the app for the documentation site, from the browser
  preview's sample data.

.DESCRIPTION
  Uses the Vite dev server of app/ (sample data, started if nothing listens
  on -Port), opens every scene (?scene=…, see app/src/scene.ts) in headless
  Microsoft Edge in Chinese and English, and saves
  website/public/screenshots/<lang>/<scene>.png. Re-run it after UI changes.

  Screens that need the real Steam / Windows / Quark (play bar status,
  SmartScreen, the Quark sign-in window…) are listed in website/SCREENSHOTS.md
  and taken by hand.

.EXAMPLE
  ./scripts/docs-screenshots.ps1
.EXAMPLE
  ./scripts/docs-screenshots.ps1 -Scenes settings-cloud-quark,guide-welcome -Out C:\path\to\website\public\screenshots
#>
param(
    [string]$Out = (Join-Path (Split-Path -Parent $PSScriptRoot) "website\public\screenshots"),
    [string[]]$Scenes = @(),
    [string[]]$Langs = @("zh-CN", "en"),
    [int]$Port = 1420,
    [int]$Width = 1280,
    [int]$Height = 820
)

$ErrorActionPreference = "Stop"
$Repo = Split-Path -Parent $PSScriptRoot

$all = @(
    # Library, game page
    "library-grid", "game-detail", "game-detail-steam", "game-detail-conflict", "game-detail-pending",
    "versions", "game-settings", "art-picker",
    # Settings
    "settings", "settings-cloud", "settings-cloud-google", "settings-cloud-folder", "settings-cloud-migrate",
    "settings-cloud-webdav", "settings-cloud-s3", "settings-cloud-quark", "settings-language", "settings-sgdb",
    "settings-steam-plugin",
    # Add game
    "add-game-scan", "add-game-scan-folder", "add-game-cloud", "add-game-manual", "save-finder", "add-to-steam",
    # First-run guide
    "guide-welcome", "guide-usage", "guide-cloud", "guide-art", "guide-steam", "guide-done"
)
if ($Scenes.Count -eq 0) { $Scenes = $all }

$edge = @(
    "${env:ProgramFiles(x86)}\Microsoft\Edge\Application\msedge.exe",
    "$env:ProgramFiles\Microsoft\Edge\Application\msedge.exe"
) | Where-Object { Test-Path $_ } | Select-Object -First 1
if (-not $edge) { throw "Microsoft Edge not found" }

function Test-Port([int]$p) {
    # Vite listens on "localhost", which may be IPv6 only.
    try { (New-Object Net.Sockets.TcpClient("localhost", $p)).Close(); return $true } catch { return $false }
}

$server = $null
if (-not (Test-Port $Port)) {
    Write-Host "Starting the dev server (app/, sample data)…"
    $server = Start-Process cmd.exe -ArgumentList "/c", "npm", "run", "dev" -WorkingDirectory (Join-Path $Repo "app") -WindowStyle Hidden -PassThru
    for ($i = 0; $i -lt 60 -and -not (Test-Port $Port); $i++) { Start-Sleep -Milliseconds 500 }
    if (-not (Test-Port $Port)) { throw "The dev server didn't start on port $Port" }
}

$profile = Join-Path $env:TEMP "ludonimbus-shots-profile"
try {
    foreach ($lang in $Langs) {
        $dir = Join-Path $Out $lang
        New-Item -ItemType Directory -Force $dir | Out-Null
        foreach ($scene in $Scenes) {
            $file = Join-Path $dir "$scene.png"
            $url = "http://localhost:$Port/?scene=$scene&lang=$lang"
            # A fresh profile each time: nothing remembered between screens.
            if (Test-Path $profile) { Remove-Item -Recurse -Force $profile }
            & $edge --headless=new --disable-gpu --hide-scrollbars --no-first-run --mute-audio `
                "--user-data-dir=$profile" "--window-size=$Width,$Height" --force-device-scale-factor=1 `
                --virtual-time-budget=8000 "--screenshot=$file" $url 2>$null | Out-Null
            if (Test-Path $file) { Write-Host ("{0,-6} {1}" -f $lang, $scene) } else { Write-Warning "no screenshot: $lang $scene" }
        }
    }
} finally {
    # cmd → npm → node: stop the whole tree.
    if ($server) { & taskkill /PID $server.Id /T /F 2>$null | Out-Null }
    if (Test-Path $profile) { Remove-Item -Recurse -Force $profile -ErrorAction SilentlyContinue }
}
Write-Host "Screenshots: $Out"
