---
title: Steam 云存档插件与 Steam 集成
description: Ludonimbus 和 Steam 深度集成：点「开始游戏」时自动同步存档，Steam 客户端插件在游戏页显示云存档状态，非 Steam 游戏还能连同 SteamGridDB 封面一键加入 Steam。
---

# Steam 集成

Ludonimbus 是围绕 Steam 设计的：你照常在 Steam 游戏库里点**「开始游戏」**，非 Steam 游戏就有了和 Steam 云存档一样的云存档。它分三部分，可以只用其中一部分。

[下载 Windows 版](https://github.com/Shupian0510/Ludonimbus/releases/latest)

## 1. 点「开始游戏」就同步

在游戏页面打开 **Steam 自动同步**。之后它的 Steam 快捷方式会先启动 `ludonimbus-run`，由它：

1. 从你的云盘拉取较新的存档（两边都改过时会让你选）；
2. 启动游戏，等游戏和启动器完全退出；
3. 把新存档作为一个新版本上传。

快捷方式的**名称、图标、封面和游戏时间都不变**，Steam 截图和手柄设置也照常可用。关闭自动同步就会恢复原来的快捷方式。

Steam 运行时会覆盖对快捷方式的改动，所以 Ludonimbus 会先短暂关闭 Steam，改好后再重新打开。见 [Steam 自动同步](../steam/auto-sync.md)。

## 2. 在 Steam 里显示云存档状态

可选的 Steam 客户端插件会在每个由 Ludonimbus 管理的游戏页上，在「开始游戏」那一栏加一项 **Ludonimbus 云存档**，和「最后运行日期」「游戏时间」并排：

- 实时更新，例如「已是最新 · 5 分钟前」「游戏运行中，退出后自动上传」「正在上传存档…」「存档冲突，点击选择保留哪一份」；
- 点右边的按钮立即同步，点状态本身会打开 Ludonimbus 里的这个游戏；
- 普通的 Steam 游戏不受影响。

插件基于开源的 Steam 客户端插件框架 [Millennium](https://github.com/SteamClientHomebrew/Millennium)。在 Ludonimbus 的设置里点一下就能装好：从 Millennium 官方 GitHub 发布页下载最新稳定版，校验 SHA-256 后安装，和 Millennium 官方安装程序的步骤一样。Millennium 不是 Valve 官方组件，插件随时可以在同一个地方移除。

![设置：在 Steam 里显示云存档状态](/screenshots/zh-CN/settings-steam-plugin.png)

插件只负责显示信息。它通过只在本机监听、每次启动都会生成新令牌的本地接口向 Ludonimbus 托盘程序询问状态，从不接触你的云盘账号。见[在 Steam 里显示云存档状态](../steam/status-in-steam.md)。

## 3. 一键加入 Steam，封面自动配好

还不在 Steam 里的游戏（包括 Xbox 应用 / PC Game Pass 的游戏）可以一键或批量加入 Steam，第一次启动就开始同步。

- 封面、背景、Logo 和图标可以自动配好，也可以从 [SteamGridDB](https://www.steamgriddb.com/) 挑选（需要你自己的免费 API Key），或者用本地图片。
- 可以选择加到哪个 Steam 账号。

![加入 Steam 并配好封面](/screenshots/zh-CN/add-to-steam.png)

见[一键加入 Steam](../steam/add-to-steam.md) 和[封面与 SteamGridDB](../steam/artwork.md)。

## 像 Steam 一样的游戏库

Ludonimbus 的窗口用网格或列表展示游戏，直接使用你在 Steam 里设置的封面、横幅和 Logo；不在 Steam 里的游戏也能配封面。Steam 云存档已经在同步的游戏会被认出来并单独列出，不会重复管理。

## 需要知道的

- Ludonimbus 运行在 **Windows 10 / 11** 上。Steam 插件只在 Steam 的桌面模式显示，Big Picture 里没有。
- 插件需要 Ludonimbus 在托盘里运行；没运行时 Steam 和游戏照常使用，自动同步也照常工作。
- 一台电脑上有多个 Steam 账号时，在「设置 → Steam」里选择账号。

## 常见问题

**这是 Steam 官方的云存档功能吗？**
不是。Ludonimbus 是独立的开源项目，存档放在你自己的存储里，不在 Valve 的服务器上。

**Steam 还会记录游戏时间吗？**
会。快捷方式还是原来那一个，游戏时间、截图和手柄设置都保留。

**不装插件能同步吗？**
能。插件只是在 Steam 里显示状态，同步是由快捷方式完成的，不装插件也照常工作。
