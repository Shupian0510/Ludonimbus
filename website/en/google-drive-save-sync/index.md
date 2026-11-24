---
title: Sync Game Saves with Google Drive
description: Use your own Google Drive as cloud storage for game saves. Automatic sync for non-Steam games and emulators, version history as plain zip files, and access limited to the files Ludonimbus created.
---

# Sync game saves with Google Drive

Most people already have a Google account with 15 GB of free Drive space, and game saves are small: usually a few KB to tens of MB per game. Ludonimbus uses **your own Google Drive** as the cloud for your non-Steam games, emulators and recompiled games, and syncs it automatically when you play from Steam.

[Download for Windows](https://github.com/Shupian0510/Ludonimbus/releases/latest) · [Google Drive setup guide](../clouds/google-drive.md)

## Why not just put saves in a Drive folder?

Google Drive for desktop can mirror a folder, and for a single game that may be enough. For game saves it has gaps:

| | A synced Drive folder | Ludonimbus with Google Drive |
|---|---|---|
| Newest saves before you play | Only if the folder finished syncing in time | Pulled before the game starts from Steam |
| Upload timing | Whenever files change, even mid-save | After the game and its launcher have exited |
| Saves in other folders | You move or link every save folder yourself | Save locations are found for you, wherever the game keeps them |
| Two PCs changed the saves | Conflicting copies of individual files | You choose which side to keep; the other stays in the history |
| History | File by file | A version of the whole save per play session, with device and time |
| Emulators | The whole emulator folder | Per game for most emulators, without keys or firmware |

## Connect in a minute

1. Open Settings → Cloud and click **Connect Google Drive** (the first-run guide offers it too).
2. Your browser opens Google's sign-in page. Choose your account and allow access.
3. Back in Ludonimbus, the cloud section shows "Connected", your account and the space used.

![Settings → Cloud: Google Drive connected](/screenshots/en/settings-cloud-google.png)

Then add games and turn on Steam auto-sync; see the [Quick start](../guide/quick-start.md).

## What Ludonimbus can see in your Drive

- **Only the files Ludonimbus itself created**, through Google's `drive.file` permission. It cannot see or change anything else in your Drive.
- The sign-in token is stored in the Windows Credential Manager on your PC.
- There are no Ludonimbus servers in between: saves travel directly between your PC and Google.

See the [Privacy policy](../privacy.md).

## What it looks like in Drive

Saves are in a **Ludonimbus** folder that you can browse and download on the Drive website:

```text
Ludonimbus/devices/<device>.json            one small file per PC
Ludonimbus/games/<game>/game.json           game info
Ludonimbus/games/<game>/snapshots/*.zip     one zip per version
```

Each version is an ordinary zip, so you can get your saves back even without Ludonimbus. Old automatic versions are cleaned up; manual backups and locked versions are kept.

## Several PCs

Sign in with the **same Google account** on every PC. When you start a game on another PC, the newest saves are pulled first. If you played on two PCs without syncing in between, Ludonimbus asks which saves to keep. See [Multiple devices](../sync/devices.md).

## Disconnecting

"Disconnect" only stops syncing on that PC and removes its sign-in token; your saves stay in Drive. To revoke access completely, remove Ludonimbus at [myaccount.google.com/permissions](https://myaccount.google.com/permissions). To delete all cloud data, delete the Ludonimbus folder in Drive.

## Not using Google Drive?

The same sync works with other storage, and you can switch later with your whole history copied over ([Switching clouds](../clouds/switching.md)):

- **OneDrive or Dropbox**: pick their desktop sync folder as a [local / synced folder](../clouds/folder.md).
- **A NAS or Nextcloud**: connect over [WebDAV](../clouds/webdav.md).
- **Cloudflare R2, MinIO and other object storage**: [S3-compatible storage](../clouds/s3.md).

See [Choosing a cloud](../clouds/index.md) for a comparison.
