# Quick start

Once Ludonimbus is installed, four steps get you going. The [first-run guide](./first-run.md) that opens the first time covers step 1 and opens the scan of step 2.

## 1. Connect a cloud

Open Settings → Cloud and pick one:

| Cloud | How to connect |
|---|---|
| **Google Drive** | Click "Connect Google Drive" and sign in in your browser. See [Google Drive](../clouds/google-drive.md) |
| **Quark** | Scan a QR code with the Quark app (experimental). See [Quark](../clouds/quark.md) |
| **WebDAV** | A NAS (Synology, QNAP…), Nextcloud, Jianguoyun, or a WebDAV service you run yourself, such as OpenList / AList. Enter the address, user name and password, then click "Test connection". See [WebDAV](../clouds/webdav.md) |
| **S3-compatible storage** | MinIO on a NAS, Cloudflare R2, Aliyun OSS, Tencent COS. See [S3-compatible storage](../clouds/s3.md) |
| **Folder** | A OneDrive / Google Drive desktop sync folder, a NAS share, a USB drive. See [Local / synced folder](../clouds/folder.md) |

Not sure which? See [Choosing a cloud](../clouds/index.md).

![Settings → Cloud](/screenshots/en/settings-cloud.png)

## 2. Add games

Click "Add game" at the top right of the library. "Scan this PC" lists the games it found:

- **Ready to add**: Ludonimbus already knows where the saves are. Tick them and click "Add to Ludonimbus".
- **Needs a save location**: click "Choose saves…" and follow the [save finder](../games/save-finder.md).
- For games kept elsewhere, click "Scan folder…" and pick the folder.

See [Scan this PC](../games/scan.md).

![Add game → Scan this PC](/screenshots/en/add-game-scan.png)

## 3. Turn on Steam auto-sync

- **Games already in Steam**: on the game's page, turn on "Steam auto-sync". See [Steam auto-sync](../steam/auto-sync.md).
- **Games not in Steam yet**: click "Add to Steam…". You can add several at once, with artwork. See [Add to Steam](../steam/add-to-steam.md).

Changing Steam shortcuts requires closing Steam briefly; Ludonimbus closes and reopens it for you.

![Game page](/screenshots/en/game-detail.png)

## 4. Play as usual

From now on, start games from Steam: newer saves are pulled before the game starts and new saves are uploaded after it exits.

When you close the window, Ludonimbus stays in the system tray. Games you start outside Steam are backed up after they exit, too.

## Optional: see the status in Steam

In Settings → "Show cloud save status in Steam", click "Install". The play bar of a game's Steam library page then shows statuses like "up to date" or "uploading", with a sync button. See [Cloud status inside Steam](../steam/status-in-steam.md).

## Interface language {#labels}

Since 0.5, Ludonimbus shows its interface in English when Windows uses a language other than Chinese, and you can pick it in Settings → "Language / 语言" (see [Settings](../reference/settings.md#language)). Version 0.4 was Chinese only; if you're still on it, the labels used in this guide are in these places:

| This guide | In the app |
|---|---|
| Library · Devices · Settings | 游戏库 · 设备 · 设置 |
| Settings → Cloud | 设置 → 云盘 |
| Add game → Scan this PC | 添加游戏 → 扫描本机 |
| Scan folder… | 扫描文件夹… |
| Test connection · Connect | 测试连接 · 连接 |
| Steam auto-sync | Steam 自动同步 |
| Add to Steam… | 加入 Steam… |
| Sync now · Back up current saves | 立即同步 · 备份当前存档 |
| Saves · Files on this PC · Settings (game tabs) | 存档 · 本机文件 · 设置 |

## What next

- Several PCs? See [Multiple devices](../sync/devices.md).
- Want to go back to an earlier save? See [Saves](../sync/versions.md).
- Something wrong? See [FAQ and troubleshooting](../reference/faq.md).
