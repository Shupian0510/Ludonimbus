# Multiple devices

To continue your games across devices (a desktop, a laptop, a handheld), install Ludonimbus on each and connect them all to the same cloud.

## Setting up another device

1. Install Ludonimbus.
2. In Settings → Cloud, connect the **same** cloud: the same Google account, the same WebDAV / S3 server and folder, or the same synced folder.
3. In Settings → "This PC and sync" → "Device name", give the device a name you'll recognise, such as "Living room" or "Handheld". It appears in the list of saves and in conflict prompts.
4. Add your games, see below.
5. Turn on Steam auto-sync, or use Add to Steam.

## Adding the cloud's games to this device

Games another device uploaded that this one doesn't have yet are grey "Only in the cloud" cards in the library. Open one to see its saves and play time.

**Option 1: just scan**

Add the game with "Add game → Scan this PC". When Ludonimbus recognises it as a game the cloud already has, it continues from that progress: the same game on the same platform joins without asking, and a version that keeps its saves in a different place is asked about once, see [Several versions of one game](./game-versions.md).

**Option 2: "From the cloud"**

"Add game → From the cloud" lists games another device uploaded that this one hasn't added yet. Click "Link on this PC…" and pick the matching Steam shortcut on this device; for games not in Steam, choose the game's program directly. The page of an "Only in the cloud" game has "Add to this PC…" as well.

![Add game → From the cloud](/screenshots/en/add-game-cloud.png)

Sync once after adding and the cloud's saves come down. If this device already has different saves, you choose which to keep, and the other stays under [Saves](./versions.md).

## Switching between devices

- When you're done on one device, let the game exit normally and finish uploading; then start it from Steam on the other, which pulls the newest saves first.
- If both devices changed the saves, you get a [conflict](./conflicts.md) and both are kept.

## A device with its own progress {#independent}

By default all devices share one progress. To let one device play its own game (a child's PC, a handheld lent to a friend), click "This PC" on the version strip of the game page:

| Choice | Meaning |
|---|---|
| Share with the other devices | The default |
| Keep this device's own | This device saves its own progress: it doesn't take the others' and doesn't change theirs |
| Follow "…" | Share the progress of another device that keeps its own. For example a parent's desktop and handheld share, and the child's PC is on its own |

- Switching to its own progress loses nothing: the device continues from the saves it has, and the shared progress stays as it is.
- Switching back to sharing, if this device's saves differ from the newest shared ones, you choose which to keep.
- A device's own progress is visible on the Saves tab of the other devices (its newest save), where "Make current" brings it over.
- Play time and achievements are recorded per device anyway.

To make every game added to this device from now on keep its own saves, turn on Settings → "This PC and sync" → "New games keep own saves". Games that already sync are not changed.

## Devices page

"Devices" in the sidebar lists the devices connected to the same cloud: device name, last seen, Ludonimbus version and system.

A device still running an older Ludonimbus is marked "Needs an update", see the next section.

## Upgrading from 0.6 to 1.0 {#upgrade}

1.0 stores saves in the cloud in a new way. Upgrading needs nothing from you:

- The first device to upgrade tidies the cloud once, in the background. **None of the old files are changed, moved or deleted.** On a slow cloud this can take a few minutes; games can be played meanwhile, and their saves wait in the queue until it is done.
- Games, saves and play time carry over. Versions that were "locked" or made by "manual backup" become [kept saves](./versions.md), and their note becomes the name.
- **Update your other devices soon.** A device still on 0.6 can't see saves made by 1.0. If you play the same game on it meanwhile, the updated devices notice what it uploaded and ask you to choose, as in a [conflict](./conflicts.md).
- Before 1.0 changes the settings on a PC for the first time, it saves a copy as `config.v1.json` in the data folder.

## Emulators on several devices

- Emulators can live in different folders on each device; saves are tracked by the emulator's data folder.
- **Switch emulators**: Ryujinx syncs game by game and finds the current user profile's save on each device, creating it when missing (Ryujinx must be closed). Eden, Citron, Sudachi, Suyu and yuzu can only sync the whole emulator, including user profiles. See [Emulators](../supported/emulators.md).
