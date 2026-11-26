# Version history

Every upload leaves a version in the cloud. Open the "Version history" tab on a game's page to see the versions from all your devices.

![Version history](/screenshots/en/versions.png)

Each version shows:

- which device saved it and when (versions from this PC are marked "this PC");
- how many files and how big;
- its trigger, see the table below;
- tags: "Latest" is the newest version in the cloud, "Current here" is the version this PC's saves match, "Locked" versions are never cleaned up.

| Trigger | When it's created |
|---|---|
| Auto after exit | A game started from Steam exited |
| Detected exit | The tray app noticed a game started outside Steam exiting |
| Uploaded before launch | Saves that weren't uploaded earlier, uploaded before the next launch |
| Sync now | You clicked "Sync now" |
| Manual backup | You clicked "Back up current saves" |
| Restored | After restoring an older version |
| Before restore | Saves that weren't uploaded yet, backed up just before a restore |
| Kept from conflict | The side you didn't choose in a conflict |

## Restore

Click "Restore" next to a version and confirm to put the game's saves back to that version.

If the current saves aren't in the cloud yet, they are backed up first as a "Before restore" version, so nothing is lost. Restored the wrong one? Restore again. You can't restore while the game is running.

## Restore from Steam

With the [Steam plugin](../steam/status-in-steam.md), click "Ludonimbus Cloud Saves" on the game's Steam page for the same version list (time, device, file count and size, note, and the Latest / On this PC / Locked tags). Pick a version, click Restore and confirm; the restore runs in Ludonimbus and Steam shows "Syncing…" until it's done. The dialog also has "Sync now" and "Open in Ludonimbus".

## Export zip

Save a version as a zip file, for example to keep a copy elsewhere or share it.

## Note

Add a note to a version, like "before the boss" or "new game plus".

## Lock

Lock a version to **keep it forever**; it's never cleaned up. Click again to unlock.

## Delete

A deleted version moves to "Recently deleted" below the list, where you can **restore it for 30 days**; after that it is deleted for good. You can also delete it for good there yourself. A restored automatic version is locked at the same time, so the cleanup doesn't remove it right away. Locked versions and the newest version can't be deleted.

## Manual backup

Click "Back up current saves" on the game page and optionally add a note. Manual backups are **kept forever** and never cleaned up, which makes them good for key moments.

## Automatic clean-up

When automatic versions pile up, they're cleaned up by Settings → "This PC and sync" → "Keep recent automatic versions":

- the **20** most recent automatic versions are kept;
- plus one per day for **30** days;
- manual backups and locked versions are kept forever, and so are the newest version and the version each PC last synced.

Both numbers can be changed; see [Settings](../reference/settings.md).

## Looking at it in the cloud

Each version is a zip under `Ludonimbus/games/<game>/snapshots/` in your cloud, so you can download and open it without Ludonimbus.
