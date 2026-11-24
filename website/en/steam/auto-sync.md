# Steam auto-sync

For non-Steam games that are already in Steam, turn on "Steam auto-sync" and their saves sync whenever you start them from Steam: newer saves are pulled before the game starts, and new saves are uploaded after it exits.

## Turn it on

1. Open the game in the library and find the "Steam auto-sync" section; switch it on.
2. If Steam is running, Ludonimbus explains that it needs to close Steam briefly: Steam overwrites shortcut changes made while it runs, so Ludonimbus closes it, makes the change and reopens it. Confirm.
3. The section now says the game syncs automatically when started from Steam.

![Game page: Steam auto-sync](/screenshots/en/game-detail-steam.png)

Games that aren't in Steam yet have an "Add to Steam…" button instead; see [Add to Steam](./add-to-steam.md).

## What changes

- The Steam shortcut first starts `ludonimbus-run`, a small Ludonimbus program that syncs, starts the game, waits until the game and its launcher have exited, and uploads.
- The shortcut's **name, icon, artwork and play time stay the same**, and Steam screenshots and controller settings work as before.
- The shortcut only stores the game's id; Ludonimbus keeps track of the path. Emulator updates and moved games need no changes in Steam; see [When a game moves](../games/moved.md).
- Launch options are set on the game page under Settings → Launch options.

## Turn it off

Switch it off to restore the original Steam shortcut (this also closes Steam briefly). Do this for every game before uninstalling Ludonimbus.

## Status messages

The "Steam auto-sync" section shows the current state:

| Message | Meaning |
|---|---|
| Syncs automatically when started from Steam | All good |
| Off: starting from Steam doesn't sync | The switch is off |
| No Steam shortcut linked | The game isn't in Steam; use "Add to Steam…" |
| Steam not found | Steam wasn't found on this PC |
| The shortcut was changed, needs updating, or was recreated | Click "Repair"; see [When a game moves](../games/moved.md#shortcut-changed) |

## Several Steam accounts

Ludonimbus reads and changes the shortcuts of the account selected in Settings → Steam → Steam account. When adding games to Steam you can also pick the account in the dialog.

## Launchers that stay open {#launcher}

By default Ludonimbus waits until the game **and its launcher** have both exited before uploading. Some launchers stay open after the game closes; for those, tick "Upload when the game process exits, without waiting for the launcher to close" under the game's Settings → Upload timing. Saves are then uploaded as soon as the game closes and checked again when the launcher closes. This needs the game's real process name under "Game process" (built-in rules usually fill it in).

## Starting outside Steam

If you start a game outside Steam, Ludonimbus notices when it exits and backs it up, as long as it's running in the tray; it can't pull before launch, though. Start games from Steam to get the newest saves first.
