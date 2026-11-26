Effective date: November 25, 2026 (updates the November 22, 2026 version: more about each kind of cloud, Recently deleted, and the data kept on your computer)

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
- The Google sign-in token is stored on your computer in the Windows Credential Manager. App settings and sync state are stored locally in `%APPDATA%\Ludonimbus` (`~/.config/Ludonimbus` on Linux); logs stay on your computer and are never uploaded.
- Save versions you delete in Ludonimbus stay in the `Ludonimbus` folder. You can restore them under Recently deleted in Ludonimbus for 30 days; after that Ludonimbus deletes them for good.

## Sharing

Ludonimbus does not sell, share, or transfer your data to anyone. It contains no advertising, analytics, or telemetry.

Ludonimbus's use and transfer of information received from Google APIs adheres to the [Google API Services User Data Policy](https://developers.google.com/terms/api-services-user-data-policy), including the Limited Use requirements.

## Your choices

- Disconnect Google Drive at any time in Ludonimbus under Settings → Cloud. This deletes the stored sign-in token from your computer.
- Revoke the app's access at [myaccount.google.com/permissions](https://myaccount.google.com/permissions).
- Delete all data the app stored in your Drive by deleting the `Ludonimbus` folder in Google Drive.
- If you use a local or synced folder instead of Google Drive, no Google data is involved.
- If you connect Baidu Netdisk, see [Baidu Netdisk](#baidu-netdisk) below.
- If you connect Quark, Ludonimbus runs the open-source tool OpenList on your computer (listening only on this computer) and uses it to read and write the "Ludonimbus" folder in your Quark drive. The Quark sign-in is kept only in OpenList's data folder on your computer, and saves travel only between your computer and Quark; the sign-in is deleted when you disconnect.
- If you use a WebDAV server or S3-compatible storage, saves are sent only to the server you entered. Which server you connect to, and what runs behind it, is your own configuration. Its password or secret key is stored in the Windows Credential Manager on your computer and removed when you disconnect.
- If you enter a SteamGridDB API key to pick artwork, Ludonimbus sends the game names you search for to SteamGridDB with your key and downloads the images you choose. The key is stored in the Windows Credential Manager and can be deleted in Settings. No save data is sent to SteamGridDB.
- Ludonimbus connects to GitHub to check for new versions of itself, and lets Ludusavi download its game database from GitHub. These requests contain no save data or account information.

## Baidu Netdisk {#baidu-netdisk}

Ludonimbus has no built-in Baidu Netdisk app. This applies only if you connect Baidu Netdisk with an app you registered on the Baidu Netdisk open platform yourself; the folder in your netdisk and the name shown in Baidu's authorization list are your app's name, with Ludonimbus used as the example below.

**Authorization.** Ludonimbus connects through the official API of the Baidu Netdisk open platform with the OAuth 2.0 device-code flow: it shows a code and a QR code, and you sign in to your Baidu account on Baidu's own authorization page (`openapi.baidu.com`) or in the Baidu Netdisk app, review the permissions and confirm. Your Baidu password is only entered on Baidu's page; Ludonimbus never sees it. The requested scope is `basic` and `netdisk`, as shown on the authorization page. Ludonimbus does not request or use full-drive access or user-level file deletion.

**Data processed.**

| Data | Purpose | Where it is kept |
|---|---|---|
| Authorization tokens (access / refresh token) | Access Baidu Netdisk on your behalf | Only in the Windows Credential Manager on your computer; deleted when you disconnect |
| Netdisk nickname | Show the connected account in Settings | Local settings file `%APPDATA%\Ludonimbus\config.json` |
| Used / total space | Show storage use in Settings | Not stored; read when Settings opens |
| The saves and related data you choose to sync | Backup, restore and sync between your own computers | The folder 我的应用数据/Ludonimbus (My App Data/Ludonimbus) in your Baidu Netdisk |

The account-info API also returns fields such as the membership type; Ludonimbus neither stores nor uses them. The app folder (`/apps/Ludonimbus` in the netdisk) holds compressed copies of your saves with the metadata of each version (time, device name, file names, sizes, checksums, an optional note), one small file per computer (device name, operating system, app version), the game definitions, and, if you turn those features on, play time, Steam screenshots, custom artwork, achievement records and game backups.

**Access scope.** Ludonimbus only reads and writes that one folder and never lists, reads, changes or deletes any other file in your netdisk. Baidu's open platform also restricts apps to their own folder on the server side, and Ludonimbus drops `..` from paths so that no path can point outside it.

**Revoking access.**

1. In Ludonimbus: Settings → Cloud → Disconnect. This deletes the Baidu authorization stored on your computer, and Ludonimbus stops accessing your Baidu Netdisk.
2. On Baidu's side: sign in to your Baidu account and open 账号设置 → 授权管理 (Account settings → Authorization management, [passport.baidu.com/v6/appAuthority](https://passport.baidu.com/v6/appAuthority)), find Ludonimbus and click 取消授权 (Revoke). The Ludonimbus authorization on every computer stops working at once.

The Baidu Netdisk entry in Ludonimbus's Settings explains both steps under "How do I revoke access?" and opens Baidu's page directly.

**Deleting data.**

- Deleting a save version in its version history moves it to Recently deleted, where you can restore it for **30 days**; after that it is deleted for good. You can also delete it for good from Recently deleted yourself.
- Automatic cleanup deletes older automatic versions according to your retention settings. Locked versions, manual backups and the latest version are never cleaned up automatically.
- To delete everything Ludonimbus stored in Baidu Netdisk, delete the folder 我的应用数据/Ludonimbus. Revoking access does not delete these files by itself.
- All of these deletions happen only inside 我的应用数据/Ludonimbus.

**Your Baidu app's keys.** You enter your app's AppKey, SecretKey and app name to connect. The SecretKey is kept only in the Windows Credential Manager (the system keyring on Linux, or a file only your account can read when there is none) and is never written to the settings file; it is deleted when you disconnect or switch to another app. The AppKey and app name are not secret and are kept in the local settings file. All three are sent only to Baidu (`openapi.baidu.com`, when signing in and renewing the authorization), never to the Ludonimbus developer or any third party.

## Children

Ludonimbus is not directed at any particular age group, collects no one's personal information, and does not specifically process information about minors.

## Changes and contact

Changes to this policy will be published on this page and in the [GitHub repository](https://github.com/Shupian0510/Ludonimbus), and the effective date at the top will be updated. Questions: please open an issue at [github.com/Shupian0510/Ludonimbus/issues](https://github.com/Shupian0510/Ludonimbus/issues). Security issues: please report them privately through [GitHub private vulnerability reporting](https://github.com/Shupian0510/Ludonimbus/security/advisories/new) rather than in public. See also the [Terms of Service](./terms.md).
