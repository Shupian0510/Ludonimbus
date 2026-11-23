<#
.SYNOPSIS
  Quark + OpenList real-account check (docs/plan-phase3.md section 1.4).

.DESCRIPTION
  Downloads OpenList (pinned version, sha256 checked), starts it on 127.0.0.1
  only, mounts Quark with the cookie you paste, creates a WebDAV user limited to
  one test folder, then runs Ludonimbus's own WebDAV code against it
  (crates/core/tests/webdav_live.rs): connection test, small writes and
  overwrites, a large upload/download, snapshots, notes, devices, deletes.

  Nothing outside the "Ludonimbus-E0" folder in your Quark drive is touched,
  and the test deletes what it wrote. The OpenList data folder holds your
  Quark login, so it is deleted at the end unless you pass -Keep.

  A report without cookies or passwords is written to <WorkDir>\e0-report.txt.

.EXAMPLE
  ./scripts/quark-e0.ps1                 # full check with your Quark account
.EXAMPLE
  ./scripts/quark-e0.ps1 -Keep           # keep the login to re-check later
  ./scripts/quark-e0.ps1 -Recheck        # days later: is the login still valid?
.EXAMPLE
  ./scripts/quark-e0.ps1 -SelfTest       # OpenList's local storage instead of Quark (checks the script only)
#>
param(
    [string]$WorkDir = (Join-Path $env:LOCALAPPDATA "Ludonimbus-e0"),
    [int]$Port = 5245,
    [int]$BigMB = 10,
    [switch]$SelfTest,
    [switch]$Keep,
    [switch]$Recheck
)

$ErrorActionPreference = "Stop"
$ProgressPreference = "SilentlyContinue"

$Version = "v4.2.10"
$Asset = "openlist-windows-amd64.zip"
$Sha256 = "e5317fb186a541558421ffebd4b0b9579d0292f9e884025f0b85994bbf6c7c81"
$DownloadUrl = "https://github.com/OpenListTeam/OpenList/releases/download/$Version/$Asset"

$Repo = Split-Path -Parent $PSScriptRoot
$Bin = Join-Path $WorkDir "bin-$Version"
$Exe = Join-Path $Bin "openlist.exe"
$Data = Join-Path $WorkDir "data"
$Zip = Join-Path $WorkDir $Asset
$ReportFile = Join-Path $WorkDir "e0-report.txt"
$StateFile = Join-Path $Data "e0-state.json"
$Base = "http://127.0.0.1:$Port"
$TestFolder = "Ludonimbus-E0"
$Report = [System.Collections.Generic.List[string]]::new()

function Say([string]$text) { Write-Host $text; $Report.Add($text) }
function Step([string]$text) { Write-Host ""; Write-Host "== $text" -ForegroundColor Cyan; $Report.Add("== $text") }
function NewSecret { -join ((48..57) + (65..90) + (97..122) | Get-Random -Count 24 | ForEach-Object { [char]$_ }) }

function Api([string]$method, [string]$path, $body = $null, [string]$token = "") {
    $headers = @{}
    if ($token) { $headers["Authorization"] = $token }
    $req = @{ Method = $method; Uri = "$Base$path"; Headers = $headers }
    if ($null -ne $body) {
        $req["ContentType"] = "application/json; charset=utf-8"
        $req["Body"] = [Text.Encoding]::UTF8.GetBytes(($body | ConvertTo-Json -Depth 10 -Compress))
    }
    $r = Invoke-RestMethod @req
    if ($r.code -ne 200) { throw "OpenList $path -> $($r.code) $($r.message)" }
    return $r.data
}

function WaitReady {
    for ($i = 0; $i -lt 60; $i++) {
        try {
            if ((Invoke-WebRequest "$Base/ping" -UseBasicParsing -TimeoutSec 2).Content -match "pong") { return }
        } catch { }
        Start-Sleep -Milliseconds 500
    }
    throw "OpenList did not start within 30 seconds; see $Data\log"
}

function StartServer {
    $p = Start-Process -FilePath $Exe -ArgumentList @("server", "--data", "`"$Data`"") -WindowStyle Hidden -PassThru `
        -RedirectStandardOutput (Join-Path $WorkDir "server.out.txt") -RedirectStandardError (Join-Path $WorkDir "server.err.txt")
    $sw = [Diagnostics.Stopwatch]::StartNew()
    WaitReady
    Say ("OpenList started in {0:N1} s (pid {1})" -f $sw.Elapsed.TotalSeconds, $p.Id)
    return $p
}

function ReadCookie {
    Write-Host ""
    Write-Host "Paste your Quark cookie (it is not shown and not saved in the report):" -ForegroundColor Yellow
    Write-Host "  1. Open https://pan.quark.cn in Edge or Chrome and sign in (scan the QR code with the Quark app)."
    Write-Host "  2. Press F12 -> Network, then refresh the page (F5)."
    Write-Host "  3. Click a request to drive-pc.quark.cn (for example one starting with 'sort?')."
    Write-Host "  4. Headers -> Request Headers -> Cookie: copy the whole value."
    $secure = Read-Host -AsSecureString "Cookie"
    $plain = [Runtime.InteropServices.Marshal]::PtrToStringBSTR([Runtime.InteropServices.Marshal]::SecureStringToBSTR($secure))
    if ($plain -notmatch "__pus|__puus") { Write-Host "Warning: the cookie has no __pus / __puus value; it may not be a signed-in cookie." -ForegroundColor Yellow }
    return $plain.Trim()
}

$server = $null
$exitCode = 0
try {
    New-Item -ItemType Directory -Force $WorkDir | Out-Null
    Say "Ludonimbus Quark E0 · $(Get-Date -Format 'yyyy-MM-dd HH:mm') · OpenList $Version · $(if ($SelfTest) { 'self-test (local storage)' } else { 'Quark' })"

    # --- 1. OpenList ---------------------------------------------------------
    Step "1. OpenList $Version"
    if (-not (Test-Path $Exe)) {
        if (-not (Test-Path $Zip) -or (Get-FileHash $Zip -Algorithm SHA256).Hash -ne $Sha256.ToUpper()) {
            Say "Downloading $DownloadUrl (about 60 MB)…"
            Invoke-WebRequest $DownloadUrl -OutFile $Zip -UseBasicParsing
        }
        $hash = (Get-FileHash $Zip -Algorithm SHA256).Hash
        if ($hash -ne $Sha256.ToUpper()) { throw "sha256 mismatch: $hash (expected $Sha256)" }
        Say "sha256 OK"
        Expand-Archive $Zip -DestinationPath $Bin -Force
    }
    Say ("openlist.exe: {0}" -f (& $Exe version 2>&1 | Select-String -Pattern "Version" | Select-Object -First 1))

    if ($Recheck) {
        # --- Recheck: same data, no new cookie -----------------------------------
        if (-not (Test-Path $StateFile)) { throw "Nothing to re-check: run once with -Keep first." }
        $state = Get-Content $StateFile -Raw | ConvertFrom-Json
        Step "2. Start the kept OpenList (set up $($state.created))"
        $server = StartServer
        $token = (Api POST "/api/auth/login" @{ username = "admin"; password = $state.admin }).token
        $storage = (Api GET "/api/admin/storage/list" $null $token).content | Where-Object { $_.mount_path -eq "/quark" }
        Say "storage status: $($storage.status)"
        if ($storage.status -ne "work") { throw "The Quark login is no longer valid (status: $($storage.status))." }
        $davUser = $state.davUser; $davPass = $state.davPass
    } else {
        # --- 2. Fresh data folder, admin password, listen on 127.0.0.1 -------------
        Step "2. Configure OpenList (127.0.0.1:$Port only)"
        if (Test-Path $Data) { Remove-Item -Recurse -Force $Data }
        New-Item -ItemType Directory -Force $Data | Out-Null
        $admin = NewSecret
        # Any command creates the default config.json and database.
        & $Exe admin --data $Data *> (Join-Path $WorkDir "admin.out.txt")
        $configPath = Join-Path $Data "config.json"
        if (-not (Test-Path $configPath)) { throw "OpenList did not create config.json; see $WorkDir\admin.out.txt" }
        $config = Get-Content $configPath -Raw | ConvertFrom-Json
        $config | Add-Member -Force -NotePropertyName force -NotePropertyValue $true
        $config.scheme.address = "127.0.0.1"
        $config.scheme.http_port = $Port
        $config.scheme.https_port = -1
        $config | ConvertTo-Json -Depth 10 | Set-Content $configPath -Encoding utf8
        Say "config.json: force=true, address=127.0.0.1, http_port=$Port"

        # Since 4.2 OpenList creates the admin user on first start only when
        # OPENLIST_ADMIN_PASSWORD is set (otherwise it waits for its web setup page).
        $env:OPENLIST_ADMIN_PASSWORD = $admin
        try { $server = StartServer } finally { Remove-Item Env:OPENLIST_ADMIN_PASSWORD -ErrorAction SilentlyContinue }
        $token = (Api POST "/api/auth/login" @{ username = "admin"; password = $admin }).token
        Say "admin login OK"

        # --- 3. Mount ---------------------------------------------------------------
        Step "3. Mount $(if ($SelfTest) { 'local storage' } else { 'Quark' }) at /quark"
        $storage = @{
            mount_path = "/quark"; order = 0; remark = "Ludonimbus E0"
            cache_expiration = 0          # other devices write too: never serve a cached listing
            web_proxy = $true; webdav_policy = "native_proxy"; down_proxy_url = ""
        }
        if ($SelfTest) {
            $local = Join-Path $WorkDir "local-storage"
            New-Item -ItemType Directory -Force $local | Out-Null
            $storage.driver = "Local"
            $storage.addition = (@{ root_folder_path = $local } | ConvertTo-Json -Compress)
        } else {
            $cookie = ReadCookie
            $storage.driver = "Quark"
            $storage.addition = (@{
                    cookie = $cookie; root_folder_id = "0"; order_by = "none"; order_direction = "asc"
                    use_transcoding_address = $false; only_list_video_file = $false
                } | ConvertTo-Json -Compress)
            $cookie = $null
        }
        $sw = [Diagnostics.Stopwatch]::StartNew()
        try { Api POST "/api/admin/storage/create" $storage $token | Out-Null }
        catch { throw "Could not mount: $($_.Exception.Message) (is the cookie from a signed-in page?)" }
        $mounted = (Api GET "/api/admin/storage/list" $null $token).content | Where-Object { $_.mount_path -eq "/quark" }
        Say ("storage status: {0} ({1:N1} s)" -f $mounted.status, $sw.Elapsed.TotalSeconds)
        if ($mounted.status -ne "work") { throw "Mount failed: $($mounted.status)" }

        # --- 4. Test folder + WebDAV user limited to it -------------------------------
        Step "4. Folder /quark/$TestFolder and a WebDAV-only user"
        try { Api POST "/api/fs/mkdir" @{ path = "/quark/$TestFolder" } $token | Out-Null; Say "folder created" }
        catch { Say "mkdir: $($_.Exception.Message) (fine if it already exists)" }
        $davUser = "ludonimbus-e0"
        $davPass = NewSecret
        # bits: 3 mkdir/upload, 4 rename, 5 move, 7 remove, 8 webdav read, 9 webdav write
        $perm = (1 -shl 3) -bor (1 -shl 4) -bor (1 -shl 5) -bor (1 -shl 7) -bor (1 -shl 8) -bor (1 -shl 9)
        Api POST "/api/admin/user/create" @{
            username = $davUser; password = $davPass; base_path = "/quark/$TestFolder"; role = 0; permission = $perm; disabled = $false
        } $token | Out-Null
        Say "user ${davUser}: base path /quark/$TestFolder, permission $perm"

        if ($Keep) {
            @{ created = (Get-Date -Format "yyyy-MM-dd HH:mm"); admin = $admin; davUser = $davUser; davPass = $davPass } |
                ConvertTo-Json | Set-Content $StateFile -Encoding utf8
        }
    }

    # --- 5. Ludonimbus's WebDAV code against it ---------------------------------------
    Step "5. Ludonimbus WebDAV checks ($BigMB MB large file)"
    $env:LUDONIMBUS_DAV_URL = "$Base/dav/"
    $env:LUDONIMBUS_DAV_USER = $davUser
    $env:LUDONIMBUS_DAV_PASS = $davPass
    $env:LUDONIMBUS_DAV_BIG_MB = "$BigMB"
    Push-Location $Repo
    try {
        cargo test -q -p ludonimbus-core --test webdav_live -- --ignored --nocapture 2>&1 | ForEach-Object {
            $line = "$_"
            Write-Host $line
            if ($line -match "^\[|PASSED|panicked|assert|Error|error|FAILED|test result") { $Report.Add($line) }
        }
        $testExit = $LASTEXITCODE
    } finally {
        Pop-Location
        Remove-Item Env:LUDONIMBUS_DAV_PASS -ErrorAction SilentlyContinue
    }

    # --- 6. Resources ---------------------------------------------------------------
    Step "6. OpenList resources"
    $p = Get-Process -Id $server.Id
    Say ("memory: {0:N0} MB working set, {1:N0} MB private" -f ($p.WorkingSet64 / 1MB), ($p.PrivateMemorySize64 / 1MB))
    if ($testExit -ne 0) { $exitCode = 1; Say "RESULT: FAILED (see the test output above)" } else { Say "RESULT: ALL PASSED" }
} catch {
    $exitCode = 1
    Say "RESULT: STOPPED: $($_.Exception.Message)"
} finally {
    if ($server) { Stop-Process -Id $server.Id -Force -ErrorAction SilentlyContinue; Start-Sleep -Milliseconds 500 }
    if (-not $Keep -and -not $Recheck -and (Test-Path $Data)) {
        Remove-Item -Recurse -Force $Data -ErrorAction SilentlyContinue
        Say "OpenList data (with the Quark login) deleted. The download stays in $WorkDir for re-runs."
    } elseif (Test-Path $Data) {
        Say "OpenList data kept in $Data (contains your Quark login). Run with -Recheck later; delete the folder when done."
    }
    $Report | Set-Content $ReportFile -Encoding utf8
    Write-Host ""
    Write-Host "Report: $ReportFile" -ForegroundColor Green
}
exit $exitCode
