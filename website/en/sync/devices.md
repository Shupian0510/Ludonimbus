# Multiple devices

To continue your games across PCs (a desktop, a laptop, a handheld), install Ludonimbus on each and connect them all to the same cloud.

## Setting up another PC

1. Install Ludonimbus.
2. In Settings → Cloud, connect the **same** cloud: the same Google account, the same WebDAV / S3 server and folder, or the same synced folder.
3. In Settings → "This PC and sync" → "Device name", give the PC a name you'll recognise, such as "Living room" or "Handheld". It appears in the version history and in conflict prompts.
4. Add your games, see below.
5. Turn on Steam auto-sync, or use Add to Steam.

## Adding the cloud's games to this PC

**Option 1: "In the cloud"** (the most reliable)

"Add game → In the cloud" lists games another PC uploaded that this PC hasn't added yet. Click "Link to this PC…" and pick the matching Steam shortcut on this PC; for games not in Steam, choose the game program directly. Sync once after linking and the cloud saves come down.

<!-- screenshot: add-game-cloud -->

**Option 2: just scan**

When you add games with "Scan this PC", games recognised by a built-in rule, and games with the same name as on your other PC, get the same game id and match the saves already in the cloud.

Each game's id is shown at the bottom of the Settings tab on its page; your PCs find the same cloud saves through it. Same id on both PCs means the same saves.

## Devices page

"Devices" in the sidebar lists the PCs connected to the same cloud: device name, last seen, Ludonimbus version and Windows version. Every version records which device uploaded it.

## Switching between PCs

- When you're done on one PC, let the game exit normally and finish uploading; then start it from Steam on the other PC, which pulls the newest saves first.
- If both PCs changed the saves, you get a [conflict](./conflicts.md) and both are kept.

## Emulators on several PCs

- Emulators can live in different folders on each PC; saves are tracked by the emulator's data folder.
- **Switch emulators**: Ryujinx syncs game by game and finds the current user profile's save on each PC, creating it when missing (Ryujinx must be closed). Eden, Citron, Sudachi, Suyu and yuzu can only sync the whole emulator, including user profiles. See [Emulators](../supported/emulators.md).
