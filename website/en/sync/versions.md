# Saves

Every upload leaves a save in the cloud. Open the "Saves" tab on a game's page to see the progress saved on all your devices and in every version of the game.

![The Saves tab](/screenshots/en/versions.png)

The app uses three words for saves: **current**, **kept** and **automatic saves**.

## What the list shows

The top part is only a few rows:

- **Current**: the save this device is using now.
- The newest save of each platform (the Steam version, the Xbox version, an emulator…), shown on its own row when it isn't the current one.
- **Kept** saves: the ones you starred or named.
- The newest of another device's own saves, see [Multiple devices](./devices.md#independent).

Below that, folded away, are the "Automatic saves N": the ones made each time you finish playing.

A row shows the platform's logo, the time, the device and the size. The newest save in the cloud is marked "Newest"; the one put aside before this device's saves were replaced is marked "Replaced". Hover over a row for the exact time and where the save came from (the table below). Click a row to see its files.

| Source | When it's made |
|---|---|
| After the game exited | A game started from Steam exited and its saves were uploaded |
| Exit seen in the background | A game started outside Steam exited and the tray app uploaded its saves |
| Uploaded before launch | Saves that couldn't be uploaded earlier, sent before the next launch |
| Synced by hand | You clicked "Sync now" |
| Manual backup | You clicked "Back up current saves" |
| Made current | After a save was made current |
| Kept before replacing | Saves on this PC that weren't uploaded yet, put aside before they were replaced |
| Kept in a conflict | The side you didn't choose in a conflict |
| Shared by someone | An imported `.lnsave` file |

## Make current {#make-current}

"Make current" on a row replaces the saves on this device with that one. The confirmation says whose save it is and when it was made; the saves you have now are kept first, so nothing is lost.

The message afterwards has "Undo". The game page also keeps a line "Replaced by the saves of …" for 24 hours, with the same Undo, as long as you haven't saved new progress since.

- It isn't available while the game is running.
- Saves of another platform, or another device's own saves, are made current the same way when the format is the same. For a different format use "More → Send to…", see [Several versions of one game](./game-versions.md#send).
- Other devices that follow the same progress pick it up at their next sync.

## Keep

Click the star on a row and that save is **kept for good**: clean-up never removes it. Click again to stop keeping it.

"More → Rename…" gives it a name, such as "Before the boss" or "End of year one". A named save is kept as well.

"Back up current saves" on the game page makes a kept save, and you can add a note to it.

## How many automatic saves stay

Automatic saves are cleaned up by Settings → "This PC and sync" → "Automatic saves":

- the last **20** stay;
- plus one a day for **30** days;
- kept saves stay for good. The newest save, the newest of each platform and the save each device last synced are never cleaned up either.

Both numbers can be changed, see [Settings reference](../reference/settings.md).

## Deleting and Recently deleted

"More → Delete" moves a save to "Recently deleted" at the bottom of the list, where you can **put it back for 30 days**; after that it is removed. You can also "Delete for good" there yourself.

A kept save has to stop being kept before it can be deleted. The newest save can't be deleted.

## Export and import {#share}

"More → Export…" writes a save to a `.lnsave` file you can copy elsewhere or give to a friend. The file doesn't hold your device's name or your folders.

Your friend clicks "Import…" at the bottom of the Saves tab of the same game and picks the file. They get one more kept save; the saves on their PC and their sync state don't change until they click "Make current".

Ludonimbus first checks that the file belongs to the same game and isn't damaged. If it doesn't fit, nothing is imported and the reason is shown.

## From Steam {#restore-from-steam}

With the [Steam plugin](../steam/status-in-steam.md), clicking "Ludonimbus cloud saves" on a game's Steam page opens the same list: time, device, file count and size, with the "Latest", "Current" and "Kept" marks. Pick one, click "Make current" and confirm. The dialog also has "Sync now" and "Open in Ludonimbus" at the bottom.

## What it looks like in your cloud

Everything of a game is under `Ludonimbus/games/<game>/` in your cloud: `packs/` holds zips with the save files themselves, and `saves/` holds one small record per save, listing its files, the device and the time.

- For saves up to 16 MB (nearly all games), each save is one complete zip in `packs/`, which you can download and unpack from your cloud's website without Ludonimbus.
- Larger saves upload only the files that changed; unchanged files stay in the earlier zips, so the whole thing isn't sent every time. To get one complete save, export it from the Saves tab.

For games that came from 0.6, the zips in the old `snapshots/` folder are left where they are and still appear under Saves.
