<#
.SYNOPSIS
  Check E1 of docs/plan-game-backup.md section 10: can Ludonimbus use game files
  that are already in your Quark / Baidu Netdisk instead of uploading them?

.DESCRIPTION
  Runs crates/core/tests/sources_live.rs with your real Ludonimbus settings and
  sign-ins (the cloud selected in Ludonimbus must be the one you test: switch in
  Settings -> Cloud first, and back afterwards). It checks:

    Quark (through the OpenList server Ludonimbus runs)
      - reading the drive's root and a folder of yours (-QuarkFolder), and which
        hash the files come with
      - uploading a 256 KB test file outside the Ludonimbus folder, copying it
        into the Ludonimbus folder on the server, reading the copy back
      - optional (-QuarkFile): copying one of your big files into the Ludonimbus
        folder and timing it. Near-instant = Quark copies on its servers;
        otherwise OpenList downloads and uploads it again

    Baidu Netdisk
      - whether the drive's root and a folder of yours (-BaiduFolder) can be
        listed (the app may be limited to "My app data/Ludonimbus"), and which
        hash and download link the files come with
      - whether creating a folder outside the app folder is refused
      - optional (-BaiduFile): copying one of your files into the app folder on
        the server

  Your files are only read (and copied). The check writes only to folders named
  "Ludonimbus-E1-..." and "e1-copy-...", and deletes them at the end, also when
  a step fails.

  The Baidu part needs the app keys: they are read from secrets.local.env in the
  repository (like build-release.ps1).

  A report is written to <WorkDir>\e1-report-<time>.txt. It lists up to 10 file
  names of each folder you name; remove them before sharing the report if you
  want.

.EXAMPLE
  ./scripts/cloud-sources-e1.ps1 -Quark -QuarkFolder "/游戏" -QuarkFile "/游戏/某游戏/game.iso"
.EXAMPLE
  ./scripts/cloud-sources-e1.ps1 -Baidu -BaiduFolder "/游戏" -BaiduFile "/游戏/小文件.zip"
#>
param(
    [switch]$Quark,
    [switch]$Baidu,
    [string]$QuarkFolder = "",
    [string]$QuarkFile = "",
    [string]$BaiduFolder = "",
    [string]$BaiduFile = "",
    [string]$WorkDir = (Join-Path $env:LOCALAPPDATA "Ludonimbus-e1")
)

$ErrorActionPreference = "Stop"
[Console]::OutputEncoding = [System.Text.Encoding]::UTF8

if (-not $Quark -and -not $Baidu) { throw "Pass -Quark and / or -Baidu." }

$Repo = Split-Path -Parent $PSScriptRoot
New-Item -ItemType Directory -Force $WorkDir | Out-Null
$ReportFile = Join-Path $WorkDir ("e1-report-{0}.txt" -f (Get-Date -Format "yyyyMMdd-HHmmss"))
$Report = [System.Collections.Generic.List[string]]::new()
$Report.Add("Ludonimbus E1 (game files already in the cloud) · $(Get-Date -Format 'yyyy-MM-dd HH:mm')")

# Baidu app keys, as build-release.ps1 reads them.
$secrets = Join-Path $Repo "secrets.local.env"
if ($Baidu -and (Test-Path $secrets)) {
    foreach ($line in Get-Content $secrets) {
        if ($line -match '^\s*(LUDONIMBUS_BAIDU_[A-Z_]+)\s*=\s*(.+?)\s*$') { Set-Item "env:$($Matches[1])" $Matches[2] }
    }
}

$vars = @{
    LUDONIMBUS_E1_QUARK        = $(if ($Quark) { "1" } else { "" })
    LUDONIMBUS_E1_QUARK_FOLDER = $QuarkFolder
    LUDONIMBUS_E1_QUARK_FILE   = $QuarkFile
    LUDONIMBUS_E1_BAIDU        = $(if ($Baidu) { "1" } else { "" })
    LUDONIMBUS_E1_BAIDU_FOLDER = $BaiduFolder
    LUDONIMBUS_E1_BAIDU_FILE   = $BaiduFile
}
foreach ($k in $vars.Keys) { Set-Item "env:$k" $vars[$k] }

$exitCode = 0
Push-Location $Repo
try {
    cargo test -q -p ludonimbus-core --test sources_live sources_live -- --ignored --nocapture 2>&1 | ForEach-Object {
        $line = "$_"
        Write-Host $line
        if ($line -match "^\[E1|panicked|FAILED|test result|error") { $Report.Add($line) }
    }
    if ($LASTEXITCODE -ne 0) { $exitCode = 1; $Report.Add("RESULT: FAILED (see above)") } else { $Report.Add("RESULT: DONE") }
} finally {
    Pop-Location
    foreach ($k in $vars.Keys) { Remove-Item "env:$k" -ErrorAction SilentlyContinue }
    $Report | Set-Content $ReportFile -Encoding utf8
    Write-Host ""
    Write-Host "Report: $ReportFile" -ForegroundColor Green
}
exit $exitCode
