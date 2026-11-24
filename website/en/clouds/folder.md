# Local / synced folder

Use any folder as your "cloud": Ludonimbus writes versions into it, and the folder itself takes care of getting them to other PCs.

Good for:

- **The OneDrive, Dropbox or Google Drive desktop app's folder**: the app uploads it and syncs it to your other PCs.
- **A NAS share**, for example a shared folder mapped as a network drive that all PCs at home can reach.
- **A USB drive**, if you just want an extra backup with version history.

## Connect

1. Open Settings → Cloud and find "Local / synced folder".
2. Click "Quick scan": it lists the folders synced by OneDrive, Dropbox, Google Drive for desktop, iCloud and similar apps on this PC. Click one to keep saves in a `Ludonimbus` folder inside it (one that already exists, for example because another PC uses it, is marked "exists").
3. Or click "Choose folder…" to pick a folder yourself, for example a NAS share or a folder on a USB drive.

Once connected, the current folder is shown there; "Choose again…" switches to another one and "Open" shows it.

![Settings → Cloud: local / synced folder](/screenshots/en/settings-cloud-folder.png)

## Keep in mind

- **For several PCs, the folder itself must be synced or shared between them.** A plain folder on one PC is invisible to the others.
- With a sync app, let it finish syncing before you play on another PC, so Ludonimbus sees the newest version.
- Each version is a new file that is never changed afterwards, so sync apps rarely create conflicting copies.
- If the folder is temporarily unavailable (the NAS is off, the USB drive isn't plugged in), Ludonimbus treats the cloud as unreachable: games still start, saves stay on the PC and are uploaded later. See [Playing offline](../sync/offline.md).
