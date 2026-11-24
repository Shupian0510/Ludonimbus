---
title: Ludonimbus vs Ludusavi, OpenSave, Steam Cloud and Syncthing
titleTemplate: false
description: How Ludonimbus compares with Ludusavi, OpenSave, Steam Cloud and folder sync tools such as Syncthing for game save backup and sync - launching from Steam, version history, emulators, storage and platforms.
---

# Ludonimbus compared with other save tools

There are several good free tools for game saves, and they solve different problems. This page compares Ludonimbus with the ones people ask about most, so you can pick the right one, or combine them.

::: info About this comparison
Facts about other projects come from their own documentation, checked in November 2026. If something is out of date, please [open an issue](https://github.com/Shupian0510/Ludonimbus/issues).
:::

## Short version

- **Ludonimbus**: you play non-Steam games (emulators, recomps, other stores) **through Steam on Windows** and want them to behave like Steam Cloud, with saves in your own cloud.
- **Ludusavi**: you want a flexible, cross-platform **backup tool** for thousands of PC games, run on demand, on a schedule or from the command line.
- **OpenSave**: you want **device-to-device** sync between PCs, Macs, Linux machines and a Steam Deck, without a cloud account.
- **Steam Cloud**: games bought in the Steam store that support it. Nothing else needed.
- **Syncthing or a synced folder**: you just want a folder mirrored between machines and handle the rest yourself.

## Comparison table

| | Ludonimbus | Ludusavi | OpenSave | Steam Cloud | Syncthing / sync folder |
|---|---|---|---|---|---|
| **What it is** | Save sync for non-Steam games in Steam | Save backup and restore tool | Peer-to-peer save sync | Valve's built-in cloud saves | General file sync |
| **Which games** | Emulators, recomps, ports and PC games outside the Steam store | PC games in its database (19,000+), plus custom entries | Games in Ludusavi's database, emulator presets, custom folders | Steam store games whose developer enabled it | Any folder you choose |
| **Newest saves before you play** | Pulled automatically when started from Steam | With `ludusavi wrap` set up as a launch option | Synced between devices while they are online | Yes | Only if the folder finished syncing |
| **After you quit** | Uploads once the game and its launcher have exited | With `wrap`; otherwise when you run a backup | Snapshots and syncs when a tracked game closes | Yes | Whenever files change |
| **Where saves are kept** | Your cloud: Google Drive, WebDAV / NAS, S3, Quark or any folder | A local backup folder, optionally uploaded with rclone | On your own devices; optional cloud mirror | Valve's servers | Your own devices or the sync service |
| **Version history** | Every sync is a version: restore, lock, add a note, export as zip | Full and differential backups with configurable retention | Snapshots and branches | No history you can browse | Depends on the tool's file versioning |
| **Conflicts** | Asks which side to keep; the other stays in the history | Not applicable (backups) | Keep yours, theirs, or both | Asks whether to keep local or cloud | Conflict copies of files |
| **Emulators** | 19 built in, most per game | Custom entries | Presets for RetroArch, Dolphin, Ryujinx, yuzu, Citra, PCSX2, RPCS3, PPSSPP, Cemu, Xenia | — | Whole folders |
| **Recompiled games** | 140+ recognised by their program | Database and custom entries | Database and custom folders | — | Manual |
| **Inside Steam** | Status on the game's play bar (Millennium plugin), one-click Add to Steam with artwork | — | Steam Deck Game Mode plugin | Built in | — |
| **Platforms** | Windows 10 / 11 | Windows, Linux, macOS, Steam Deck | Windows, Linux, macOS, Steam Deck | Wherever Steam runs | Most desktop systems |
| **License** | MIT | MIT | MIT | Proprietary | Syncthing: MPL-2.0 |

## Ludonimbus and Ludusavi

[Ludusavi](https://github.com/mtkennerly/ludusavi) is an excellent backup tool, and Ludonimbus is built on it: the save locations of regular PC games come from Ludusavi's database, and the Ludonimbus installer includes Ludusavi's official program.

They differ in what they are for:

- Ludusavi backs up and restores saves of many games at once. Its `wrap` command can restore before and back up after a game, and its cloud upload uses rclone, which you set up in Ludusavi.
- Ludonimbus is built for **one game at a time, at the moment you press Play**. Turning on Steam auto-sync rewrites the shortcut for you; sync, conflicts and version history are handled per game; cloud sign-in is built in (Google Drive in the browser, Quark with a QR code); and it adds emulator and recomp rules on top of Ludusavi's database.

If you already back up everything with Ludusavi, you can keep doing that. Ludonimbus only touches the games you add to it.

## Ludonimbus and OpenSave

[OpenSave](https://www.opensave.org/) syncs saves directly between your devices, over your local network or through a relay, and can mirror them to a cloud folder. It runs on Windows, Linux, macOS and the Steam Deck.

- Choose OpenSave if you play on several kinds of devices, especially a Steam Deck or a Mac, or you don't want saves in a cloud at all.
- Choose Ludonimbus if you play on Windows PCs through Steam and want the newest saves pulled from your own cloud right before each launch, plus per-game emulator rules and the status inside Steam.

## Ludonimbus and Steam Cloud

Steam Cloud is the experience Ludonimbus imitates, and for Steam store games it is all you need. Ludonimbus recognises games Steam Cloud already syncs and lists them apart, so they are never managed twice. The difference is scope: Steam Cloud only works for store games whose developer enabled it; Ludonimbus covers everything else in your library and stores saves in your own cloud.

## Ludonimbus and Syncthing or a synced folder

Syncthing, OneDrive, Dropbox or Google Drive for desktop mirror folders and know nothing about games. That is fine if your game keeps its saves in one folder and you never play on two PCs at once. Ludonimbus adds what folder sync can't know: where each game keeps its saves, when it starts and quits, which side to keep in a conflict, and a version of the whole save for every session. You can also use a synced folder **as** Ludonimbus's cloud; see [Local / synced folder](../clouds/folder.md).

## Other tools

- **EmuDeck CloudSync** syncs emulator saves for EmuDeck installations through rclone.
- **Decky plugins** such as Decky Cloud Save and Decky Ludusavi bring save backups to the Steam Deck's Game Mode.

## Try Ludonimbus

[Download for Windows](https://github.com/Shupian0510/Ludonimbus/releases/latest) · [Quick start](../guide/quick-start.md) · [Supported games](../supported-games/index.md)
