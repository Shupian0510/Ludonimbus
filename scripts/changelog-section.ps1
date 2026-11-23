# Prints the CHANGELOG.md section for a version (used as release notes) and
# fails unless it contains both a "### Chinese" and a "### English" subsection.
#
# Usage: ./scripts/changelog-section.ps1 -Version 0.1.3
param([Parameter(Mandatory = $true)][string]$Version)
$ErrorActionPreference = "Stop"
$root = Split-Path -Parent $PSScriptRoot
$text = Get-Content (Join-Path $root "CHANGELOG.md") -Raw -Encoding utf8
$v = $Version.TrimStart("v")

$m = [regex]::Match($text, "(?ms)^## \[?v?$([regex]::Escape($v))\]?[^\r\n]*\r?\n(.*?)(?=^## |\z)")
if (-not $m.Success) { throw "CHANGELOG.md has no '## $v' section" }
$body = $m.Groups[1].Value.Trim()

# "\u4E2D\u6587" is the word for Chinese; escaped so the script works in any PowerShell encoding.
if ($body -notmatch "(?m)^### \u4E2D\u6587\s*$") { throw "CHANGELOG.md '## $v' is missing the Chinese subsection" }
if ($body -notmatch "(?m)^### English\s*$") { throw "CHANGELOG.md '## $v' is missing the '### English' subsection" }
if ($body -match "(?m)^-\s*$") { throw "CHANGELOG.md '## $v' still has an empty '- ' placeholder" }

$body
