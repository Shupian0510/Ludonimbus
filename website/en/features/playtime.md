# Play time

Ludonimbus records how long each PC plays each game, and the cloud adds it up over all your PCs. Switch PCs or go back and forth between a laptop and a handheld, and you always see the total.

## How it's recorded

- **Started from Steam** (with [Steam auto-sync](../steam/auto-sync.md) on): from game start to exit. Syncing saves before launch and after exit doesn't count.
- **Started directly, without Steam**: recorded too while Ludonimbus is in the tray, from when it sees the game running until it exits.
- **Time played before**: the first time, the play time Steam already recorded on this PC is imported once. After that Ludonimbus's own record counts, so nothing is counted twice.
- Starts shorter than 10 seconds (a failed launch, say) don't count.

## Where you see it

- **Library**: below each cover in the grid, and in the "Playtime" column of the list.
- **Game page**: "played …" at the top; hover for how long each PC played, and how much of it was imported from Steam.
- **Steam game page**: with the [Steam plugin](../steam/status-in-steam.md), Steam's own "Play time" shows the total over all PCs, and so do the library hover and sorting by play time.

::: info Only Steam's display changes
Steam keeps counting on its own and Ludonimbus doesn't write Steam's files. If Steam counted more than Ludonimbus (a session Ludonimbus didn't see, say), the higher number is shown. Quit Ludonimbus and restart Steam, and you see Steam's own number for this PC again.
:::

## Sync

Nothing to set up: each PC writes only its own record, which syncs together with the saves, so PCs playing at the same time don't overwrite each other. Offline play is recorded too and uploaded later.

In the cloud it's `Ludonimbus/games/<game>/playtime/<device>.json`.
