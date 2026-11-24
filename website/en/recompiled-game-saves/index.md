---
title: Cloud Saves for Recompiled Games and PC Ports
description: Back up and sync the saves of recompiled games and fan-made PC ports such as Zelda 64 Recompiled, Ship of Harkinian, Unleashed Recompiled, sm64ex and OpenGOAL. 140+ games recognised, with version history in your own cloud.
---

# Cloud saves for recompiled games and ports

Static recompilations (N64Recomp, XenonRecomp), decompilation-based ports and native remakes let you play classic games as native PC programs. They don't come from the Steam store, so they have no Steam Cloud, and many of them keep their saves **right next to the program**, which is exactly the folder you replace when a new version comes out.

Ludonimbus recognises 140+ recompiled games and ports by their program, knows where each keeps its saves, and syncs them to your own cloud with full version history.

[Download for Windows](https://github.com/Shupian0510/Ludonimbus/releases/latest) · [Full list of recomps and ports](../supported/recomps.md)

## Popular projects

These save locations were checked against the source code or PCGamingWiki. "Game folder" is the folder with the game program.

| Game | Project | Save location |
|---|---|---|
| The Legend of Zelda: Ocarina of Time | Ship of Harkinian | `Game folder\Save` |
| The Legend of Zelda: Ocarina of Time | Longshot (native remake) | `%APPDATA%\Longshot\save` |
| The Legend of Zelda: Majora's Mask | 2 Ship 2 Harkinian | `Game folder\saves` |
| Super Mario 64 | Ghostship, sm64ex, sm64coopdx | `Game folder\saves`, `%APPDATA%\sm64ex` |
| Star Fox 64 | Starship | `Game folder\default.sav` |
| Mario Kart 64 | SpaghettiKart | `Game folder\default.sav` |
| Perfect Dark | Perfect Dark port | `%APPDATA%\perfectdark` |
| Sonic Unleashed | Unleashed Recompiled | `%APPDATA%\UnleashedRecomp` |
| Jak and Daxter, Jak II, Jak 3 | OpenGOAL | `%APPDATA%\OpenGOAL\jak1…3\saves` |
| Fire Emblem: Fortune's Weave | fefw recomp | `Game folder\save` |
| Bloodborne | Lantern Engine (shadPS4) | `Game folder\user\savedata\1\CUSA00207` |

The rest come from [RecompLauncher](https://recomplauncher.com/)'s catalog: projects whose developers declared their save location, and N64Recomp games such as **Zelda 64: Recompiled**, **Banjo: Recompiled** and **Donkey Kong 64: Recompiled**.

## Why recomp saves are easy to lose

- **Saves in the game folder.** Deleting the old folder after extracting an update deletes the saves with it.
- **Each release in a new folder.** If you keep versions side by side, every new one starts with empty saves.
- **No installer, no uninstaller.** Nothing remembers where the game was, so moving it or switching PCs means copying saves by hand.

## How Ludonimbus handles them

- **Saves are tracked relative to the game folder**, not by a fixed path. After a reinstall, a move or on a new PC, they come back on the first launch.
- **A new version in a new folder is found automatically.** The Steam shortcut only stores the game's id, so you don't need to change anything in Steam. See [When a game moves](../games/moved.md).
- **Only saves are synced.** The rules leave out programs, disc images, logs and caches.
- **Launchers that stay open are handled.** For Longshot, saves upload as soon as the game closes, without waiting for its launcher. The same option is available for any game under [Upload timing](../steam/auto-sync.md#launcher).
- **Several games, one program.** OpenGOAL's three Jak games share `gk.exe` and are told apart by the launch option.

## Adding recomps

- Games installed with **RecompLauncher** are listed directly by "Scan this PC", and Steam shortcuts made by RecompLauncher are recognised.
- For games installed any other way, scan your Steam shortcuts with "Scan this PC", use "Scan folder…" on the folder where you keep them, or pick the game program in "Add manually".
- Games that aren't in Steam yet can be [added to Steam](../steam/add-to-steam.md) with artwork from SteamGridDB, and sync from their first launch.

![Add a game: scanning a folder of recomps and ports](/screenshots/en/add-game-scan-folder.png)

## Questions

**My recomp isn't recognised.**
Use the [save finder](../games/save-finder.md): pick from common locations, or play once and let Ludonimbus list the folders the game just wrote to. Rules for [`adapters/builtin.yaml`](https://github.com/Shupian0510/Ludonimbus/blob/main/adapters/builtin.yaml) are welcome.

**Do I need the original game files?**
Ludonimbus doesn't deal with game files or ROMs at all. It only syncs the saves your recomp or port creates.

**What about portable mode?**
Where a project has a portable mode (Unleashed Recompiled with `portable.txt`, Perfect Dark), the rules look in the game folder first.
