# Game backup

Besides the saves, you can back up the game itself to your cloud. After deleting a game, a failed drive or on a new PC, install it back from Ludonimbus or the Steam game page; the saves are pulled automatically.

![The Game files card on a game's page](/screenshots/en/game-files.png)

## What's backed up

Use the "Game files" card on the game's page. It backs up the game's folder **without saves, logs and caches** (saves sync separately).

Emulator and recompiled games are backed up in parts:

| Part | Contents |
|---|---|
| Program | The emulator or the recompiled program |
| Game files | Disc images, ROMs, game folders, updates, DLC |
| Settings and mods | This game's settings, graphic packs, mods |
| Keys and firmware | Only when "Also back up keys, firmware and BIOS" is ticked in the backup settings (Cemu, Ryujinx, Dolphin, shadPS4, RPCS3) |

- When several games share an emulator, the emulator is uploaded once.
- What a recompiled game's launcher builds itself isn't backed up; the launcher builds it again after a restore. A few games (fefw recomp, for example) need you to run the launcher once after restoring to build it.
- "Calculate size" lists each part's file count and size without uploading anything. The backup settings can leave a part out.

## Back up and update

- "Back up to the cloud" scans, checks the cloud, uploads (with speed and time left), verifies and saves a version. You can stop it; the next backup doesn't upload what already arrived.
- Later, "Update backup" splits the game files into chunks by content and **uploads only what changed**; with no changes it tells you so.
- Each backup is a version. The newest 2 are kept by default and older ones are deleted after a successful backup; change this in the backup settings. Content shared by versions is stored once.

## Restore and install from the cloud

- "Restore…" lists the parts in the version. For the program you can keep the emulator already on this PC or restore the backed-up one; pick where to install it and where to put the game files.
- When the game files aren't on this PC, the card says "Not on this PC" and offers "Install from the cloud…".
- With "Afterwards, point Ludonimbus and the Steam shortcut at the new place and pull the saves" ticked, the game's location and launch options are fixed for you and the saves are pulled.
- A stopped restore can be picked up again with "Continue restoring" on the card. Files already on this PC with the same content aren't touched.

## In Steam

With the [Steam plugin](../steam/status-in-steam.md), when a game's files aren't on this PC, the Ludonimbus status on its Steam page says "Game files missing · Click to install from the cloud". Clicking opens the restore dialog in Ludonimbus, and Steam shows the progress while it installs.

## Clouds and speed

- Backups go to `Ludonimbus/gamebackup/` in your cloud. They're your own backups; there's no sharing.
- Games are often tens of GB, so mind your cloud space. Google Drive allows about 750 GB of uploads per account per day.
- Baidu Netdisk without a membership downloads slowly (about 80 KB/s), so a restore takes a long time; you're told before backing up.
- Settings → Cloud speed test measures upload and download speed of your cloud and deletes its test files afterwards.

![Settings → Cloud speed test](/screenshots/en/settings-speed.png)
