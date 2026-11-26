# Linux and Steam Deck (preview)

Since 1.0 there is a Linux version, for Steam Deck (SteamOS), Bazzite and ordinary Linux desktops.

It is a **preview**: the syncing is the same as on Windows, but it hasn't been tried on enough real devices yet. Please report problems on [GitHub Issues](https://github.com/Shupian0510/Ludonimbus/issues).

## Install

1. Download `Ludonimbus-linux-x86_64.tar.gz` from [Releases](https://github.com/Shupian0510/Ludonimbus/releases/latest) and unpack it. On a Deck, switch to Desktop Mode first.
2. In the unpacked folder, run:

```bash
./install-linux.sh
```

It installs into your home folder and needs no root, so it also works on SteamOS and Bazzite, where the system partition is read-only.

| What is installed | Where |
|---|---|
| The app (for Desktop Mode) | `~/.local/share/ludonimbus/Ludonimbus.AppImage`, with a "Ludonimbus" entry in the application menu |
| The small program that syncs saves when a game starts from Steam | `~/.config/Ludonimbus/bin/ludonimbus-run` |
| The background service (for Game Mode) | `~/.config/Ludonimbus/bin/ludonimbus-agent`, started when you log in |

- **Update**: download the new version and run `./install-linux.sh` again.
- **Uninstall**: `./install-linux.sh --uninstall`. Settings and sync records stay in `~/.config/Ludonimbus`.

## First setup

Open Ludonimbus in Desktop Mode and set it up as on Windows: [connect a cloud](../clouds/index.md), [add games](../games/scan.md) and turn on [Steam auto-sync](../steam/auto-sync.md). Connect the same cloud as your Windows PC to continue your games on both, see [Multiple devices](../sync/devices.md).

Windows games running through Proton keep their saves in the prefix Proton makes for each game, and Ludonimbus looks for them there. If a game has never run through Proton on this device, start it once before syncing.

## Game Mode

Game Mode has no windows and no tray, so the background service does the syncing: saves are pulled before a game starts from Steam and uploaded after it exits. When you end a game with "Exit game" in the Steam menu, the background service uploads what didn't get uploaded in time.

Questions that need your choice at launch (a [conflict](../sync/conflicts.md), for example) show as a full-screen window you can answer with the controller, see [Handhelds and the controller view](./handheld.md).

### The Decky plugin {#decky}

With [Decky Loader](https://decky.xyz/) installed, you can add a Ludonimbus panel to the Quick Access Menu: the current game's cloud save status, "Sync now", resolving a conflict, the list of saves and "Make current".

1. Download `Ludonimbus-decky.zip` from [Releases](https://github.com/Shupian0510/Ludonimbus/releases/latest).
2. In Decky's settings, turn on Developer mode, choose "Install plugin from ZIP file" and pick the file.

If the panel says Ludonimbus isn't running: open Ludonimbus once in Desktop Mode, or run `./install-linux.sh` again to make sure the background service is installed.

## Differences from Windows

- The plugin that [shows cloud status inside Steam](../steam/status-in-steam.md) is for desktop Steam on Windows; in Game Mode, use the Decky plugin.
- The Xbox app (Game Pass) version of a game exists on Windows only.
- There is no in-app update; see above for updating.
