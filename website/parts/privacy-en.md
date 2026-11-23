Effective date: November 22, 2026

Ludonimbus is an open-source desktop application for Windows that backs up and syncs game save files. It runs entirely on your computer. The developer does not operate any server and does not collect, receive, or store any of your data.

## Google user data the app accesses

If you choose to connect Google Drive, Ludonimbus asks for these permissions:

- `https://www.googleapis.com/auth/drive.file` — to create, read, update, and delete **only the files that Ludonimbus itself creates** in your Google Drive. It cannot see or change any other file in your Drive.
- `openid` and `email` — to show which Google account is connected inside the app.

## How the data is used

- Ludonimbus creates a folder named `Ludonimbus` in your Google Drive and stores in it: compressed copies of your game save files, metadata about each saved version (time, file names, sizes, checksums, an optional comment), and a small file per computer with its device name, operating system, and app version.
- These files are used only to provide the app's features: restoring saves, showing version history, and syncing between your own computers.
- Your email address is only displayed in the app's settings.

## How the data is stored

- Data is transferred directly between your computer and Google's servers over HTTPS. It never passes through any server run by the developer.
- The Google sign-in token is stored on your computer in the Windows Credential Manager. App settings and sync state are stored locally in `%APPDATA%\Ludonimbus`.

## Sharing

Ludonimbus does not sell, share, or transfer your data to anyone. It contains no advertising, analytics, or telemetry.

Ludonimbus's use and transfer of information received from Google APIs adheres to the [Google API Services User Data Policy](https://developers.google.com/terms/api-services-user-data-policy), including the Limited Use requirements.

## Your choices

- Disconnect Google Drive at any time in Ludonimbus under Settings → Cloud. This deletes the stored sign-in token from your computer.
- Revoke the app's access at [myaccount.google.com/permissions](https://myaccount.google.com/permissions).
- Delete all data the app stored in your Drive by deleting the `Ludonimbus` folder in Google Drive.
- If you use a local or synced folder instead of Google Drive, no Google data is involved.
- If you connect Baidu Netdisk, Ludonimbus uses Baidu's official open platform (permissions `basic,netdisk`) and can only read and write the folder 我的应用数据/Ludonimbus (My App Data/Ludonimbus) in your Baidu Netdisk. The sign-in token is stored in the Windows Credential Manager and removed when you disconnect. Delete that folder to delete all data the app stored there.
- If you use a WebDAV server or S3-compatible storage, saves are sent only to the server you entered. Its password or secret key is stored in the Windows Credential Manager on your computer and removed when you disconnect.
- If you enter a SteamGridDB API key to pick artwork, Ludonimbus sends the game names you search for to SteamGridDB with your key and downloads the images you choose. The key is stored in the Windows Credential Manager and can be deleted in Settings. No save data is sent to SteamGridDB.

## Changes and contact

Changes to this policy will be published on this page and in the [GitHub repository](https://github.com/Shupian0510/Ludonimbus). Questions: please open an issue at [github.com/Shupian0510/Ludonimbus/issues](https://github.com/Shupian0510/Ludonimbus/issues).
