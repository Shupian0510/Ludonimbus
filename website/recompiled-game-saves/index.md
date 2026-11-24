---
title: 重编译游戏与移植游戏云存档
description: 备份和同步重编译游戏、玩家移植游戏的存档：Zelda 64 Recompiled、Ship of Harkinian、Unleashed Recompiled、sm64ex、OpenGOAL 等 140 多个游戏，版本历史保存在你自己的云盘里。
---

# 重编译游戏云存档

静态重编译（N64Recomp、XenonRecomp）、基于反编译的移植和原生重制版，让经典游戏变成可以直接运行的 PC 程序。它们不是从 Steam 商店来的，所以没有 Steam 云存档，而且很多都把存档**放在程序旁边**，恰好就是出新版本时你要替换掉的那个文件夹。

Ludonimbus 按游戏程序自动认出 140 多个重编译 / 移植游戏，知道它们的存档在哪里，同步到你自己的云盘，并保留完整的版本历史。

[下载 Windows 版](https://github.com/Shupian0510/Ludonimbus/releases/latest) · [重编译 / 移植游戏完整列表](../supported/recomps.md)

## 热门项目

这些游戏的存档位置按源码或 PCGamingWiki 核对过。表里的「游戏目录」是游戏程序所在的文件夹。

| 游戏 | 项目 | 存档位置 |
|---|---|---|
| 塞尔达传说：时之笛 | Ship of Harkinian | `游戏目录\Save` |
| 塞尔达传说：时之笛 | Longshot（原生重制） | `%APPDATA%\Longshot\save` |
| 塞尔达传说：姆吉拉的假面 | 2 Ship 2 Harkinian | `游戏目录\saves` |
| 超级马力欧 64 | Ghostship、sm64ex、sm64coopdx | `游戏目录\saves`、`%APPDATA%\sm64ex` |
| 星际火狐 64 | Starship | `游戏目录\default.sav` |
| 马力欧赛车 64 | SpaghettiKart | `游戏目录\default.sav` |
| 完美黑暗 | Perfect Dark 移植版 | `%APPDATA%\perfectdark` |
| 索尼克：释放 | Unleashed Recompiled | `%APPDATA%\UnleashedRecomp` |
| 杰克与达斯特 1–3 | OpenGOAL | `%APPDATA%\OpenGOAL\jak1…3\saves` |
| 火焰纹章：Fortune's Weave | fefw recomp | `游戏目录\save` |
| 血源诅咒 | Lantern Engine（shadPS4） | `游戏目录\user\savedata\1\CUSA00207` |

其余的来自 [RecompLauncher](https://recomplauncher.com/) 的游戏目录：开发者声明了存档位置的项目，以及 **Zelda 64: Recompiled**、**Banjo: Recompiled**、**Donkey Kong 64: Recompiled** 等 N64Recomp 游戏。

## 为什么重编译游戏容易丢存档

- **存档在游戏目录里。** 解压新版本后删掉旧文件夹，存档也跟着删了。
- **每个版本一个新文件夹。** 新旧版本并排放的话，每个新版本都从空存档开始。
- **没有安装程序。** 没有什么记得游戏放在哪里，挪位置或换电脑都要手动复制存档。

## Ludonimbus 怎样处理

- **按游戏目录的相对位置记录存档**，而不是写死路径。重装、挪位置或换电脑后，第一次启动就会恢复。
- **新版本解压到新文件夹会被自动找到。** Steam 快捷方式里只记游戏的编号，Steam 里什么都不用改。见[游戏位置变了](../games/moved.md)。
- **只同步存档。** 规则排除了程序、光盘镜像、日志和缓存。
- **启动器不退出也没关系。** Longshot 在游戏一关闭就上传存档，不等启动器关闭。任何游戏都可以在[上传时机](../steam/auto-sync.md#launcher)里打开这个选项。
- **一个程序对应多个游戏。** OpenGOAL 的三部 Jak 共用 `gk.exe`，按启动参数区分。

## 怎么添加

- 用 **RecompLauncher** 安装的游戏，「扫描本机」时会直接列出来；RecompLauncher 建的 Steam 快捷方式也能认出。
- 其他方式装的，用「扫描本机」扫描 Steam 快捷方式，或者对存放游戏的文件夹「扫描文件夹…」，也可以在「手动添加」里选游戏程序。
- 还不在 Steam 里的游戏，可以[一键加入 Steam](../steam/add-to-steam.md)，封面从 SteamGridDB 配好，第一次启动就开始同步。

![添加游戏：扫描存放重编译 / 移植游戏的文件夹](/screenshots/zh-CN/add-game-scan-folder.png)

## 常见问题

**我的重编译游戏没被认出来。**
用[找存档向导](../games/save-finder.md)：从常见位置里选，或者先玩一次存个档，让 Ludonimbus 列出游戏刚写入的文件夹。也欢迎给 [`adapters/builtin.yaml`](https://github.com/Shupian0510/Ludonimbus/blob/main/adapters/builtin.yaml) 贡献规则。

**需要原版游戏文件吗？**
Ludonimbus 完全不碰游戏文件或 ROM，只同步重编译 / 移植游戏自己生成的存档。

**便携模式呢？**
有便携模式的项目（Unleashed Recompiled 的 `portable.txt`、Perfect Dark），规则会先在游戏目录里找。
