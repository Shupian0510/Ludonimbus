# 快速上手

装好 Ludonimbus 后，四步就能用起来。第一次打开时的[首次使用引导](./first-run.md)会带你完成第 1 步，并直接打开第 2 步的扫描。

## 1. 连接云盘

打开「设置 → 云盘」，选一种：

| 云盘 | 怎么连接 |
|---|---|
| **Google Drive** | 点「连接 Google Drive」，在浏览器里登录即可。见 [Google Drive](../clouds/google-drive.md) |
| **夸克网盘** | 用夸克 App 扫码登录（实验性）。见[夸克网盘](../clouds/quark.md) |
| **WebDAV** | 坚果云、群晖 / 威联通等 NAS、Nextcloud，或者通过 OpenList / AList 接入百度网盘、阿里云盘等。填地址、用户名和密码，先点「测试连接」。见 [WebDAV](../clouds/webdav.md) |
| **S3 兼容存储** | NAS 上的 MinIO，或阿里云 OSS、腾讯云 COS、Cloudflare R2。见 [S3 兼容存储](../clouds/s3.md) |
| **文件夹** | OneDrive / Google Drive 电脑版同步的文件夹、NAS 共享目录、移动硬盘。见[本地 / 同步文件夹](../clouds/folder.md) |

不知道选哪个？看[怎么选云盘](../clouds/index.md)。

![设置 → 云盘](/screenshots/zh-CN/settings-cloud.png)

## 2. 添加游戏

点游戏库右上角的「添加游戏」，在「扫描本机」里会列出找到的游戏：

- **可以直接添加**：Ludonimbus 已经知道存档在哪里，勾选后点「添加到 Ludonimbus」。
- **需要指定存档位置**：点「指定存档…」，按[找存档向导](../games/save-finder.md)的提示找到存档文件夹。
- 放在别处的游戏，点「扫描文件夹…」选放游戏的文件夹。

见[扫描本机](../games/scan.md)。

![添加游戏 → 扫描本机](/screenshots/zh-CN/add-game-scan.png)

## 3. 开启 Steam 自动同步

- **已经在 Steam 里的游戏**：在游戏详情页打开「Steam 自动同步」。见 [Steam 自动同步](../steam/auto-sync.md)。
- **还不在 Steam 里的游戏**：点「加入 Steam…」，可以一次加好几个，还能自动配封面。见[一键加入 Steam](../steam/add-to-steam.md)。

修改 Steam 快捷方式需要暂时关闭 Steam，Ludonimbus 会自动关闭并重新打开它。

![游戏详情页](/screenshots/zh-CN/game-detail.png)

## 4. 照常玩

以后从 Steam 启动游戏就行：启动前拉取云端较新的存档，退出后自动上传。

关闭窗口后 Ludonimbus 会留在系统托盘。不通过 Steam、直接打开游戏时，它也会在游戏退出后自动备份。

## 可选：在 Steam 里看到同步状态

在「设置 → 在 Steam 中显示云存档状态」点「一键安装」，Steam 游戏页的「开始游戏」栏里就会显示「已是最新」「正在上传」这类状态，还能一键同步。见[在 Steam 里显示云存档状态](../steam/status-in-steam.md)。

## 接下来

- 有好几台电脑？看[多台设备](../sync/devices.md)。
- 想把存档恢复到以前的某个时间点？看[版本历史](../sync/versions.md)。
- 遇到问题？看[常见问题与故障排查](../reference/faq.md)。
