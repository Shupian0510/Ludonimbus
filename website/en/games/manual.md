# Add manually

For games the scan doesn't find, or when you want to set things up yourself, use "Add game → Add manually".

## 1. Choose the game program

Click "Choose game program…" and pick the game's exe. Ludonimbus uses it to tell whether the game is running and to look for saves near the game folder, and you can add it to Steam later.

- Below are shortcuts to each store's install folder: Steam, Xbox, Epic, GOG, EA and Ubisoft.
- **Xbox app (PC Game Pass) games**: pick `gamelaunchhelper.exe` in the game's `Content` folder. The game's own program is protected by Windows and can't be picked or started.
- **Emulators**: pick the emulator program; the built-in rule is applied automatically.
- No program (for example you only want to back up a save folder): click "No program, just choose saves".

![Add game → Add manually](/screenshots/en/add-game-manual.png)

## 2. Recognised by a built-in rule

If the program is recognised, Ludonimbus says it will use the built-in save rule, so you don't need to find the saves. Check the name and click "Add".

::: tip Emulators: whole emulator or game by game
Picking the emulator program syncs the saves of every game in that emulator together. To sync games separately, add the games with saves inside the emulator from "Scan this PC", or create a Steam shortcut per game that starts its game file (for example with Steam ROM Manager) and add those from "Scan this PC". See [Emulators](../supported/emulators.md).
:::

## 3. Not recognised: find the saves

Otherwise the [save finder](./save-finder.md) opens: search Ludusavi's database, pick from common locations, "play and find", or choose a folder yourself.

## Add from Ludusavi's database {#ludusavi}

"Add game" also has a "Ludusavi database" tab. Search by the game's English name (for example Hollow Knight), click "View saves" to see the save files found on this PC, then "Add". You can add a game that has no saves on this PC yet; it syncs as soon as saves appear. This way has no game program; set the game's location on its page later if you need one.

## After adding

- On the game page's Settings tab you can change the name, save locations, excluded files, game process and launch options; see [Settings](../reference/settings.md#per-game).
- To start it from Steam with sync: turn on "Steam auto-sync" if it's already in Steam, otherwise click "Add to Steam…".
