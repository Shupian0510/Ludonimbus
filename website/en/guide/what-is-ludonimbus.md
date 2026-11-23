# What is Ludonimbus

Steam Cloud only works for games bought in the Steam store. The emulators, recompilations and other games you add to Steam keep their saves on your PC only, so they are easy to lose when you reinstall a game, move it to another folder, update an emulator or switch PCs.

Ludonimbus gives these games the Steam Cloud experience: **newer saves are pulled before the game starts from Steam, new saves are uploaded after it exits, and every version is kept.** Saves live in **your own** Google Drive, NAS, WebDAV / S3 storage or any folder. There are no third-party servers.

<!-- screenshot: library-grid -->

## Which games it is for

- **Emulated games**: 19 emulators such as Dolphin, Cemu, Ryujinx, RPCS3 and PCSX2, most of which can sync each game on its own. See [Emulators](../supported/emulators.md).
- **Recomps and ports**: 140+ projects such as Ship of Harkinian, Zelda 64: Recompiled and Unleashed Recompiled, recognised by their program. See [Recomps and ports](../supported/recomps.md).
- **Third-party PC games**: games installed by Epic, GOG or the Xbox app (PC Game Pass), and other PC games. Save locations come from Ludusavi's database of thousands of games. See [PC games](../supported/pc-games.md).

Games bought in the Steam store already have Steam Cloud and don't need Ludonimbus.

## How it works

```text
Steam "Play"
  ↓
ludonimbus-run: pull newer saves from your cloud
  ↓
start the game and wait until it and its launcher have exited
  ↓
upload the new saves as a new version
```

Once [Steam auto-sync](../steam/auto-sync.md) is on for a game, its Steam shortcut first starts a small Ludonimbus program, `ludonimbus-run`, which syncs before and after the game. The shortcut's name, icon, artwork and play time stay the same; you keep pressing Play in Steam as usual.

Every upload is a new **version**: an append-only zip snapshot that records the device, the time and the previous version. Any sync can be undone; see [Version history](../sync/versions.md).

When you close the window, Ludonimbus stays in the system tray. If you start a game outside Steam, it notices when the game exits and backs it up. It cannot pull before launch in that case, so start games from Steam to get the newest saves first.

## Where saves are kept

Saves go wherever you chose in Settings → Cloud, inside a `Ludonimbus` folder:

```text
Ludonimbus/devices/<device>.json            one small file per PC
Ludonimbus/games/<game>/game.json           game info
Ludonimbus/games/<game>/snapshots/*.zip     one zip per version
```

The zips are ordinary archives you can open without Ludonimbus. The developer runs no servers and cannot collect any data; see the [Privacy policy](../privacy.md).

## What it does not do

- It does not sync the games themselves, emulator keys, firmware, shader caches or save states, only saves.
- It leaves games that Steam Cloud already syncs alone.
- It never edits your saves: uploading packs a copy, and restoring puts a version's files back (if the current saves are not in the cloud yet, they are backed up as a version first).

## Next steps

- [Download and install](./install.md)
- [Quick start](./quick-start.md): connect a cloud → add games → turn on Steam auto-sync → play as usual
