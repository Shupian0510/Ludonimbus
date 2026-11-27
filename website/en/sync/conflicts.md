# Conflicts

A conflict happens when **both** devices changed the saves since they last synced, so Ludonimbus can't know which one you want. It doesn't decide for you: it keeps both and asks.

It tells this from what every save records (how far it includes each device's changes), not from the clock: one PC's later time never quietly overwrites the other's progress.

## Starting from Steam

Ludonimbus shows "Save conflict" with both sides: which device saved the cloud's saves and when, with the file count and size, and the file count and size on this PC.

- **Use cloud saves**: continue from the other device.
- **Use this PC's saves**: continue from this PC.
- **Don't start**: decide later (for example after looking at the Saves tab).

<!-- screenshot: runner-conflict (real-environment screenshot needed) -->

**The side you don't choose stays under [Saves](./versions.md)** (source "Kept in a conflict"), so you can make it current again at any time.

## In Ludonimbus

The game page says that both this PC and the cloud have new save changes, with the same "Use cloud saves" and "Use this PC's saves" buttons. "Sync now" is unavailable until the conflict is resolved.

![Game page: save conflict](/screenshots/en/game-detail-conflict.png)

## When it happens

- **You played on two devices, and one hadn't finished uploading when you switched.** For example, you shut it down while the upload was still going in the background, or played offline for a while.
- **Another device uploaded while you were playing.** For example, the same game was open on two devices at once. When you quit, Ludonimbus tells you that the other device also uploaded new saves: both are in the cloud, and you're asked which to use at the next launch (or you can sort it out in Ludonimbus).
- **You chose "Play with this PC's saves" at launch.** The cloud had newer saves and you started before they were downloaded, see [How long it waits](./how-it-works.md#waiting).
- **A new device that already has saves.** At its first sync, the saves on it differ from the newest in the cloud.
- **Two versions start sharing progress** and each has progress of its own, see [Several versions of one game](./game-versions.md#ask-once).

When both sides hold exactly the same saves, it isn't a conflict.

## Avoiding conflicts

- Before switching devices, let the game exit normally and finish uploading. "Synced" in the library, or the status on the game's Steam page, tells you it's done.
- On a device you shut down or put to sleep right after playing, turn on Settings → "This PC and sync" → "Always wait until it's done".
- Keep Ludonimbus in the tray, so games started outside Steam are uploaded when they exit and unfinished uploads are completed in the background.
- If you played on two devices while both were offline, a conflict is expected once they're back online; just pick one.
