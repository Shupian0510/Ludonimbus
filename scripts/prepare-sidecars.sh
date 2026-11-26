#!/usr/bin/env bash
# Prepares the files that ship next to the Ludonimbus app on Linux and macOS
# (Windows: prepare-sidecars.ps1):
#   app/src-tauri/binaries/ludonimbus-run-<triple>   Steam launch wrapper, built here
#   app/src-tauri/binaries/ludusavi-<triple>         official Ludusavi release, checksum-verified
#   app/src-tauri/plugin/ludonimbus.star             Steam plugin, built by build-plugin.ps1
#
# Usage:
#   scripts/prepare-sidecars.sh                       debug build, download Ludusavi if missing
#   scripts/prepare-sidecars.sh --release             release build
#   scripts/prepare-sidecars.sh --ludusavi <file>     use a Ludusavi binary you already have
#   scripts/prepare-sidecars.sh --stand-ins           compile checks only (CI): empty files
#                                                     instead of Ludusavi and the plugin
set -euo pipefail

LUDUSAVI_VERSION=0.31.0
# SHA-256 of the official release assets (GitHub release asset digests).
LUDUSAVI_SHA256_LINUX=7322ff45d41eae7ae064a80d8c9ecccc5b8fb6fc090a603a66369cd4b054068d
LUDUSAVI_SHA256_MAC=5787e64d4c795180ab485535cae0b0ef6ee14d4fbe3813797a86913a37cc47f1

root="$(cd "$(dirname "$0")/.." && pwd)"
bin="$root/app/src-tauri/binaries"
plugin="$root/app/src-tauri/plugin/ludonimbus.star"
mkdir -p "$bin" "$(dirname "$plugin")"
triple="$(rustc -vV | sed -n 's/^host: //p')"

profile=debug
cargo_args=(build -p ludonimbus-runner)
ludusavi=""
stand_ins=0
while [ $# -gt 0 ]; do
  case "$1" in
    --release) profile=release; cargo_args+=(--release) ;;
    --ludusavi) ludusavi="$2"; shift ;;
    --stand-ins) stand_ins=1 ;;
    *) echo "Unknown option: $1" >&2; exit 2 ;;
  esac
  shift
done

echo "Building ludonimbus-run ($profile, $triple)..."
cargo "${cargo_args[@]}"
cp "${CARGO_TARGET_DIR:-$root/target}/$profile/ludonimbus-run" "$bin/ludonimbus-run-$triple"

out="$bin/ludusavi-$triple"
if [ -n "$ludusavi" ]; then
  cp "$ludusavi" "$out"
elif [ "$stand_ins" = 1 ]; then
  [ -s "$out" ] || { : > "$out"; echo "WARNING: empty stand-in for Ludusavi (compile checks only, not for a release)"; }
elif [ ! -s "$out" ]; then
  case "$(uname -s)" in
    Linux) os=linux; sum=$LUDUSAVI_SHA256_LINUX ;;
    Darwin) os=mac; sum=$LUDUSAVI_SHA256_MAC ;;
    *) echo "Unsupported system: $(uname -s)" >&2; exit 1 ;;
  esac
  url="https://github.com/mtkennerly/ludusavi/releases/download/v$LUDUSAVI_VERSION/ludusavi-v$LUDUSAVI_VERSION-$os.tar.gz"
  tmp="$(mktemp -d)"
  trap 'rm -rf "$tmp"' EXIT
  echo "Downloading Ludusavi v$LUDUSAVI_VERSION from $url"
  curl -fsSL "$url" -o "$tmp/ludusavi.tar.gz"
  if command -v sha256sum >/dev/null; then
    got="$(sha256sum "$tmp/ludusavi.tar.gz" | cut -d' ' -f1)"
  else
    got="$(shasum -a 256 "$tmp/ludusavi.tar.gz" | cut -d' ' -f1)"
  fi
  if [ "$got" != "$sum" ]; then
    echo "Ludusavi download checksum mismatch: $got" >&2
    exit 1
  fi
  echo "SHA256 verified: $got"
  tar -xzf "$tmp/ludusavi.tar.gz" -C "$tmp"
  cp "$tmp/ludusavi" "$out"
fi
chmod +x "$bin/ludonimbus-run-$triple" "$out"

if [ ! -s "$plugin" ]; then
  if [ "$stand_ins" = 1 ]; then
    : > "$plugin"
    echo "WARNING: empty stand-in for the Steam plugin (compile checks only, not for a release)"
  else
    echo "The Steam plugin is missing: run scripts/build-plugin.ps1 (PowerShell 7) first" >&2
    exit 1
  fi
fi

echo "Sidecars ready in $bin"
ls -1 "$bin" | grep -- "-$triple" | sed 's/^/  /'
