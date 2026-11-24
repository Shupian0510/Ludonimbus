---
title: 模拟器存档同步与云存档
description: 在多台电脑之间同步模拟器存档，按游戏保留版本历史：Dolphin、Ryujinx、Cemu、RPCS3、PCSX2、shadPS4、PPSSPP、RetroArch 等。只同步存档，不碰密钥、固件和即时存档。
---

# 模拟器存档同步

模拟器把存档放在自己的文件夹里，便携版和安装版的位置往往还不一样；把新版本解压到新文件夹时，存档很容易被留在旧文件夹。Ludonimbus 知道 19 个模拟器的存档在哪里，把它们同步到你自己的云盘，模拟器支持时**逐个游戏单独同步**。

[下载 Windows 版](https://github.com/Shupian0510/Ludonimbus/releases/latest) · [完整的模拟器说明](../supported/emulators.md)

## 支持的模拟器

| 模拟器 | 平台 | 同步方式 |
|---|---|---|
| Dolphin | GameCube / Wii | 按游戏 |
| Cemu | Wii U | 按游戏 |
| Azahar、Citra、Lime3DS | 3DS | 按游戏 |
| Ryujinx | Switch | 按游戏 |
| Eden、Citron、Sudachi、Suyu、yuzu | Switch | 整个模拟器 |
| RPCS3 | PS3 | 按游戏 |
| shadPS4 | PS4 | 按游戏 |
| PPSSPP | PSP | 按游戏 |
| Vita3K | PS Vita | 按游戏 |
| PCSX2 | PS2 | 整个模拟器 |
| DuckStation | PS1 | 整个模拟器 |
| Xenia | Xbox 360 | 整个模拟器 |
| RetroArch | 多平台 | 按游戏（按游戏文件名） |

便携版和安装版都能找到。每个模拟器的存档位置和注意事项见[模拟器](../supported/emulators.md)。

## 按游戏还是整个模拟器

- **按游戏**：每个游戏单独一份存档和版本历史，恢复一个游戏的旧版本不会影响其他游戏。模拟器支持时默认用这种方式。
- **整个模拟器**：模拟器里所有游戏的存档一起同步。用在存档没法拆开的情况：PCSX2 和 DuckStation 的记忆卡是所有游戏共用的，yuzu 系 Switch 模拟器的存档和本机用户档案绑定。

## 同步什么、不同步什么

| 同步 | 不同步 |
|---|---|
| 存档文件和记忆卡 | 密钥（`prod.keys` 等）和固件 |
| 每个游戏的存档文件夹（`savedata`、`title`、`SAVEDATA`…） | 着色器缓存、日志和游戏文件 |
| 存档依赖的用户档案 | 即时存档（state） |

Ludonimbus 从不改动你的存档：上传时打包一份副本，恢复时把某个版本的文件放回去。

## 怎么添加模拟器游戏

- **扫描本机**：「添加游戏 → 扫描本机」会把模拟器里已有存档的游戏逐个列出来，不需要先建 Steam 快捷方式。能找到游戏文件时（Ryujinx、Dolphin 的游戏文件夹，Cemu 的游戏列表），加入 Steam 后会直接启动这个游戏。
- **Steam ROM Manager 建的快捷方式**：快捷方式启动的是游戏文件时，Ludonimbus 从光盘镜像、游戏文件夹或 NSP / XCI 读出游戏 ID，只同步这个游戏的存档。
- **只打开模拟器本身的快捷方式**会把整个模拟器的存档一起同步。

![扫描本机：列出模拟器里有存档的游戏](/screenshots/zh-CN/add-game-scan.png)

## 模拟器更新不断档

Steam 快捷方式里只记游戏的编号，模拟器的路径由 Ludonimbus 记着。把模拟器的新版本解压到新文件夹时，Ludonimbus 会自动找到它；Steam 快捷方式被改动或重建时，会提醒你一键修复。见[游戏位置变了](../games/moved.md)。

## Switch 存档跨电脑

Ryujinx 的存档文件夹编号是每台电脑按创建顺序分配的，同一个游戏在不同电脑上编号不同。Ludonimbus 在每台电脑上按当前用户档案找到这个游戏的存档；在还没有这个游戏存档的电脑上，会按 Ryujinx 的方式新建一个（这时需要关闭 Ryujinx）。

## 常见问题

**即时存档（state）会同步吗？**
不会。只同步游戏内存档和记忆卡。即时存档和模拟器版本有关，而且往往很大。

**RetroArch 能用吗？**
能，按游戏同步。前提是存档在 RetroArch 文件夹里的 `saves` 下，并且按游戏文件命名（RetroArch 的默认设置）。

**我的模拟器不在列表里。**
用[找存档向导](../games/save-finder.md)选它的存档文件夹，或者给 [`adapters/emulators.yaml`](https://github.com/Shupian0510/Ludonimbus/blob/main/adapters/emulators.yaml) 贡献一条规则。

**支持 EmuDeck 或 Steam Deck 吗？**
Ludonimbus 目前只支持 Windows 10 和 11。在 Windows 上，不管模拟器怎么装的（包括便携版）都能用。
