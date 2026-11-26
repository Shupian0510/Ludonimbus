# Games Steam installed

Games you bought on Steam can be added to Ludonimbus too: to keep a second copy of what Steam Cloud syncs, with history, or to hand the syncing to Ludonimbus so the Steam version shares one progress with other platforms and other devices.

## Three states

For a game Steam installed, the game page has a "Steam Cloud" panel in place of "Steam auto-sync". It shows which state the game is in:

| State | What Ludonimbus does | Good for |
|---|---|---|
| **Steam's** | Leaves the game alone | Just seeing it in the library |
| **Backup only** | Steam Cloud syncs as before. Ludonimbus saves a copy after the game exits and **never changes the game's saves by itself** | Save history and going back to an earlier save, with everything else unchanged |
| **Ludonimbus** | Steam Cloud is off for this game; Ludonimbus pulls before launch and uploads after exit | Sharing progress with the Xbox version, an emulator, or a device on another account, or keeping [a device's own saves](../sync/devices.md#independent) |

Games Steam Cloud doesn't handle at all (games without cloud saves) are synced by Ludonimbus as soon as they're added, like non-Steam games.

## Adding them

- **One at a time**: the results of "Add game → Scan this PC" have a group for games Steam Cloud already syncs; tick the ones you want and add them.
- **All of them**: Settings → Steam → "Games Steam installed" → "Back up all". Steam Cloud keeps syncing, and Ludonimbus saves its own copies. Games whose save location isn't known are skipped; add those one at a time and point to their saves.

"Backup only" carries no risk: Ludonimbus reads and never writes.

## Backup only

- After the game exits, Ludonimbus saves a copy, which appears on the [Saves tab](../sync/versions.md).
- A newer save in the cloud isn't written into the game: Steam Cloud brings the progress over.
- You can still click "Make current" yourself to go back to a save. The confirmation adds one line: next time Steam Cloud syncs it may ask which saves to keep. **Pick this PC's.**

## Taking over {#take-over}

Click "Take over…" in the "Steam Cloud" panel. The dialog has two checks, ticked by themselves when they hold:

1. **Steam is done syncing with its cloud.** Until then, this device may not have the newest saves. Open Steam and let it finish; start the game once and quit if needed.
2. **Steam Cloud is off for this game.** In Steam: Properties → General → turn off "Keep game saves in the Steam Cloud" ("Open Properties" in the dialog goes straight there). Ludonimbus doesn't change this switch for you. Steam may note the change only after a while, or when it quits.

Then click "Take over". Ludonimbus:

- first saves the current saves as a kept save named "Before taking over from Steam Cloud";
- changes the game's launch options in Steam so it starts through Ludonimbus, keeping your own launch options. If Steam is running, it says "Steam needs to close for a moment": click "Close Steam and continue", and Steam reopens when the change is made.

From then on, starting the game from Steam works like [auto-sync for non-Steam games](./auto-sync.md): pull before launch, upload after exit.

::: warning Steam Cloud has to stay off
Two services syncing the same game overwrite each other. Ludonimbus looks at the switch again before every sync: if Steam Cloud was turned back on, the game drops to "Backup only", you're told, and nothing is written into the game.
:::

### Leaving Steam's launch options alone

Games with anti-cheat or their own launcher may not start through Ludonimbus. Tick "Leave Steam's launch options alone" when taking over: the tray app then syncs in the background (pulling while the game isn't running, uploading after it exits). The saves aren't guaranteed to be the newest before launch; if both sides end up with new progress, you choose, as in a [conflict](../sync/conflicts.md).

### Taking over all

Settings → Steam → "Games Steam installed" → "More → Take over all…" takes over every "Backup only" game at once. Turn Steam Cloud off for all games first: in Steam, Settings → Cloud → turn off "Enable Steam Cloud". Games Steam hadn't finished syncing are skipped and stay "Backup only"; the games left out and the reasons are behind the "?" on that row.

## Giving a game back to Steam

Click "Give back to Steam" in the "Steam Cloud" panel: the launch options go back to what they were, and the game returns to "Backup only". Then turn Steam Cloud back on for the game in Steam; if Steam asks which saves to keep, **pick this PC's**.

Before uninstalling Ludonimbus, give back every game you took over.

## When it needs repair

"Repair" in the "Steam Cloud" panel means the launch options in Steam were cleared or changed, or Ludonimbus moved, so the game no longer syncs before launch. Click it to put them back.

## Playing in several places

- **One Steam account, several devices**: take the game over on each, and continue as with any other game.
- **The Steam version and an Xbox or emulator version**: see [Several versions of one game](../sync/game-versions.md).
- **One account, several people playing their own game**: each person's device chooses "Keep this device's own", see [Multiple devices](../sync/devices.md#independent).
