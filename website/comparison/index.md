---
title: Ludonimbus 与 Ludusavi、OpenSave、Steam 云存档、Syncthing 对比
titleTemplate: false
description: Ludonimbus 和 Ludusavi、OpenSave、Steam 云存档以及 Syncthing 等文件夹同步工具的对比：从 Steam 启动时的同步、版本历史、模拟器支持、存档放在哪里和支持的系统。
---

# 和其他存档工具对比

管理游戏存档的免费工具有好几个，它们解决的问题各不相同。这一页把 Ludonimbus 和大家问得最多的几个工具放在一起比较，方便你挑选，或者搭配使用。

::: info 关于这份对比
其他项目的信息来自它们自己的文档，核对于 2026 年 11 月。如果有过时的地方，欢迎[提 Issue](https://github.com/Shupian0510/Ludonimbus/issues)。
:::

## 一句话选择

- **Ludonimbus**：你在 **Windows 上通过 Steam** 玩非 Steam 游戏（模拟器、重编译游戏、其他商店的游戏），想让它们像 Steam 云存档一样同步，存档放在自己的云盘里。
- **Ludusavi**：你想要一个灵活、跨平台的**备份工具**，给上万款 PC 游戏手动、定时或用命令行备份。
- **OpenSave**：你想在 PC、Mac、Linux 和 Steam Deck 之间**设备对设备**直接同步，不用云盘账号。
- **Steam 云存档**：Steam 商店里支持云存档的游戏，什么都不用装。
- **Syncthing 或同步文件夹**：你只想让一个文件夹在几台机器间保持一致，其他的自己处理。

## 对比表

| | Ludonimbus | Ludusavi | OpenSave | Steam 云存档 | Syncthing / 同步文件夹 |
|---|---|---|---|---|---|
| **是什么** | Steam 里非 Steam 游戏的存档同步 | 存档备份与恢复工具 | 点对点存档同步 | Valve 内置的云存档 | 通用文件同步 |
| **适用游戏** | Steam 商店以外的模拟器、重编译 / 移植游戏和 PC 游戏 | 数据库里的 PC 游戏（19,000 多款）和自定义条目 | Ludusavi 数据库里的游戏、模拟器预设、自定义文件夹 | 开发者开启了云存档的 Steam 商店游戏 | 你选的任意文件夹 |
| **开始游戏前拿到最新存档** | 从 Steam 启动时自动拉取 | 把 `ludusavi wrap` 设为启动参数 | 设备在线时互相同步 | 是 | 要看文件夹有没有同步完 |
| **退出游戏后** | 游戏和启动器都退出后上传 | 用 `wrap`；否则在运行备份时 | 被追踪的游戏关闭时生成快照并同步 | 是 | 文件一变就同步 |
| **存档放在哪里** | 你的云盘：Google Drive、夸克、WebDAV / NAS、S3 或任意文件夹 | 本地备份文件夹，可以用 rclone 上传 | 你自己的设备上，可选云端镜像 | Valve 的服务器 | 你自己的设备或同步服务 |
| **版本历史** | 每次同步一个版本：可恢复、锁定、写备注、导出 zip | 完整和差异备份，保留数量可设置 | 快照和分支 | 没有可浏览的历史 | 取决于工具的文件版本功能 |
| **冲突** | 让你选保留哪边，另一边留在历史里 | 不适用（备份工具） | 保留自己的、对方的，或者都留 | 问你用本地还是云端 | 生成文件冲突副本 |
| **模拟器** | 内置 19 个，大部分按游戏 | 自定义条目 | RetroArch、Dolphin、Ryujinx、yuzu、Citra、PCSX2、RPCS3、PPSSPP、Cemu、Xenia 预设 | — | 整个文件夹 |
| **重编译游戏** | 按程序认出 140 多个 | 数据库和自定义条目 | 数据库和自定义文件夹 | — | 手动 |
| **Steam 里** | 游戏页「开始游戏」栏显示状态（Millennium 插件），一键连封面加入 Steam | — | Steam Deck 游戏模式插件 | 内置 | — |
| **系统** | Windows 10 / 11 | Windows、Linux、macOS、Steam Deck | Windows、Linux、macOS、Steam Deck | 能运行 Steam 的系统 | 大多数桌面系统 |
| **许可** | MIT | MIT | MIT | 专有 | Syncthing：MPL-2.0 |

## Ludonimbus 和 Ludusavi

[Ludusavi](https://github.com/mtkennerly/ludusavi) 是很优秀的备份工具，Ludonimbus 也建立在它之上：普通 PC 游戏的存档位置来自 Ludusavi 的数据库，Ludonimbus 的安装包也自带 Ludusavi 官方程序。

两者的用途不同：

- Ludusavi 一次备份和恢复很多游戏的存档。它的 `wrap` 命令可以在游戏前恢复、游戏后备份，云端上传通过 rclone 实现，需要在 Ludusavi 里配置。
- Ludonimbus 针对的是**你点「开始游戏」的那一个游戏、那一刻**。开启 Steam 自动同步会替你改好快捷方式；同步、冲突和版本历史都按游戏处理；云盘登录是内置的（Google Drive 在浏览器里登录，夸克扫码登录）；并在 Ludusavi 数据库之外加上了模拟器和重编译游戏的规则。

如果你已经在用 Ludusavi 备份所有游戏，可以继续用。Ludonimbus 只管你添加给它的游戏。

## Ludonimbus 和 OpenSave

[OpenSave](https://www.opensave.org/) 在你的设备之间直接同步存档，走局域网或者中继，也可以镜像到云端文件夹。它支持 Windows、Linux、macOS 和 Steam Deck。

- 如果你在多种设备上玩，特别是 Steam Deck 或 Mac，或者根本不想把存档放到云端，选 OpenSave。
- 如果你在 Windows 电脑上通过 Steam 玩，希望每次启动前都从自己的云盘拉取最新存档，还想要按游戏的模拟器规则和 Steam 里的状态显示，选 Ludonimbus。

## Ludonimbus 和 Steam 云存档

Ludonimbus 模仿的就是 Steam 云存档的体验，对 Steam 商店游戏来说有它就够了。Ludonimbus 会认出 Steam 云存档已经在同步的游戏并单独列出，不会重复管理。区别在于范围：Steam 云存档只对开发者开启了它的商店游戏有效；Ludonimbus 管的是游戏库里的其他所有游戏，存档放在你自己的云盘里。

## Ludonimbus 和 Syncthing、同步文件夹

Syncthing、OneDrive、Dropbox 或 Google Drive 桌面版同步的是文件夹，对游戏一无所知。如果游戏只把存档放在一个文件夹里，而且你从不在两台电脑上同时玩，这样就够了。Ludonimbus 补上的是文件夹同步不知道的事：每个游戏的存档在哪里、游戏什么时候开始和退出、冲突时保留哪边，以及每次游戏一个完整的存档版本。你也可以把同步文件夹**当作** Ludonimbus 的云盘，见[本地 / 同步文件夹](../clouds/folder.md)。

## 其他工具

- **EmuDeck CloudSync**：通过 rclone 同步 EmuDeck 安装的模拟器存档。
- **Decky 插件**：Decky Cloud Save、Decky Ludusavi 等，在 Steam Deck 的游戏模式里备份存档。

## 试试 Ludonimbus

[下载 Windows 版](https://github.com/Shupian0510/Ludonimbus/releases/latest) · [快速上手](../guide/quick-start.md) · [支持的游戏](../supported-games/index.md)
