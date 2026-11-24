# Settings

## Settings page {#app}

Click "Settings" in the sidebar.

![Settings](/screenshots/en/settings.png)

### Cloud

Connect, switch or disconnect your cloud; see [Choosing a cloud](../clouds/index.md). Once connected it shows the cloud type, account or address, and the space used (where the cloud reports it). If something is wrong with the cloud, it says it needs attention and why.

### Language / 语言 {#language}

Interface language: "Same as Windows", 简体中文 or English; it applies right away. By default it follows the Windows display language (Chinese for a Chinese Windows, English otherwise). The tray menu, the dialogs shown when a game starts from Steam and the [cloud status inside Steam](../steam/status-in-steam.md) use it too.

![Settings → Language](/screenshots/en/settings-language.png)

### This PC and sync

| Setting | Default | What it does |
|---|---|---|
| Device name | The computer name | The name shown in the version history, conflict prompts and the Devices page. With several PCs, pick names you'll recognise |
| Keep recent automatic versions | 20 | Older automatic versions beyond this number are cleaned up |
| Plus one per day, kept for | 30 days | On top of the above, one version per day is kept for this many days |
| Network timeout | 8 seconds | If the cloud can't be reached before a game starts, wait this long and then start the game (uploading after exit) |

Manual backups and locked versions are kept forever, regardless of the two settings above. Click "Save" when done.

### Steam

| Setting | What it does |
|---|---|
| Steam location | The Steam folder Ludonimbus uses, detected automatically. With several Steam installs, or when none was found: "Quick scan" looks in the registry and the usual folders on each drive (click one if it finds several); "Choose again…" lets you pick the folder Steam is installed in (it has `steam.exe`). After picking one by hand, "Detect automatically" goes back |
| Steam account | With several Steam accounts, which account's shortcuts Ludonimbus reads and changes |
| Screenshot sync | Off (default) / Upload this PC's screenshots / Upload, and download other PCs' screenshots; see [Screenshot sync](../features/screenshots.md) |
| Launch wrapper | Where `ludonimbus-run.exe` is. If it says `ludonimbus-run.exe` can't be found, Steam auto-sync won't work; reinstall Ludonimbus |

### Show cloud save status in Steam

Install, enable or remove the Steam plugin, and see the state of Millennium and the plugin. See [Cloud status inside Steam](../steam/status-in-steam.md).

### Achievements {#achievements}

| Setting | What it does |
|---|---|
| Show Steam's achievement popup for newly linked games | Each game can also turn popups on or off on its own Achievements tab |
| When Steam can't show it, use a small Ludonimbus window | That window is only visible over windowed or borderless fullscreen games |
| Test popup / Test in 8 seconds | Sends a test achievement to check that popups appear |
| PS4 trophy key | Needed for official PS4 trophy names, kept only on this PC; shadPS4's key is used automatically when set there. See [Achievements](../features/achievements.md#ps4-trophy-key) |
| RetroAchievements account | User name and Web API key (copy it from Settings → Keys on the RetroAchievements site); the key is kept in Windows Credential Manager |

### Cloud speed test {#speed-test}

Measures upload and download speed of your cloud and deletes its test files afterwards. Useful for estimating how long a [game backup](../features/game-backup.md) takes.

### Artwork (SteamGridDB)

Enter or delete your SteamGridDB API key. See [Artwork and SteamGridDB](../steam/artwork.md).

### About

| Item | What it is |
|---|---|
| Version | The current version; "Check for updates" checks right away |
| Data folder | `%APPDATA%\Ludonimbus`: settings, sync records and logs; click to open |
| Ludusavi | Where the bundled Ludusavi is; "Not found" only affects adding PC games from Ludusavi's database |
| Device id | This PC's unique id in the cloud |
| Links | Documentation, GitHub, reporting a problem, and "Show the first-run guide again" (see [First-run guide](../guide/first-run.md)) |

## Per-game settings {#per-game}

Open the "Settings" tab on a game's page.

![Game page: settings](/screenshots/en/game-settings.png)

| Setting | What it does |
|---|---|
| Name | The name shown in Ludonimbus |
| Source group | The library groups games by this name; leave empty to detect it automatically. See [Game sources and groups](../games/sources.md) |
| Auto-sync | When off, starting / quitting the game no longer syncs; manual actions (Sync now, backups) still work |
| Upload timing | Tick "Upload when the game process exits, without waiting for the launcher to close" for launchers that stay open; needs "Game process" |
| Save locations | Folders or files to sync, one per line. Leave empty to use the built-in rule (the grey hint shows what it uses). "Find saves…" lists folders with a similar name to tick and add; "Add folder…" lets you pick one |
| Exclude | Files not to sync, written the same way. "Exclude folder…" adds a folder you pick |
| Game process | The game's real process name, for example `game.exe`, used to tell whether it's still running. Leave empty to use the built-in rule. "Choose program…" adds the file name of a program you pick |
| Launch options | Extra arguments passed to the game |

Click "Save settings" when done. The game id is shown at the bottom; your PCs find the same cloud saves through it, see [Multiple devices](../sync/devices.md).

"Remove from Ludonimbus" only removes the game from Ludonimbus on this PC. Its saves and version history stay in the cloud, so adding it again brings them back. A Steam shortcut made by Ludonimbus is deleted too.

### Writing save locations

Each line starts with a root folder followed by a relative path; `*` matches any file name and `**` any number of folders.

| Root | Points to |
|---|---|
| `{install}` | The game folder (where the game program is) |
| `{appdata}` | `%APPDATA%` (`AppData\Roaming`) |
| `{localappdata}` | `%LOCALAPPDATA%` (`AppData\Local`) |
| `{documents}` | Your Documents folder |
| `{home}` | Your user folder |

Examples:

```text
{install}/save/**
{appdata}/MyGame/Saves/**
{documents}/My Games/MyGame/*.sav
```

Locations written with `{install}` follow the game folder, so saves go back to the right place after moving the game or switching PCs.

## Tray

When you close the window, Ludonimbus stays in the system tray, where it watches games started outside Steam and answers the Steam plugin. The tray menu has "Open Ludonimbus" and "Quit".
