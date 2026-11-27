---
title: 非 Steam 游戏云存档
description: 给 Steam 游戏库里的非 Steam 游戏补上和 Steam 云存档一样的自动同步：模拟器、重编译游戏，以及 Epic、GOG、Xbox 等商店的游戏。免费开源，带版本历史，存档放在你自己的云盘里。
---

# 非 Steam 游戏云存档

Steam 云存档只同步 Steam 商店里买的游戏。用**「添加非 Steam 游戏」**加进来的其他游戏，Steam 只是建了一个快捷方式，存档仍然只留在这一台电脑上。

Ludonimbus 让这些游戏也拥有 Steam 云存档的体验：照常在 Steam 里点「开始游戏」，启动前拉取云端较新的存档，退出后上传新存档，每个版本都保存在**你自己的**云盘里。

[下载 Windows 版](https://github.com/Shupian0510/Ludonimbus/releases/latest) · [快速上手](../guide/quick-start.md)

## 为什么非 Steam 游戏容易丢存档

- **Steam 云存档管不到快捷方式。** 它只对开发者开启了云存档的商店游戏有效。
- **存档位置五花八门。** 有的在 `%APPDATA%`，有的在「文档」或「保存的游戏」，还有的就在游戏程序旁边。
- **更新会换文件夹。** 很多模拟器、重编译和移植游戏的更新方式是把新版本解压到新文件夹，存档留在了旧文件夹里。
- **换电脑就是空存档。** 重装系统或者换电脑，要手动把每个存档文件夹找出来复制过去。
- **同步文件夹不够用。** OneDrive、Dropbox 能同步一个文件夹，但它们不知道游戏什么时候开始、什么时候退出：没法保证开始游戏前拿到的是最新存档，还可能在游戏写存档写到一半时就上传。

## Ludonimbus 怎样同步

```text
Steam「开始游戏」
  ↓
ludonimbus-run：从你的云盘拉取较新的存档
  ↓
启动游戏，等它和启动器完全退出
  ↓
把新存档作为一个新版本上传
```

给一个游戏开启 [Steam 自动同步](../steam/auto-sync.md)后，它的 Steam 快捷方式会先启动 Ludonimbus 的小程序 `ludonimbus-run`，由它在游戏前后完成同步。快捷方式的**名称、图标、封面和游戏时间都不变**，Steam 截图和手柄设置也照常可用。

![Ludonimbus 游戏页：已开启 Steam 自动同步](/screenshots/zh-CN/game-detail-steam.png)

## 适合哪些游戏

| 游戏类型 | 例子 | 怎么找到存档 |
|---|---|---|
| 模拟器游戏 | Dolphin、Cemu、Ryujinx、RPCS3、PCSX2、PPSSPP、RetroArch | 内置 19 个模拟器的规则，大部分按游戏同步。见[模拟器存档同步](../emulator-cloud-saves/index.md) |
| 重编译 / 移植游戏 | Zelda 64: Recompiled、Ship of Harkinian、Unleashed Recompiled、OpenGOAL | 按游戏程序认出 140 多个。见[重编译游戏云存档](../recompiled-game-saves/index.md) |
| 其他商店的游戏 | Epic、GOG、Xbox 应用（PC Game Pass）、EA、育碧 | Ludusavi 数据库，覆盖上万款游戏 |
| 其他 PC 游戏 | 免 DRM 游戏、独立游戏、Mod | Ludusavi 数据库，或者用[找存档向导](../games/save-finder.md) |

完全不在 Steam 里的游戏，可以[一键加入 Steam](../steam/add-to-steam.md)，封面从 SteamGridDB 自动配好，第一次启动就开始同步。Steam 云存档已经在同步的游戏会单独列出，不会重复管理。

## 除了同步还有什么

- **[版本历史](../sync/versions.md)**：每次上传都留下一份存档，记着时间、设备和来源，可以设为当前、保留、起名字、导出给朋友。
- **[冲突时两份都留](../sync/conflicts.md)**：两台电脑都改过存档时，让你选用哪一份，另一份也留着。
- **[离线也能玩](../sync/offline.md)**：连不上云端时照常启动游戏，联网后再上传。
- **[游戏换了位置也能找回](../games/moved.md)**：存档按游戏目录的相对位置记录，重装、换文件夹、换电脑后，第一次启动就会恢复。
- **[在 Steam 里显示云存档状态](../steam/status-in-steam.md)**：可选插件，在 Steam 游戏页显示「已是最新」「正在上传」「存档冲突」。

## 存档放在哪里

Ludonimbus 没有服务器，也不用注册账号。存档放在你选的地方：

- [Google Drive](../google-drive-save-sync/index.md)（在浏览器里登录一次）
- [夸克网盘](../clouds/quark.md)（扫码登录，实验性）
- 坚果云、群晖等 NAS、Nextcloud 或任何 [WebDAV](../clouds/webdav.md) 服务
- 阿里云 OSS、腾讯云 COS、Cloudflare R2 等 [S3 兼容存储](../clouds/s3.md)
- 任意[文件夹](../clouds/folder.md)，包括 OneDrive、Dropbox 的同步文件夹和 U 盘

每个版本都是普通的 zip 文件，不装 Ludonimbus 也能打开。

## 四步开始

1. [下载并安装](../guide/install.md) Ludonimbus（Windows 10 / 11，64 位）。
2. 在首次使用引导里连接云盘。
3. 「添加游戏 → 扫描本机」，勾选要同步的游戏。
4. 在每个游戏的页面开启「Steam 自动同步」；还不在 Steam 里的游戏，点「加入 Steam…」。

之后在任何一台电脑上都从 Steam 启动游戏就行。

## 常见问题

**一定要从 Steam 启动吗？**
从 Steam 启动，Ludonimbus 才能在游戏开始前先拉取最新存档。用其他方式启动时，只要 Ludonimbus 在托盘里运行，游戏退出后照样会上传存档。

**会改动我的 Steam 快捷方式吗？**
只改开启了自动同步的游戏，而且只改它启动的程序。关闭自动同步就会恢复原来的快捷方式。

**支持 macOS、Linux 或 Steam Deck 吗？**
Windows 10 和 11 是主要平台。从 1.0 开始有 [Linux 和 Steam Deck 版](../platforms/linux.md)（预览）。macOS 暂时不支持。

**收费吗？**
免费。Ludonimbus 以 MIT 许可开源，只需要你已有的云盘或存储。
