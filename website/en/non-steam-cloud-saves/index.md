---
title: Cloud Saves for Non-Steam Games
description: Add Steam-Cloud-style save sync to the non-Steam games in your Steam library, including emulators, recompiled games and Epic, GOG or Xbox games. Free and open source, with version history and your own cloud storage.
---

# Cloud saves for non-Steam games

Steam Cloud only syncs games bought in the Steam store. When you add anything else with **"Add a Non-Steam Game"**, Steam creates a shortcut and nothing more: the game's saves stay on that one PC.

Ludonimbus gives those games the Steam Cloud experience. Press Play in Steam as usual: newer saves are pulled before the game starts, new saves are uploaded after it exits, and every version is kept in **your own** cloud storage.

[Download for Windows](https://github.com/Shupian0510/Ludonimbus/releases/latest) · [Quick start](../guide/quick-start.md)

## Why non-Steam games lose their saves

- **Steam Cloud doesn't apply to shortcuts.** It only works for store games whose developer turned it on.
- **Saves are scattered.** Depending on the game they live in `%APPDATA%`, `Documents`, `Saved Games` or right next to the game program.
- **Updates replace folders.** Many emulators, recompilations and fan ports are updated by extracting a new version into a new folder, leaving the saves behind in the old one.
- **New PC, empty saves.** Reinstalling Windows or moving to another PC means finding and copying every save folder by hand.
- **A synced folder isn't enough.** OneDrive or Dropbox can mirror a folder, but they don't know when a game starts or quits, so they can't make sure you have the newest saves before you play, and they may upload saves while the game is still writing them.

## How Ludonimbus syncs them

```text
Steam "Play"
  ↓
ludonimbus-run: pull newer saves from your cloud
  ↓
start the game and wait until it and its launcher have exited
  ↓
upload the new saves as a new version
```

When you turn on [Steam auto-sync](../steam/auto-sync.md) for a game, its Steam shortcut first starts `ludonimbus-run`, a small program that syncs before and after the game. The shortcut's **name, icon, artwork and play time stay the same**, and Steam screenshots and controller settings work as before.

![A game page in Ludonimbus with Steam auto-sync turned on](/screenshots/en/game-detail-steam.png)

## Which games it covers

| Kind of game | Examples | How saves are found |
|---|---|---|
| Emulated games | Dolphin, Cemu, Ryujinx, RPCS3, PCSX2, PPSSPP, RetroArch | Built-in rules for 19 emulators, most per game. See [emulator save sync](../emulator-cloud-saves/index.md) |
| Recompiled games and ports | Zelda 64: Recompiled, Ship of Harkinian, Unleashed Recompiled, OpenGOAL | 140+ recognised by their program. See [recompiled game saves](../recompiled-game-saves/index.md) |
| Store games outside Steam | Epic, GOG, Xbox app (PC Game Pass), EA, Ubisoft | Ludusavi's database of thousands of games |
| Other PC games | DRM-free games, indie games, mods | Ludusavi's database, or the [save finder](../games/save-finder.md) |

Games that are not in Steam at all can be added with [one click](../steam/add-to-steam.md), together with artwork from SteamGridDB, and they sync from the first launch. Games that Steam Cloud already syncs are listed apart, so nothing is managed twice.

## What you get besides sync

- **[Version history](../sync/versions.md)**: every upload leaves a save with its time, device and source. Make it current, keep it, name it or export it for a friend.
- **[Conflicts keep both sides](../sync/conflicts.md)**: if two PCs both changed the saves, you choose which one to use, and the other stays in the history.
- **[Offline play](../sync/offline.md)**: if the cloud can't be reached, the game starts anyway and uploads later.
- **[Moved games are found again](../games/moved.md)**: saves are tracked relative to the game folder, so a reinstall, a new folder or a new PC picks them up on the first launch.
- **[Cloud status inside Steam](../steam/status-in-steam.md)**: an optional plugin shows "up to date", "uploading" or "conflict" on the game's Steam page.

## Where your saves are stored

Ludonimbus has no servers and no accounts. Saves go to the storage you pick:

- [Google Drive](../google-drive-save-sync/index.md) (sign in once in the browser)
- a NAS, Nextcloud or any [WebDAV](../clouds/webdav.md) server
- [S3-compatible storage](../clouds/s3.md) such as Cloudflare R2 or MinIO
- [Quark](../clouds/quark.md) (experimental)
- any [folder](../clouds/folder.md), including OneDrive or Dropbox sync folders and USB drives

Each version is an ordinary zip file you can open without Ludonimbus.

## Get started

1. [Download and install](../guide/install.md) Ludonimbus (Windows 10 / 11, 64-bit).
2. Connect a cloud in the first-run guide.
3. "Add game → Scan this PC" and tick the games you want.
4. Turn on "Steam auto-sync" on each game's page, or click "Add to Steam…" for games that aren't in Steam yet.

From then on, start your games from Steam on any PC.

## Questions

**Do I have to start the game from Steam?**
Starting from Steam is what lets Ludonimbus pull the newest saves first. If you start a game some other way, Ludonimbus (in the tray) still notices when it exits and uploads the saves.

**Does it change my Steam shortcuts?**
Only the shortcuts of games you turn auto-sync on for, and only what they start. Turning auto-sync off restores the original shortcut.

**Does it work on macOS, Linux or the Steam Deck?**
Windows 10 and 11 are the main platform. Since 1.0 there is a [Linux and Steam Deck version](../platforms/linux.md) (preview). macOS isn't supported yet.

**Is it free?**
Yes. Ludonimbus is open source under the MIT license, and you only need storage you already have.
