# Conflicts

A conflict happens when **both** PCs changed the saves since they last synced, so Ludonimbus can't know which one you want. It doesn't decide for you: it keeps both and asks.

## Starting from Steam

Ludonimbus shows "Save conflict" with both sides: which device saved the cloud version and when, with its file count and size, and the file count and size on this PC.

- **Use cloud saves**: continue from the other PC.
- **Use local saves**: continue from this PC.
- **Don't start now**: decide later (for example after looking at the version history).

<!-- screenshot: runner-conflict (real-environment screenshot needed) -->

**The side you don't choose is kept in the version history** (trigger "Kept from conflict"), so you can restore it at any time.

## In Ludonimbus

The game page says that both this PC and the cloud have new changes, with the same "Use cloud saves" and "Use local saves" buttons. "Sync now" is unavailable until the conflict is resolved.

<!-- screenshot: game-detail-conflict -->

## Another PC uploaded while you were playing

For example, the same game was open on two PCs at once. When you quit, Ludonimbus tells you that the other device also uploaded new saves: both are in the cloud, and you'll be asked which to use at the next launch (or you can sort it out in Ludonimbus).

## A new PC that already has saves

The first sync on a new PC that already has saves for the game is treated as a conflict if those saves differ from the newest version in the cloud.

## Avoiding conflicts

- Before switching PCs, let the game exit normally and finish uploading. "Synced" in the library, or the status on the game's Steam page, tells you it's done.
- Keep Ludonimbus in the tray so games started outside Steam are backed up when they exit.
- If you played on two PCs while both were offline, a conflict is expected once they're back online; just pick one.
