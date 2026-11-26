# Writes latest.json for the in-app updater: the file the app downloads from
# https://github.com/Shupian0510/Ludonimbus/releases/latest/download/latest.json
# to learn about a new version. Same shape tauri-action used to produce.
# Used by .github/workflows/release.yml.
#
# Usage: ./scripts/updater-manifest.ps1 -Version 0.6.0 -Url <installer download URL> `
#          -SignatureFile <installer>.sig -NotesFile release-notes.md -Out latest.json
param(
    [Parameter(Mandatory)][string]$Version,
    [Parameter(Mandatory)][string]$Url,
    [Parameter(Mandatory)][string]$SignatureFile,
    [Parameter(Mandatory)][string]$NotesFile,
    [Parameter(Mandatory)][string]$Out
)
$ErrorActionPreference = "Stop"

$platform = [ordered]@{
    signature = (Get-Content -Raw $SignatureFile).Trim()
    url       = $Url
}
$manifest = [ordered]@{
    version   = $Version
    notes     = (Get-Content -Raw -Encoding utf8 $NotesFile).TrimEnd()
    pub_date  = [DateTime]::UtcNow.ToString("yyyy-MM-dd'T'HH:mm:ss.fff'Z'")
    platforms = [ordered]@{
        "windows-x86_64"      = $platform
        "windows-x86_64-nsis" = $platform
    }
}
$manifest | ConvertTo-Json -Depth 5 | Set-Content -Path $Out -Encoding utf8NoBOM
Write-Host "Wrote $Out for $Version"
