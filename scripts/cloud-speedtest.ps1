<#
.SYNOPSIS
  Upload / download speed test of the cloud Ludonimbus is connected to
  (docs/plan-game-backup.md section 1).

.DESCRIPTION
  Builds the developer command line (crates/cli) and runs `ludonimbus speedtest`
  with your real Ludonimbus settings: the cloud selected in Settings and its
  saved sign-in. It writes random test files (8 MB each, 200 MB per round by
  default) to the root of the Ludonimbus folder in that cloud, first one at a
  time and then several in parallel, reads them back the same two ways, and
  deletes them. Each round stops early after -MaxSecs, so slow clouds (Baidu
  Netdisk without membership downloads at about 80 KB/s) finish in minutes.

  Your saves and other files are not touched. Files named
  ".ludonimbus-speed-..." left by an interrupted run are deleted the next time.

  To test another cloud, switch to it in Ludonimbus (Settings -> Cloud), run the
  script again, then switch back. Close the Ludonimbus window first if you
  switch clouds, so it doesn't sync in between.

  A report (text and JSON, no passwords or tokens) is written to
  <WorkDir>\speedtest-<cloud>-<time>.txt / .json.

.EXAMPLE
  ./scripts/cloud-speedtest.ps1                     # 200 MB per round, 4 parallel workers
.EXAMPLE
  ./scripts/cloud-speedtest.ps1 -Mb 500 -Threads 8  # more data, more workers
.EXAMPLE
  ./scripts/cloud-speedtest.ps1 -Threads 2,4,8      # compare worker counts (one run each)
#>
param(
    [string]$WorkDir = (Join-Path $env:LOCALAPPDATA "Ludonimbus-speedtest"),
    [int]$Mb = 200,
    [int]$ObjectMb = 8,
    [int[]]$Threads = @(4),
    [int]$MaxSecs = 90
)

$ErrorActionPreference = "Stop"
# cargo and the command line print UTF-8 (Chinese messages).
[Console]::OutputEncoding = [System.Text.Encoding]::UTF8

$Repo = Split-Path -Parent $PSScriptRoot
New-Item -ItemType Directory -Force $WorkDir | Out-Null

Write-Host "== Building the Ludonimbus command line (release)" -ForegroundColor Cyan
Push-Location $Repo
try {
    cargo build -q -p ludonimbus-cli --release
    if ($LASTEXITCODE -ne 0) { throw "cargo build failed" }
} finally {
    Pop-Location
}
$Exe = Join-Path $Repo "target\release\ludonimbus.exe"

$exitCode = 0
foreach ($t in $Threads) {
    Write-Host ""
    Write-Host "== Speed test: $Mb MB per round, $ObjectMb MB objects, $t parallel workers, at most $MaxSecs s per round" -ForegroundColor Cyan
    $json = & $Exe speedtest --mb $Mb --object-mb $ObjectMb --threads $t --max-secs $MaxSecs --json
    if ($LASTEXITCODE -ne 0) {
        $exitCode = 1
        Write-Host "RESULT: FAILED (see the message above)" -ForegroundColor Red
        continue
    }
    $text = $json -join "`n"
    $r = $text | ConvertFrom-Json
    $stamp = Get-Date -Format "yyyyMMdd-HHmmss"
    $base = Join-Path $WorkDir "speedtest-$($r.kind)-t$t-$stamp"
    $text | Set-Content "$base.json" -Encoding utf8

    function Rate($p) {
        if (-not $p) { return "-" }
        $line = "{0,8:N2} MB/s  ({1} x {2} MB in {3:N1} s)" -f ($p.bytesPerSec / 1MB), $p.objects, ($p.bytes / $p.objects / 1MB), $p.secs
        if ($p.objects -eq 0) { $line = "       -  (nothing finished in time)" }
        if ($p.capped) { $line += "  [stopped at the time limit]" }
        if ($p.error) { $line += "  ERROR: $($p.error)" }
        return $line
    }
    function Hours([double]$gb, [double]$bps) {
        if ($bps -le 0) { return "-" }
        return "{0:N1} h" -f ($gb * 1GB / $bps / 3600)
    }
    $lines = @(
        "Ludonimbus cloud speed test · $(Get-Date -Format 'yyyy-MM-dd HH:mm') · $($r.kind)",
        ("small file          write {0:N0} ms, read {1:N0} ms (fixed cost of one request)" -f $r.smallWriteMs, $r.smallReadMs),
        "upload, 1 worker    $(Rate $r.uploadSerial)",
        "upload, $t workers   $(Rate $r.uploadParallel)",
        "download, 1 worker  $(Rate $r.downloadSerial)",
        "download, $t workers $(Rate $r.downloadParallel)",
        "estimate            10 GB: up $(Hours 10 $r.uploadBps), down $(Hours 10 $r.downloadBps); 50 GB: up $(Hours 50 $r.uploadBps), down $(Hours 50 $r.downloadBps)",
        "verdict             $($r.verdict)"
    )
    foreach ($n in $r.notes) { $lines += "note                $n" }
    if ($r.cleanupError) { $lines += "CLEANUP FAILED      $($r.cleanupError)" }
    $lines | ForEach-Object { Write-Host $_ }
    $lines | Set-Content "$base.txt" -Encoding utf8
    Write-Host ""
    Write-Host "Report: $base.txt" -ForegroundColor Green
}
exit $exitCode
