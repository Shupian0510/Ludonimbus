# PC games (Ludusavi)

Save locations for regular PC games come from [Ludusavi](https://github.com/mtkennerly/ludusavi)'s database (sourced from PCGamingWiki), which covers thousands of games. The Ludonimbus installer includes Ludusavi's official program.

## How to add them

- **Scan this PC** uses Ludusavi to find every game that already has saves on this PC (source "Saves found"); it takes about half a minute. For games found by their saves, Ludonimbus also looks for the game program in the install folders of Epic, GOG, Xbox, EA, Ubisoft and others, and in common game folders. See [Scan this PC](../games/scan.md).
- **Ludusavi database**: search by English name under "Add game → Ludusavi database", for games the scan didn't find or that have no saves on this PC yet. The first search downloads the database, so give it a moment. See [Add manually](../games/manual.md#ludusavi).
- Games that aren't in the database: use the [save finder](../games/save-finder.md) to pick from common locations, "play and find", or choose the save folder yourself.

## Store games

| Store | Notes |
|---|---|
| Epic, GOG | "Scan this PC" lists the games they installed |
| Xbox app (PC Game Pass) | Can be added to Steam and started through `gamelaunchhelper.exe` in the game folder; see [Add to Steam](../steam/add-to-steam.md#xbox) |
| Steam store games | Games installed in Steam that Steam Cloud already syncs are listed apart; you normally don't need to add them |

## Not supported

- Games that keep their saves **in the registry** can't be synced yet (the scan lists them separately as "Saves in the registry").
- Ludonimbus only syncs save files, never the games themselves.

## Ludusavi not found

If Settings → About → Ludusavi says "Not found", only adding PC games from Ludusavi's database is affected; emulators and recomps work as usual. Reinstalling Ludonimbus fixes it.
