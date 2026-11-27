---
layout: home
title: Ludonimbus – Open Source Game Save Sync for Non-Steam Games & Emulators
titleTemplate: false
description: Free, open-source Steam Cloud alternative for non-Steam games, emulators and recompiled games. Automatic game save sync, version history and Steam integration, using your own Google Drive, NAS, WebDAV or S3 storage.

hero:
  name: Ludonimbus
  text: Steam Cloud for Non-Steam Games, Emulators and Recompiled Games
  tagline: Free, open-source game save sync. Saves are pulled before you play and uploaded when you quit, every version is kept, and everything lives in your own cloud storage, with no third-party servers.
  image:
    src: /app-icon.svg
    alt: Ludonimbus
  actions:
    - theme: brand
      text: Download for Windows
      link: https://github.com/Shupian0510/Ludonimbus/releases/latest
    - theme: alt
      text: Quick start
      link: /en/guide/quick-start
    - theme: alt
      text: Compare with other tools
      link: /en/comparison/
    - theme: alt
      text: GitHub
      link: https://github.com/Shupian0510/Ludonimbus

features:
  - icon: ☁️
    title: Works like Steam Cloud
    details: Press Play in Steam as usual. Newer saves are pulled before the game starts and new saves are uploaded after it exits. Steam still tracks your play time.
    link: /en/non-steam-cloud-saves/
    linkText: Cloud saves for non-Steam games
  - icon: 🎮
    title: Status inside Steam
    details: The play bar of a game's library page shows "up to date", "uploading", "conflict" and so on, with a sync button.
    link: /en/steam-integration/
    linkText: Steam integration
  - icon: 🧩
    title: Emulators, per game
    details: 19 emulators (Dolphin, Ryujinx, Cemu, RPCS3, PCSX2…), most of them synced game by game. Emulator updates don't break sync.
    link: /en/emulator-cloud-saves/
    linkText: Emulator save sync
  - icon: 🛠️
    title: Recomps and ports
    details: 140+ recompiled games and ports, such as Zelda 64, Ship of Harkinian and Unleashed Recompiled, are recognised by their program.
    link: /en/recompiled-game-saves/
    linkText: Recompiled game saves
  - icon: 🕘
    title: Version history
    details: Every save records the time, the device and where it came from. Make it current, keep it, name it or export it for a friend. Old automatic saves are cleaned up; kept saves stay.
    link: /en/sync/versions
    linkText: Version history
  - icon: 🤝
    title: Conflicts keep both sides
    details: When two PCs both changed the saves, you choose which one to use and the other stays in the history. No connection? Play anyway.
    link: /en/sync/conflicts
    linkText: Conflicts
  - icon: 🔍
    title: Add games without hunting for saves
    details: Scan Steam shortcuts, Epic / GOG / Xbox games, saves inside emulators and thousands of PC games, then add them in bulk, or add them to Steam with artwork.
    link: /en/supported-games/
    linkText: Supported games
  - icon: 🗄️
    title: Your own storage
    details: Google Drive, a NAS, Nextcloud, any WebDAV server, S3-compatible storage or any folder. Switching clouds copies your whole history over.
    link: /en/google-drive-save-sync/
    linkText: Google Drive save sync
---

<div class="vp-doc home-content">

## Built for the games Steam Cloud leaves out

Steam Cloud only syncs games bought in the Steam store. Everything else you add to your library with "Add a Non-Steam Game" keeps its saves on one PC, where a reinstall, an emulator update or a new PC can lose them.

<div class="home-cards">

- [**Non-Steam games**](./non-steam-cloud-saves/index.md)<br>Epic, GOG, Xbox app and DRM-free games, and anything else you launch from Steam.
- [**Emulators**](./emulator-cloud-saves/index.md)<br>GameCube, Wii U, Switch, PS2, PS3, PS4, PSP and more, synced per game where possible.
- [**Recompiled games**](./recompiled-game-saves/index.md)<br>N64 recomps, decompilation ports and native remakes that keep saves next to the program.
- [**Steam integration**](./steam-integration/index.md)<br>Sync on Play, cloud status on the game page, one-click Add to Steam with artwork.
- [**Google Drive**](./google-drive-save-sync/index.md)<br>Use the 15 GB you already have. Ludonimbus only sees the files it created.
- [**Supported games**](./supported-games/index.md)<br>19 emulators, 140+ recomps and ports, and thousands of PC games.

</div>

## How it works

1. **Connect a cloud**: Google Drive, a NAS over WebDAV, S3-compatible storage, Quark or any folder.
2. **Add games**: "Scan this PC" finds Steam shortcuts, store games, emulator saves and PC games with known save locations.
3. **Turn on Steam auto-sync**: the game's Steam shortcut now syncs before and after the game. Its name, artwork and play time stay the same.
4. **Play as usual**: press Play in Steam on any PC. The newest saves are already there.

![The Ludonimbus library](/screenshots/en/library-grid.png)

## More than a backup tool

Backup tools copy save folders when you remember to run them. Ludonimbus hooks into the moment you press Play: it makes sure the newest saves are on this PC before the game starts, waits until the game and its launcher have exited, and uploads a new version. See [how it compares to Ludusavi, OpenSave, Steam Cloud and Syncthing](./comparison/index.md).

## Free, open source and private

- **MIT licensed**, with the source code on [GitHub](https://github.com/Shupian0510/Ludonimbus).
- **No account and no servers**: saves only travel between your PC and the cloud you chose. Every version is a plain zip you can open without Ludonimbus.
- **Built on open data**: save locations come from [Ludusavi](https://github.com/mtkennerly/ludusavi)'s database and [RecompLauncher](https://recomplauncher.com/)'s catalog, plus hand-checked rules for emulators.

**Requirements**: Windows 10 / 11 (64-bit) with Steam installed. New here? Start with [Download and install](./guide/install.md) and the [Quick start](./guide/quick-start.md). · [Privacy policy](./privacy.md) · [简体中文](../index.md)

</div>
