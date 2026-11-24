---
title: Supported Games and Emulators
description: Which games Ludonimbus can sync - 19 emulators, 140+ recompiled games and ports, and thousands of PC games from Ludusavi's database - plus a save finder for everything else.
---

# Supported games and emulators

Ludonimbus syncs the saves of games that Steam Cloud doesn't cover. It knows where saves are for:

| | How many | How they are recognised |
|---|---|---|
| [Emulators](#emulators) | 19 | Built-in rules, per game for most emulators |
| [Recompiled games and ports](#recomps) | 140+ | By the game program |
| [PC games](#pc-games) | Thousands | Ludusavi's database, sourced from PCGamingWiki |
| [Anything else](#other) | — | The save finder: common locations, or "play and find" |

Everything runs on **Windows 10 / 11 (64-bit)** with Steam installed.

## Emulators {#emulators}

| Emulator | Platform | Sync mode |
|---|---|---|
| Dolphin | GameCube / Wii | Per game (GameCube needs GCI folder memory cards, Dolphin's default) |
| Cemu | Wii U | Per game; reads the MLC folder you set in Cemu |
| Azahar, Citra, Lime3DS | 3DS | Per game |
| Ryujinx | Switch | Per game; finds the save of the current user profile on each PC |
| Eden, Citron, Sudachi, Suyu, yuzu | Switch | Whole emulator (saves are tied to the local user profile) |
| RPCS3 | PS3 | Per game |
| shadPS4 | PS4 | Per game |
| PPSSPP | PSP | Per game |
| Vita3K | PS Vita | Per game |
| PCSX2 | PS2 | Whole emulator (memory cards are shared by all games) |
| DuckStation | PS1 | Whole emulator |
| Xenia | Xbox 360 | Whole emulator |
| RetroArch | Multi-system | Per game (by game file name) |

Portable and installed versions are both found. Only saves are synced, never keys, firmware, shader caches or save states. More: [emulator save sync](../emulator-cloud-saves/index.md) · [where each emulator keeps its saves](../supported/emulators.md).

## Recompiled games and ports {#recomps}

Popular projects include:

- **Zelda**: Ship of Harkinian, 2 Ship 2 Harkinian, Longshot (Ocarina of Time), Zelda 64: Recompiled
- **Mario**: Ghostship, sm64ex, sm64coopdx, SpaghettiKart, Super Mario Bros. Remastered
- **Others**: Starship (Star Fox 64), Perfect Dark, Unleashed Recompiled (Sonic Unleashed), OpenGOAL (Jak 1–3), Banjo: Recompiled, Donkey Kong 64: Recompiled, fefw recomp (Fire Emblem: Fortune's Weave), Lantern Engine (Bloodborne)

The rest come from [RecompLauncher](https://recomplauncher.com/)'s catalog. Games installed with RecompLauncher are listed directly by "Scan this PC". More: [recompiled game saves](../recompiled-game-saves/index.md) · [save locations](../supported/recomps.md).

## PC games {#pc-games}

Save locations for regular PC games come from [Ludusavi](https://github.com/mtkennerly/ludusavi)'s database, which covers thousands of games; the Ludonimbus installer includes Ludusavi's official program.

| Where the game comes from | What works |
|---|---|
| Epic, GOG | "Scan this PC" lists the games they installed |
| Xbox app (PC Game Pass) | Can be added to Steam and started through `gamelaunchhelper.exe`; see [Add to Steam](../steam/add-to-steam.md#xbox) |
| EA, Ubisoft and other stores | Found by their saves; Ludonimbus looks for the game program in the stores' install folders |
| DRM-free and other games | Found by their saves, or searched by name in the Ludusavi database |
| Steam store games | Not needed: Steam Cloud already syncs them, so they are listed apart |

More: [PC games](../supported/pc-games.md).

## Games that aren't recognised {#other}

The [save finder](../games/save-finder.md) helps with any other game:

1. search Ludusavi's database by the game's English name;
2. or pick from folders named like the game in Documents, Saved Games, AppData and the game folder;
3. or **play and find**: start the game, save once, and Ludonimbus lists the folders that were just written to;
4. or choose the save folder by hand.

## Not supported yet

- Games that keep their saves **in the Windows registry** (the scan lists them separately).
- **macOS, Linux and the Steam Deck**: Ludonimbus is Windows-only for now.
- Syncing the games themselves, screenshots or play time (play time sync is on the [roadmap](https://github.com/Shupian0510/Ludonimbus#roadmap)).

## Add a game or emulator

Save rules live in [`adapters/builtin.yaml`](https://github.com/Shupian0510/Ludonimbus/blob/main/adapters/builtin.yaml) (games) and [`adapters/emulators.yaml`](https://github.com/Shupian0510/Ludonimbus/blob/main/adapters/emulators.yaml) (emulators). New rules are welcome; see [Contributing](../reference/contributing.md).
