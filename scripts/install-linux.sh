#!/usr/bin/env bash
# Installs Ludonimbus on Linux, Steam Deck (SteamOS) and Bazzite, into your
# home folder: no root, works on read-only systems (docs/plan-platforms.md
# §1.5, §2.1). Run it from the folder of the Linux download:
#
#   ./install-linux.sh              install or update
#   ./install-linux.sh --flatpak    only the background service, for the Flatpak of Ludonimbus
#   ./install-linux.sh --uninstall  remove the program (your settings stay)
#
# What goes where:
#   ~/.local/share/ludonimbus/Ludonimbus.AppImage    the app (desktop mode)
#   ~/.config/Ludonimbus/bin/ludonimbus-run          the Steam launch wrapper
#   ~/.config/Ludonimbus/bin/ludonimbus-agent        the background service (Game Mode)
#   ~/.local/share/applications/ludonimbus.desktop   menu entry
#   ~/.config/systemd/user/ludonimbus-agent.service  starts the service at login
#
# The service runs at a lower CPU and disk priority than games, and keeps
# running after you log out (switching between Game Mode and desktop mode
# logs you out): that needs "lingering" for your user, which this script
# turns on when it may (it tells you the one command when it needs your
# password).
set -euo pipefail

here="$(cd "$(dirname "$0")" && pwd)"
data="${XDG_DATA_HOME:-$HOME/.local/share}"
config="${XDG_CONFIG_HOME:-$HOME/.config}"
app_dir="$data/ludonimbus"
bin="$config/Ludonimbus/bin"
unit_dir="$config/systemd/user"
unit="ludonimbus-agent.service"
flatpak_id="io.github.shupian0510.ludonimbus"
user="${USER:-$(id -un)}"
have_systemd=0
command -v systemctl >/dev/null && systemctl --user show-environment >/dev/null 2>&1 && have_systemd=1

if [ "${1:-}" = "--uninstall" ]; then
  if [ "$have_systemd" = 1 ]; then
    systemctl --user disable --now "$unit" 2>/dev/null || true
  fi
  rm -f "$unit_dir/$unit" "$data/applications/ludonimbus.desktop" "$data/icons/hicolor/256x256/apps/ludonimbus.png"
  rm -rf "$app_dir" "$bin"
  [ "$have_systemd" = 1 ] && systemctl --user daemon-reload || true
  echo "Ludonimbus removed. Settings and saves records are still in $config/Ludonimbus."
  exit 0
fi

# Writes the user service that runs `$1` (the agent, or the Flatpak's).
write_unit() {
  mkdir -p "$unit_dir"
  cat > "$unit_dir/$unit" <<EOF
[Unit]
Description=Ludonimbus background service (cloud saves)
Wants=network-online.target
After=network-online.target

[Service]
ExecStart=$1
Restart=on-failure
RestartSec=10
# Games run beside it: syncing in the background yields the CPU and the disk to them.
Nice=10
IOSchedulingClass=best-effort
IOSchedulingPriority=7

[Install]
WantedBy=default.target
EOF
}

# Starts the service now and at every login, and keeps it running after
# logging out when lingering can be turned on without a password.
start_service() {
  if [ "$have_systemd" != 1 ]; then
    echo "systemd isn't available: start the background service yourself (e.g. from your session's autostart): $1"
    return
  fi
  systemctl --user daemon-reload
  systemctl --user enable "$unit" >/dev/null
  systemctl --user restart "$unit"
  if command -v loginctl >/dev/null; then
    if [ "$(loginctl show-user "$user" -p Linger --value 2>/dev/null || true)" != "yes" ]; then
      if ! loginctl --no-ask-password enable-linger "$user" >/dev/null 2>&1; then
        echo "To keep the background service running when you switch to Game Mode or log out, run this once (it asks for your password):"
        echo "  sudo loginctl enable-linger $user"
      fi
    fi
  fi
}

if [ "${1:-}" = "--flatpak" ]; then
  if ! command -v flatpak >/dev/null || ! flatpak info "$flatpak_id" >/dev/null 2>&1; then
    echo "The Flatpak $flatpak_id isn't installed." >&2
    exit 1
  fi
  write_unit "$(command -v flatpak) run --command=ludonimbus-agent $flatpak_id"
  start_service "flatpak run --command=ludonimbus-agent $flatpak_id"
  echo "Background service for the Flatpak installed."
  exit 0
fi

appimage="$(ls "$here"/Ludonimbus*.AppImage 2>/dev/null | head -n 1 || true)"
if [ -z "$appimage" ]; then
  echo "Ludonimbus*.AppImage not found next to this script." >&2
  exit 1
fi

mkdir -p "$app_dir" "$bin" "$unit_dir" "$data/applications"
install -m 755 "$appimage" "$app_dir/Ludonimbus.AppImage"
for f in ludonimbus-run ludonimbus-agent; do
  if [ -f "$here/$f" ]; then
    install -m 755 "$here/$f" "$bin/$f"
  fi
done
printf '%s' "$app_dir/Ludonimbus.AppImage" > "$bin/app-path"

if [ -f "$here/ludonimbus.png" ]; then
  mkdir -p "$data/icons/hicolor/256x256/apps"
  install -m 644 "$here/ludonimbus.png" "$data/icons/hicolor/256x256/apps/ludonimbus.png"
fi
cat > "$data/applications/ludonimbus.desktop" <<EOF
[Desktop Entry]
Type=Application
Name=Ludonimbus
Comment=Cloud saves for emulators, recomps and non-Steam games
Exec="$app_dir/Ludonimbus.AppImage"
Icon=ludonimbus
Categories=Game;Utility;
Terminal=false
EOF

if [ -x "$bin/ludonimbus-agent" ]; then
  write_unit "\"$bin/ludonimbus-agent\""
  start_service "$bin/ludonimbus-agent"
fi

echo "Ludonimbus installed."
echo "  Desktop mode: open Ludonimbus from the menu, connect a cloud and add games."
echo "  Game Mode: the background service syncs; install the Decky plugin for the quick access menu."
