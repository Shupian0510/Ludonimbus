# Scan this PC

Click "Add game" at the top right of the library; the first tab is "Scan this PC". It lists every game on this PC that can be added, ready to tick and add in bulk, without hunting for saves.

<!-- screenshot: add-game-scan -->

## What it looks for

| Source | What it is |
|---|---|
| Steam shortcut | Non-Steam games you added to Steam. With several Steam accounts you can switch account at the top |
| Epic, GOG, Xbox | Games installed by these stores, including Xbox app (PC Game Pass) games |
| RecompLauncher | Recomps installed with [RecompLauncher](https://recomplauncher.com/) |
| Saves in an emulator | Games that have saves inside an emulator, one by one, without needing a Steam shortcut. See [Emulators](../supported/emulators.md) |
| Saves found | PC games from Ludusavi's database that already have saves on this PC. This takes about half a minute |
| Folder | Games found in a folder you picked with "Scan folder…"; see [Scan folder](./scan-folder.md) |

A game found by several sources (say, an Epic game whose saves Ludusavi also found) is listed once.

## How results are grouped

- **Ready to add**: the save location is known (a built-in rule or Ludusavi's database). "Select all" is available.
- **Needs a save location**: Ludonimbus doesn't know where these games keep their saves yet. Click "Choose saves…" and the [save finder](./save-finder.md) walks you through it.
- **Steam Cloud already syncs these**: the game is installed in Steam and Steam Cloud already syncs it, so you normally don't need to add it; tick it if you want an extra backup.
- **Saves in the registry**: games that store their saves in the Windows registry can't be synced yet.
- **Already added**: already in your library.

Each game shows some tags:

| Tag | Meaning |
|---|---|
| Recognised: … | A built-in rule recognised it, for example an emulator or a recomp |
| Ludusavi knows the save location | The save location comes from Ludusavi's database |
| Found the game (Epic…) | A game found by its saves; its program was found in that store's install folder |
| Installed in Steam | Steam has this game installed |
| Large saves, may include caches | More than 200 MB of saves; click "View" to check what's inside |

"N save files on this PC" is what is on this PC right now. Games marked "Game program not found (maybe uninstalled)" can still be added: their saves sync normally, and you can set the game's location later on its page.

With many games, type in the box at the top to filter by name.

## Add

Tick the games you want and click "Add to Ludonimbus" at the bottom.

If some of the games you just added aren't in Steam yet, a banner says so; click "Add to Steam…" to add them all, so they sync whenever you start them from Steam. See [Add to Steam](../steam/add-to-steam.md).

For games already in Steam, open the game's page and turn on "Steam auto-sync"; see [Steam auto-sync](../steam/auto-sync.md).

## Other tabs in "Add game"

- **In the cloud**: games another PC uploaded that this PC hasn't added yet; see [Multiple devices](../sync/devices.md).
- **Ludusavi database**: search by the game's English name; see [PC games](../supported/pc-games.md).
- **Add manually**: pick the game program or save folder yourself; see [Add manually](./manual.md).

## My game isn't listed

- The game is in another folder: click "Scan folder…".
- You haven't saved in the game yet: Ludusavi only finds games that already have saves. Play a bit first, or use [Add manually](./manual.md).
- Neither works: use [Add manually](./manual.md) with the save finder's "play and find".
