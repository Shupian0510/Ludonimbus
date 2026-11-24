---
title: 支持的游戏和模拟器
description: Ludonimbus 能同步哪些游戏：19 个模拟器、140 多个重编译 / 移植游戏，以及 Ludusavi 数据库里的上万款 PC 游戏；其他游戏可以用找存档向导添加。
---

# 支持的游戏和模拟器

Ludonimbus 同步的是 Steam 云存档管不到的游戏。它知道这些游戏的存档在哪里：

| | 数量 | 怎么认出来 |
|---|---|---|
| [模拟器](#emulators) | 19 个 | 内置规则，大部分按游戏同步 |
| [重编译 / 移植游戏](#recomps) | 140 多个 | 按游戏程序 |
| [PC 游戏](#pc-games) | 上万款 | Ludusavi 数据库（来自 PCGamingWiki） |
| [其他游戏](#other) | — | 找存档向导：常见位置，或者「玩一下就找到」 |

运行环境：**Windows 10 / 11（64 位）**，已安装 Steam。

## 模拟器 {#emulators}

| 模拟器 | 平台 | 同步方式 |
|---|---|---|
| Dolphin | GameCube / Wii | 按游戏（GameCube 需要 GCI 文件夹记忆卡，Dolphin 默认就是） |
| Cemu | Wii U | 按游戏；读取你在 Cemu 里设置的 MLC 文件夹 |
| Azahar、Citra、Lime3DS | 3DS | 按游戏 |
| Ryujinx | Switch | 按游戏；在每台电脑上找到当前用户档案的存档 |
| Eden、Citron、Sudachi、Suyu、yuzu | Switch | 整个模拟器（存档和本机用户档案绑定） |
| RPCS3 | PS3 | 按游戏 |
| shadPS4 | PS4 | 按游戏 |
| PPSSPP | PSP | 按游戏 |
| Vita3K | PS Vita | 按游戏 |
| PCSX2 | PS2 | 整个模拟器（记忆卡所有游戏共用） |
| DuckStation | PS1 | 整个模拟器 |
| Xenia | Xbox 360 | 整个模拟器 |
| RetroArch | 多平台 | 按游戏（按游戏文件名） |

便携版和安装版都能找到。只同步存档，不碰密钥、固件、着色器缓存和即时存档。更多：[模拟器存档同步](../emulator-cloud-saves/index.md) · [各模拟器的存档位置](../supported/emulators.md)。

## 重编译 / 移植游戏 {#recomps}

热门项目包括：

- **塞尔达**：Ship of Harkinian、2 Ship 2 Harkinian、Longshot（时之笛）、Zelda 64: Recompiled
- **马力欧**：Ghostship、sm64ex、sm64coopdx、SpaghettiKart、Super Mario Bros. Remastered
- **其他**：Starship（星际火狐 64）、Perfect Dark、Unleashed Recompiled（索尼克：释放）、OpenGOAL（Jak 1–3）、Banjo: Recompiled、Donkey Kong 64: Recompiled、fefw recomp（火焰纹章：Fortune's Weave）、Lantern Engine（血源诅咒）

其余的来自 [RecompLauncher](https://recomplauncher.com/) 的游戏目录。用 RecompLauncher 安装的游戏，「扫描本机」时会直接列出来。更多：[重编译游戏云存档](../recompiled-game-saves/index.md) · [存档位置](../supported/recomps.md)。

## PC 游戏 {#pc-games}

普通 PC 游戏的存档位置来自 [Ludusavi](https://github.com/mtkennerly/ludusavi) 的数据库，覆盖上万款游戏；Ludonimbus 的安装包自带 Ludusavi 官方程序。

| 游戏来源 | 能做什么 |
|---|---|
| Epic、GOG | 「扫描本机」会列出它们装的游戏 |
| Xbox 应用（PC Game Pass） | 可以加入 Steam，通过 `gamelaunchhelper.exe` 启动，见[一键加入 Steam](../steam/add-to-steam.md#xbox) |
| EA、育碧等其他商店 | 按存档找到；Ludonimbus 会在这些商店的安装文件夹里找游戏程序 |
| 免 DRM 和其他游戏 | 按存档找到，或者在 Ludusavi 数据库里按名字搜索 |
| Steam 商店游戏 | 不需要：Steam 云存档已经在同步，会单独列出 |

更多：[PC 游戏](../supported/pc-games.md)。

## 没被认出的游戏 {#other}

其他任何游戏都可以用[找存档向导](../games/save-finder.md)：

1. 按游戏的英文名在 Ludusavi 数据库里搜索；
2. 或者从「文档」「保存的游戏」、AppData 和游戏目录里按游戏名找到的文件夹中选；
3. 或者**玩一下就找到**：启动游戏、存一次档，Ludonimbus 列出刚刚被写入的文件夹；
4. 或者自己选存档文件夹。

## 暂不支持

- 存档放在 **Windows 注册表**里的游戏（扫描时会单独列出）。
- **macOS、Linux 和 Steam Deck**：Ludonimbus 目前只支持 Windows。
- 同步游戏本体、截图或游戏时间（游戏时间同步在[路线图](https://github.com/Shupian0510/Ludonimbus/blob/main/README.zh-CN.md#路线图)上）。

## 添加游戏或模拟器规则

存档规则在 [`adapters/builtin.yaml`](https://github.com/Shupian0510/Ludonimbus/blob/main/adapters/builtin.yaml)（游戏）和 [`adapters/emulators.yaml`](https://github.com/Shupian0510/Ludonimbus/blob/main/adapters/emulators.yaml)（模拟器）里，欢迎贡献新规则，见[参与贡献](../reference/contributing.md)。
