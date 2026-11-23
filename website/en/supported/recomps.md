# Recomps and ports

Ludonimbus recognises 140+ recompilations and ports by their program and knows where they keep their saves, so you don't have to look.

- Games installed with [RecompLauncher](https://recomplauncher.com/) are listed directly by "Scan this PC", and Steam shortcuts made by RecompLauncher are recognised.
- For games installed any other way, scan your Steam shortcuts with "Scan this PC", use "Scan folder…", or pick the game program in "Add manually".

## Popular projects with checked save locations

These save locations were checked against the source code or PCGamingWiki. "Game folder" is the folder with the game program.

| Game | Project | Save location |
|---|---|---|
| The Legend of Zelda: Ocarina of Time | Longshot (native remake) | `%APPDATA%\Longshot\save` |
| The Legend of Zelda: Ocarina of Time | Ship of Harkinian | `Game folder\Save` |
| The Legend of Zelda: Majora's Mask | 2 Ship 2 Harkinian | `Game folder\saves` |
| Super Mario 64 | Ghostship | `Game folder\saves` |
| Super Mario 64 | sm64ex | `%APPDATA%\sm64ex\sm64_save_file.bin` |
| Super Mario 64 (online co-op) | sm64coopdx | `%APPDATA%\sm64coopdx\sm64_save_file.bin` |
| Star Fox 64 | Starship | `Game folder\default.sav` |
| Mario Kart 64 | SpaghettiKart | `Game folder\default.sav` |
| Perfect Dark | Perfect Dark | `%APPDATA%\perfectdark`; in portable mode `Game folder\eeprom.bin` |
| Sonic Unleashed | Unleashed Recompiled | `%APPDATA%\UnleashedRecomp\save`, `mlsave`; in the game folder when `portable.txt` is next to the program |
| Jak and Daxter, Jak II, Jak 3 | OpenGOAL | `%APPDATA%\OpenGOAL\jak1` (`jak2`, `jak3`) `\saves` |
| Super Mario Bros. | Super Mario Bros. Remastered | `%APPDATA%\SMB1R` (without logs and caches) |
| Fire Emblem: Fortune's Weave | fefw recomp | `Game folder\save` |
| Bloodborne | Lantern Engine (shadPS4) | `Game folder\user\savedata\1\CUSA00207` |

Special cases:

- **Longshot (Ocarina of Time)**: "upload when the game process exits" is on by default, so saves upload as soon as the game closes, without waiting for the launcher that stays open.
- **fefw recomp, Lantern Engine**: a new version extracted into a new folder is found automatically; see [When a game moves](../games/moved.md).
- **OpenGOAL**: the three Jak games share `gk.exe` and are told apart by the launch option (`--game jak2` and so on).

## Games from RecompLauncher's catalog

The remaining rules come from RecompLauncher's catalog:

- projects whose developers declared their save location in the catalog;
- N64Recomp games such as Zelda 64: Recompiled, Banjo: Recompiled and Donkey Kong 64: Recompiled (saves in the program folder's `saves`).

These rules leave out programs, disc images, logs and caches, and sync only saves.

## My game isn't recognised

Use the [save finder](../games/save-finder.md), or add a rule to [`adapters/builtin.yaml`](https://github.com/Shupian0510/Ludonimbus/blob/main/adapters/builtin.yaml); see [Contributing](../reference/contributing.md).
