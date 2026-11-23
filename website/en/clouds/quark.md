# Quark (experimental)

Since 0.5, Quark connects in one click: sign in by scanning a QR code with the Quark app, and Ludonimbus does the rest.

::: info Why "experimental"
Quark has no official API for third-party apps. Ludonimbus connects through the open-source tool [OpenList](https://github.com/OpenListTeam/OpenList), whose Quark driver uses the Quark website's own interface, so a change on Quark's side can break it for a while. Even then no saves are lost: they stay on your PC, failed syncs are reported clearly, and you can switch to another cloud at any time and copy your version history over (see [Switching clouds](./switching.md)).
:::

## Connect

1. Open Settings → Cloud and find "Quark (Experimental)".
2. The first time, the OpenList component has to be downloaded: click "Download the component and connect". It is about 57 MB, comes from OpenList's official GitHub releases, and its SHA-256 is checked.
   - If GitHub is slow, download [openlist-windows-amd64.zip](https://github.com/OpenListTeam/OpenList/releases/download/v4.2.10/openlist-windows-amd64.zip) (the official OpenList v4.2.10 package) in your browser, then click "GitHub too slow? Choose a downloaded package…" and pick it. Any other file is refused.
3. A "Sign in to Quark" window opens with Quark's own website. Scan the QR code with the Quark app (or use another way the page offers).
4. Once you're signed in, the window closes by itself and Settings shows that Quark is connected.

![Settings → Cloud → Quark](/screenshots/en/settings-cloud-quark.png)

![The "Sign in to Quark" window](/screenshots/zh-CN/quark-login-window.png)

You can click Cancel or simply close the sign-in window while signing in.

## Where saves go

The "Ludonimbus" folder at the top of your Quark drive, visible on the website and in the app. Ludonimbus only reads and writes that folder and can't see anything else in your drive.

## Speed

Measured in November 2026 with a free account:

| What | Time |
|---|---|
| Checking the cloud before a game starts from Steam | about 1–2 s |
| Uploading a 1 MB save version | about 12 s |
| Downloading a 1 MB save version | about 2 s |

Slower than other clouds, but uploads happen after you quit the game, so they don't hold you up. The first upload of each game takes about 10 seconds longer (its folder is created first).

## When the sign-in expires

The sidebar then shows "Cloud needs reconnecting" and Settings says the Quark sign-in has expired. Click "Scan to sign in again" and scan once more. The component isn't downloaded again and your saves aren't affected.

## About the OpenList component

- OpenList is an open-source (AGPL-3.0) tool that mounts cloud drives. Ludonimbus doesn't modify or bundle it; it downloads the official release only after you agree and runs it as is.
- It listens only on this PC (`127.0.0.1`) with a random admin password, and syncing uses an account that can only access the "Ludonimbus" folder. Both passwords are kept in Windows Credential Manager.
- The Quark sign-in is kept only in OpenList's data folder, `%APPDATA%\Ludonimbus\openlist\data`; Ludonimbus doesn't store it itself. The sign-in window is private and keeps nothing once closed.
- It only runs while you use Quark: alongside Ludonimbus, and it exits with it. When a game starts from Steam without Ludonimbus open, it starts for that sync and stops afterwards. It uses about 70 MB of memory while running.
- Saves travel only between your PC and Quark, never through any third-party server.

## Disconnect

Click Disconnect in Settings → Cloud: OpenList stops and its data folder (with the Quark sign-in) is deleted. The program itself stays, so connecting again doesn't download it again. The saves in your Quark drive are not deleted.

## Already running your own OpenList?

You can keep using it and connect over WebDAV instead; see [WebDAV → OpenList / AList](./webdav.md#openlist). Don't use both ways on the same folder.
