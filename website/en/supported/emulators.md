# Emulators

Ludonimbus has built-in save rules for 19 emulators and finds both portable and installed versions. **Only saves are synced**: keys, firmware, shader caches and save states are left alone.

## Game by game, or the whole emulator

| Mode | What it means | When to use it |
|---|---|---|
| **Per game** | Each game has its own saves and version history | Recommended where supported. Restoring one game's old version doesn't touch the others |
| **Whole emulator** | The saves of every game in the emulator sync together | Memory cards shared by all games (PCSX2), saves tied to the local user profile (the yuzu family of Switch emulators), or you just want a simple full backup |

For each emulator it's one or the other: the whole emulator, or game by game.

### Adding games one by one

- **Scan this PC**: "Add game → Scan this PC" lists every game that has saves inside an emulator (source "Saves in an emulator"), without needing a Steam shortcut. When the game file can be found (Ryujinx's and Dolphin's game folders, Cemu's game list), adding it to Steam starts that game directly.
- **Steam shortcuts that start a game file**, for example ones made by Steam ROM Manager: Ludonimbus reads the game ID from the disc image, game folder or NSP / XCI and syncs only that game's saves.
- A shortcut that only opens the emulator (or picking the emulator program in "Add manually") syncs the whole emulator.

## Overview

| Emulator | Platform | Mode |
|---|---|---|
| [Dolphin](#dolphin) | GameCube / Wii | Per game |
| [Cemu](#cemu) | Wii U | Per game |
| [Azahar, Citra, Lime3DS](#azahar) | 3DS | Per game |
| [Ryujinx](#ryujinx) | Switch | Per game |
| [Eden, Citron, Sudachi, Suyu, yuzu](#yuzu) | Switch | Whole emulator |
| [RPCS3](#rpcs3) | PS3 | Per game |
| [shadPS4](#shadps4) | PS4 | Per game |
| [PPSSPP](#ppsspp) | PSP | Per game |
| [Vita3K](#vita3k) | PS Vita | Per game |
| [PCSX2](#pcsx2) | PS2 | Whole emulator |
| [DuckStation](#duckstation) | PS1 | Whole emulator |
| [Xenia](#xenia) | Xbox 360 | Whole emulator |
| [RetroArch](#retroarch) | Multi-system | Per game (by game file name) |

Under "Where saves are" below, the portable location (next to the emulator program) comes first, then the installed version's default; Ludonimbus uses the first one that exists.

## Dolphin {#dolphin}

- **Platform**: GameCube / Wii　**Mode**: per game
- **Where saves are**: `User\` (portable), or `Documents\Dolphin Emulator`, `%APPDATA%\Dolphin Emulator`
  - GameCube: `GC\<region>\Card A\*.gci` (and `Card B`)
  - Wii: `Wii\title\…\<game ID>\data`
- **Notes**: per-game GameCube saves need "GCI folder" memory cards, which is Dolphin's default. `.raw` memory cards are shared by all games and only work in whole-emulator mode. Whole-emulator mode also includes Miis (`Wii\shared2\menu\FaceLib`). The scan reads the game folders from `Dolphin.ini` to find game files.

## Cemu {#cemu}

- **Platform**: Wii U　**Mode**: per game
- **Where saves are**: `mlc01\usr\save\<title ID>`, with `mlc01` in `portable\mlc01`, `mlc01` next to the program, or `%APPDATA%\Cemu\mlc01`
- **Notes**: the MLC folder you set in Cemu is respected. Game names and files come from Cemu's game list.

## Azahar, Citra, Lime3DS {#azahar}

- **Platform**: 3DS　**Mode**: per game
- **Where saves are**: `user\` (portable) or `%APPDATA%\Azahar` (`Citra`, `Lime3DS`), then `sdmc\Nintendo 3DS\…\title\<title ID>\data`
- **Notes**: per-game sync covers the game's own saves; extra data (extdata), which games may share, is only included in whole-emulator mode.

## Ryujinx {#ryujinx}

- **Platform**: Switch　**Mode**: per game
- **Where saves are**: `portable\` (portable) or `%APPDATA%\Ryujinx`, then `bis\user\save\<number>`
- **Notes**:
  - Save numbers are assigned per PC in the order saves are created, so on each PC Ludonimbus looks up the save of the current user profile (the one last opened in Ryujinx) for that game.
  - On a PC that has no save for the game yet, Ludonimbus creates one the way Ryujinx does; **Ryujinx must be closed while it does this**.
  - The game ID comes from the file name or the NSP's metadata, or, when needed, from decrypting the NSP / XCI with your `prod.keys`. Updates and DLC count as the base game.
  - Whole-emulator mode includes all saves, the save index and the user profiles.

## Eden, Citron, Sudachi, Suyu, yuzu {#yuzu}

- **Platform**: Switch (yuzu includes forks such as torzu)　**Mode**: whole emulator
- **Where saves are**: `user\` (portable) or `%APPDATA%\<emulator name>`, then `nand\user\save` and the user avatars
- **Notes**: these emulators tie save folders to the local user profile, so saves synced separately wouldn't line up on another PC; the whole emulator is synced together with the user profiles.

## RPCS3 {#rpcs3}

- **Platform**: PS3　**Mode**: per game
- **Where saves are**: `dev_hdd0\home\00000001\savedata\<game ID>…` in the RPCS3 folder
- **Notes**: only the first user's (00000001) saves are synced.

## shadPS4 {#shadps4}

- **Platform**: PS4　**Mode**: per game
- **Where saves are**: `user\` (portable) or `%APPDATA%\shadPS4`, then `savedata\1\<game ID>`

## PPSSPP {#ppsspp}

- **Platform**: PSP　**Mode**: per game
- **Where saves are**: `memstick\PSP\SAVEDATA\<game ID>…` (portable), or `Documents\PPSSPP\PSP\SAVEDATA`

## Vita3K {#vita3k}

- **Platform**: PS Vita　**Mode**: per game
- **Where saves are**: `ux0\` (portable) or `%APPDATA%\Vita3K\Vita3K\ux0`, then `user\00\savedata\<game ID>`

## PCSX2 {#pcsx2}

- **Platform**: PS2　**Mode**: whole emulator
- **Where saves are**: `memcards\` (portable) or `Documents\PCSX2\memcards`
- **Notes**: memory cards are shared by all games, so they can only be synced as a whole.

## DuckStation {#duckstation}

- **Platform**: PS1　**Mode**: whole emulator
- **Where saves are**: `memcards\` (portable), or `Documents\DuckStation\memcards`, `%LOCALAPPDATA%\DuckStation\memcards`
- **Notes**: the whole memory card folder is synced.

## Xenia {#xenia}

- **Platform**: Xbox 360　**Mode**: whole emulator
- **Where saves are**: `content\` (portable) or `Documents\Xenia\content`
- **Notes**: saves and profiles are synced; DLC and title updates are not.

## RetroArch {#retroarch}

- **Platform**: multi-system　**Mode**: per game (by game file name)
- **Where saves are**: `saves\` in the RetroArch folder, matched by the game's file name, for example `saves\…\Super Mario World.srm`
- **Notes**: only the `saves` folder inside the RetroArch folder is recognised, and save files must be named after the game file (RetroArch's default). Save states are not synced.

## Want another emulator?

Rules for [`adapters/emulators.yaml`](https://github.com/Shupian0510/Ludonimbus/blob/main/adapters/emulators.yaml) are welcome; see [Contributing](../reference/contributing.md).
