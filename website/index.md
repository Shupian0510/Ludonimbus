---
layout: home
title: Ludonimbus
titleTemplate: 给非 Steam 游戏补上云存档

hero:
  name: Ludonimbus
  text: 给 Steam 里的非 Steam 游戏补上云存档
  tagline: 模拟器、重制版、第三方 PC 游戏，像 Steam 云存档一样自动同步。存档放在你自己的云盘里，没有任何第三方服务器。
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
      text: GitHub
      link: https://github.com/Shupian0510/Ludonimbus

features:
  - icon: ☁️
    title: 和 Steam 云存档一样
    details: 照常从 Steam 点「开始游戏」。启动前拉取云端较新的存档，退出后上传新存档，Steam 照常记录游戏时间。
    link: /sync/how-it-works
    linkText: 同步怎样工作
  - icon: 🎮
    title: 在 Steam 里直接看到状态
    details: 游戏页的「开始游戏」栏显示「已是最新」「正在上传」「存档冲突」等状态，还能一键同步。
    link: /steam/status-in-steam
    linkText: 一键安装
  - icon: 🔍
    title: 添加游戏不用自己找存档
    details: 扫描 Steam 快捷方式、Epic / GOG / Xbox 的游戏、模拟器里的存档和上万款 PC 游戏，勾选后批量添加。
    link: /games/scan
    linkText: 扫描本机
  - icon: ➕
    title: 一键加入 Steam
    details: 不在 Steam 里的游戏点一下就能加进 Steam，从第一次启动起就自动同步，封面可以从 SteamGridDB 自动配好。
    link: /steam/add-to-steam
    linkText: 加入 Steam
  - icon: 🕘
    title: 版本历史
    details: 每个版本都记录时间、设备和来源，可以恢复、导出 zip、锁定、写备注。旧版本自动清理，手动备份永久保留。
    link: /sync/versions
    linkText: 版本历史
  - icon: 🤝
    title: 冲突时两份都留
    details: 两台电脑都改过存档时让你选择用哪一份，没选的那份也留在版本历史里。连不上云端也能照常玩。
    link: /sync/conflicts
    linkText: 存档冲突
  - icon: 🧩
    title: 模拟器和重编译游戏
    details: 19 个模拟器（Dolphin、Ryujinx、PCSX2…）和 140 多个重编译 / 移植游戏开箱即用，模拟器更新也不断档。
    link: /supported/emulators
    linkText: 支持的游戏
  - icon: 🗄️
    title: 云盘随你选
    details: Google Drive、坚果云、群晖等 NAS、Nextcloud、阿里云 OSS 等对象存储，或者任意文件夹。换云盘时一键复制历史版本。
    link: /clouds/
    linkText: 怎么选云盘
---

<div class="vp-doc" style="max-width: 1152px; margin: 48px auto 0; padding: 0 24px;">

**系统要求**：Windows 10 / 11（64 位），已安装 Steam。免费开源（MIT 许可）。第一次用请看[下载与安装](./guide/install.md)和[快速上手](./guide/quick-start.md)。

**English** — Ludonimbus brings Steam-Cloud-like save sync to non-Steam games (emulators, recompilations and third-party PC games) using your own Google Drive, NAS, WebDAV / S3 storage or any folder. [Read the English documentation →](./en/index.md) · [Privacy Policy](./privacy.md)

</div>
