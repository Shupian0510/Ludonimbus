---
layout: home
title: Ludonimbus – 开源游戏云存档同步，支持非 Steam 游戏与模拟器
titleTemplate: false
description: 免费开源的 Steam 云存档替代方案：给非 Steam 游戏、模拟器和重编译游戏自动同步存档，带版本历史和 Steam 集成，存档放在你自己的 Google Drive、NAS、WebDAV 或 S3 里。

hero:
  name: Ludonimbus
  text: 给非 Steam 游戏、模拟器和重编译游戏补上 Steam 云存档
  tagline: 免费开源的游戏存档同步工具。开始游戏前拉取最新存档，退出后自动上传，每个版本都留有记录；存档放在你自己的云盘里，没有任何第三方服务器。
  image:
    src: /app-icon.svg
    alt: Ludonimbus
  actions:
    - theme: brand
      text: 下载 Windows 版
      link: https://github.com/Shupian0510/Ludonimbus/releases/latest
    - theme: alt
      text: 快速上手
      link: /guide/quick-start
    - theme: alt
      text: 和其他工具对比
      link: /comparison/
    - theme: alt
      text: GitHub
      link: https://github.com/Shupian0510/Ludonimbus

features:
  - icon: ☁️
    title: 和 Steam 云存档一样
    details: 照常从 Steam 点「开始游戏」。启动前拉取云端较新的存档，退出后上传新存档，Steam 照常记录游戏时间。
    link: /non-steam-cloud-saves/
    linkText: 非 Steam 游戏云存档
  - icon: 🎮
    title: 在 Steam 里直接看到状态
    details: 游戏页的「开始游戏」栏显示「已是最新」「正在上传」「存档冲突」等状态，还能一键同步。
    link: /steam-integration/
    linkText: Steam 集成
  - icon: 🧩
    title: 模拟器按游戏同步
    details: Dolphin、Ryujinx、Cemu、RPCS3、PCSX2 等 19 个模拟器，大部分可以逐个游戏同步，模拟器更新也不断档。
    link: /emulator-cloud-saves/
    linkText: 模拟器存档同步
  - icon: 🛠️
    title: 重编译 / 移植游戏
    details: Zelda 64、Ship of Harkinian、Unleashed Recompiled 等 140 多个重编译和移植游戏，按游戏程序自动认出。
    link: /recompiled-game-saves/
    linkText: 重编译游戏云存档
  - icon: 🕘
    title: 版本历史
    details: 每一份存档都记着时间、设备和来源，可以设为当前、保留、起名字、导出给朋友。旧的自动存档自动清理，保留的一直留着。
    link: /sync/versions
    linkText: 版本历史
  - icon: 🤝
    title: 冲突时两份都留
    details: 两台设备都改过存档时让你选择用哪一份，没选的那份也留着。连不上云端也能照常玩。
    link: /sync/conflicts
    linkText: 存档冲突
  - icon: 🔍
    title: 添加游戏不用自己找存档
    details: 扫描 Steam 快捷方式、Epic / GOG / Xbox 的游戏、模拟器里的存档和上万款 PC 游戏，勾选后批量添加，也能连封面一起加进 Steam。
    link: /supported-games/
    linkText: 支持的游戏
  - icon: 🗄️
    title: 云盘随你选
    details: Google Drive、夸克网盘、坚果云、群晖等 NAS、Nextcloud、阿里云 OSS 等对象存储，或者任意文件夹。换云盘时一键复制历史版本。
    link: /google-drive-save-sync/
    linkText: 用 Google Drive 同步存档
---

<div class="vp-doc home-content">

## 专为 Steam 云存档管不到的游戏

Steam 云存档只同步 Steam 商店里买的游戏。用「添加非 Steam 游戏」加进游戏库的其他游戏，存档只留在一台电脑上：重装游戏、更新模拟器或者换电脑时，很容易就丢了。

<div class="home-cards">

- [**非 Steam 游戏**](./non-steam-cloud-saves/index.md)<br>Epic、GOG、Xbox 应用和免 DRM 的游戏，以及其他从 Steam 启动的游戏。
- [**模拟器**](./emulator-cloud-saves/index.md)<br>GameCube、Wii U、Switch、PS2、PS3、PS4、PSP 等，能按游戏同步的就按游戏同步。
- [**重编译游戏**](./recompiled-game-saves/index.md)<br>N64 重编译、反编译移植和原生重制版，存档常常就放在程序旁边。
- [**Steam 集成**](./steam-integration/index.md)<br>点「开始游戏」就同步，游戏页显示云存档状态，一键连封面加进 Steam。
- [**Google Drive**](./google-drive-save-sync/index.md)<br>用你已有的 15 GB 空间，Ludonimbus 只能看到它自己创建的文件。
- [**支持的游戏**](./supported-games/index.md)<br>19 个模拟器、140 多个重编译 / 移植游戏，以及上万款 PC 游戏。

</div>

## 怎样工作

1. **连接云盘**：Google Drive、夸克网盘、通过 WebDAV 连接的 NAS 或坚果云、S3 兼容存储，或者任意文件夹。
2. **添加游戏**：「扫描本机」会找出 Steam 快捷方式、各商店装的游戏、模拟器里的存档，以及存档位置已知的 PC 游戏。
3. **开启 Steam 自动同步**：游戏的 Steam 快捷方式从此会在游戏前后同步，名称、封面和游戏时间都不变。
4. **照常玩**：在任何一台电脑上从 Steam 点「开始游戏」，最新的存档已经在了。

![Ludonimbus 游戏库](/screenshots/zh-CN/library-grid.png)

## 不只是备份工具

备份工具要你记得去运行，才会把存档文件夹复制一份。Ludonimbus 接管的是你点「开始游戏」的那一刻：游戏启动前确保本机是最新存档，等游戏和启动器都退出后，再上传一个新版本。详见[和 Ludusavi、OpenSave、Steam 云存档、Syncthing 的对比](./comparison/index.md)。

## 免费、开源、不碰你的数据

- **MIT 许可**，源代码在 [GitHub](https://github.com/Shupian0510/Ludonimbus) 上。
- **不用注册，没有服务器**：存档只在你的电脑和你选的云盘之间传输。每个版本都是普通的 zip，不装 Ludonimbus 也能打开。
- **基于开放数据**：存档位置来自 [Ludusavi](https://github.com/mtkennerly/ludusavi) 的数据库和 [RecompLauncher](https://recomplauncher.com/) 的游戏目录，模拟器规则逐个核对过。

**系统要求**：Windows 10 / 11（64 位），已安装 Steam。第一次用请看[下载与安装](./guide/install.md)和[快速上手](./guide/quick-start.md)。· [隐私政策](./privacy.md)

**English** — Ludonimbus is an open-source Steam Cloud alternative for non-Steam games, emulators and recompiled games, using your own Google Drive, NAS, WebDAV / S3 storage or any folder. [Read the English site →](./en/index.md)

</div>
