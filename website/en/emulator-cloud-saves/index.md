---
title: Emulator Save Sync and Cloud Saves
description: Sync emulator saves across PCs with per-game version history for Dolphin, Ryujinx, Cemu, RPCS3, PCSX2, shadPS4, PPSSPP, RetroArch and more. Only saves are synced, never keys, firmware or save states.
---

# Emulator save sync

Emulators keep their saves in their own folders, often in a different place for the portable and the installed version, and an emulator update extracted into a new folder can leave them behind. Ludonimbus knows where 19 emulators keep their saves and syncs them to your own cloud, **game by game** wherever the emulator allows it.

[Download for Windows](https://github.com/Shupian0510/Ludonimbus/releases/latest) · [Full emulator reference](../supported/emulators.md)

## Supported emulators

| Emulator | Platform | Sync mode |
|---|---|---|
| Dolphin | GameCube / Wii | Per game |
| Cemu | Wii U | Per game |
| Azahar, Citra, Lime3DS | 3DS | Per game |
| Ryujinx | Switch | Per game |
| Eden, Citron, Sudachi, Suyu, yuzu | Switch | Whole emulator |
| RPCS3 | PS3 | Per game |
| shadPS4 | PS4 | Per game |
| PPSSPP | PSP | Per game |
| Vita3K | PS Vita | Per game |
| PCSX2 | PS2 | Whole emulator |
| DuckStation | PS1 | Whole emulator |
| Xenia | Xbox 360 | Whole emulator |
| RetroArch | Multi-system | Per game (by game file name) |

Both portable and installed versions are found. Where each emulator keeps its saves, and the details of each one, are on the [Emulators](../supported/emulators.md) page.

## Per game or the whole emulator

- **Per game**: each game gets its own saves and version history. Restoring an old version of one game doesn't touch the others. This is the default wherever it is supported.
- **Whole emulator**: all of the emulator's saves sync together. It is used where saves can't be separated: PCSX2 and DuckStation memory cards are shared by every game, and the yuzu family of Switch emulators ties saves to the local user profile.

## What is synced, and what isn't

| Synced | Not synced |
|---|---|
| Save files and memory cards | Keys (`prod.keys` and similar) and firmware |
| Per-game save folders (`savedata`, `title`, `SAVEDATA`…) | Shader caches, logs and game files |
| User profiles where saves depend on them | Save states |

Ludonimbus never edits your saves: uploading packs a copy, and restoring puts a version's files back.

## Adding emulated games

- **Scan this PC**: "Add game → Scan this PC" lists every game that already has saves inside an emulator, without needing a Steam shortcut first. When the game file can be found (Ryujinx's and Dolphin's game folders, Cemu's game list), adding it to Steam starts that game directly.
- **Steam ROM Manager shortcuts**: when a Steam shortcut starts a game file, Ludonimbus reads the game ID from the disc image, game folder or NSP / XCI and syncs only that game's saves.
- **A shortcut that opens the emulator** syncs the whole emulator.

![Scan this PC, listing games with saves inside emulators](/screenshots/en/add-game-scan.png)

## Emulator updates don't break sync

The Steam shortcut only stores the game's id; Ludonimbus keeps track of the emulator's path. When a new version of an emulator is extracted into a new folder, Ludonimbus finds it, and if a Steam shortcut was edited or recreated, it offers a one-click repair. See [When a game moves](../games/moved.md).

## Switch saves across PCs

Ryujinx numbers save folders per PC in the order they were created, so the same game has a different folder on each machine. Ludonimbus looks up the save of the current user profile on every PC and, on a PC that has no save for the game yet, creates one the way Ryujinx does (Ryujinx must be closed while it does this).

## Questions

**Are save states synced?**
No. Only in-game saves and memory cards are synced. Save states depend on the emulator version and are often large.

**Does RetroArch work?**
Yes, per game, as long as saves are in the `saves` folder inside the RetroArch folder and named after the game file (RetroArch's default).

**My emulator isn't listed.**
Use the [save finder](../games/save-finder.md) to pick its save folder, or contribute a rule to [`adapters/emulators.yaml`](https://github.com/Shupian0510/Ludonimbus/blob/main/adapters/emulators.yaml).

**Does it work with EmuDeck or on a Steam Deck?**
Ludonimbus runs on Windows 10 and 11 only for now. On Windows it works with emulators installed any way, including portable builds.
