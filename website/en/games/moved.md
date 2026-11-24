# When a game moves

You extracted a new emulator version, moved a game to another drive, reinstalled it… When the game program Ludonimbus remembers is no longer where it was, here's what happens.

## Starting from Steam

With Steam auto-sync on, the Steam shortcut only stores the game's id; Ludonimbus keeps track of the path. If the program is gone:

- **Found nearby** (for example a new version extracted into a folder next to the old one): Ludonimbus shows "Game location changed" with the old and new location. Click "Yes" to use the new one.
- **Not found**: Ludonimbus asks you to choose the new game program; pick the new exe.

<!-- screenshot: runner-game-moved (real-environment screenshot needed) -->

The game then starts as usual. The Steam shortcut doesn't need changing, so emulator updates don't break sync.

## In Ludonimbus

The "Game location" section of the game page shows "Not found" and says the original location no longer exists:

- Click "Find automatically": Ludonimbus looks for the new program near the old location and asks you to confirm what it found.
- Click "Choose again…" to pick the new program yourself.
- Or use the store folder shortcuts below it (Steam, Xbox, Epic, GOG, EA, Ubisoft).

When the location is fine, "Choose again…" switches to another program.

## Saves found, but no game program

Games added from Ludusavi's database or found by their saves may have no game program yet. On the game page, click "Find automatically": Ludonimbus looks for the game in Steam, Xbox, Epic, GOG and installed programs, and asks you to confirm the location it found. A game needs its program before it can be added to Steam.

## Reinstalled, or moved to another folder

Saves are tracked relative to the game folder. If the new folder has no saves, **they are restored from the cloud on the first launch**; there's nothing to do.

## The Steam shortcut was changed {#shortcut-changed}

The "Steam auto-sync" section of the game page shows the shortcut's state:

| Message | What to do |
|---|---|
| The Steam shortcut was changed (it now points to …); auto-sync no longer works | You or another tool (such as Steam ROM Manager) edited the shortcut. Click "Repair" |
| Ludonimbus has moved; the shortcut needs updating | Click "Repair" |
| The shortcut was recreated and can be linked again | Click "Repair" |
| The game's shortcut is no longer in Steam | The shortcut was deleted. Click "Add to Steam…" to add it again |

Repairing requires closing Steam briefly; Ludonimbus closes and reopens it.
