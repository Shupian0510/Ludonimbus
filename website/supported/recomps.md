# 重编译 / 移植游戏

Ludonimbus 按游戏程序自动认出 140 多个重编译 / 移植游戏，知道它们的存档在哪里，添加时不用自己找。

- 用 [RecompLauncher](https://recomplauncher.com/) 安装的游戏，「扫描本机」时会直接列出来；RecompLauncher 建的 Steam 快捷方式也能认出。
- 其他方式装的，用「扫描本机」扫描 Steam 快捷方式或者「扫描文件夹…」，或者在「手动添加」里选游戏程序。

## 核对过存档位置的热门项目

这些游戏的存档位置按源码或 PCGamingWiki 核对过。表里的「游戏目录」是游戏程序所在的文件夹。

| 游戏 | 项目 | 存档位置 |
|---|---|---|
| 塞尔达传说：时之笛 | Longshot（原生重制） | `%APPDATA%\Longshot\save` |
| 塞尔达传说：时之笛 | Ship of Harkinian | `游戏目录\Save` |
| 塞尔达传说：姆吉拉的假面 | 2 Ship 2 Harkinian | `游戏目录\saves` |
| 超级马力欧64 | Ghostship | `游戏目录\saves` |
| 超级马力欧64 | sm64ex | `%APPDATA%\sm64ex\sm64_save_file.bin` |
| 超级马力欧64（联机版） | sm64coopdx | `%APPDATA%\sm64coopdx\sm64_save_file.bin` |
| 星际火狐64 | Starship | `游戏目录\default.sav` |
| 马力欧卡丁车64 | SpaghettiKart | `游戏目录\default.sav` |
| 完美黑暗 | Perfect Dark | `%APPDATA%\perfectdark`；便携模式时是 `游戏目录\eeprom.bin` |
| 索尼克：释放 | Unleashed Recompiled | `%APPDATA%\UnleashedRecomp\save`、`mlsave`；程序旁有 `portable.txt` 时在游戏目录 |
| Jak and Daxter、Jak II、Jak 3 | OpenGOAL | `%APPDATA%\OpenGOAL\jak1`（`jak2`、`jak3`）`\saves` |
| 超级马力欧兄弟 | Super Mario Bros. Remastered | `%APPDATA%\SMB1R`（不含日志和缓存） |
| 火焰纹章：万缕千丝 | fefw recomp | `游戏目录\save` |
| Bloodborne | Lantern Engine（shadPS4） | `游戏目录\user\savedata\1\CUSA00207` |

几个特别处理：

- **Longshot（时之笛）**：默认「游戏进程退出就上传」，游戏一关就上传，不等一直开着的启动器。
- **fefw recomp、Lantern Engine**：解压新版本到新文件夹时会自动找到它，见[游戏位置变了](../games/moved.md)。
- **OpenGOAL**：三部 Jak 共用 `gk.exe`，按启动参数（`--game jak2` 等）区分。

## 来自 RecompLauncher 目录的游戏

其余的规则来自 RecompLauncher 的游戏目录：

- 开发者在目录里声明了存档位置的项目；
- Zelda 64: Recompiled、Banjo: Recompiled、Donkey Kong 64: Recompiled 等 N64Recomp 游戏（存档在程序文件夹的 `saves`）。

这些规则会排除程序、光盘镜像、日志和缓存，只同步存档。

## 没认出我的游戏

用[找存档向导](../games/save-finder.md)，或者欢迎为 [`adapters/builtin.yaml`](https://github.com/Shupian0510/Ludonimbus/blob/main/adapters/builtin.yaml) 补充规则，见[参与贡献](../reference/contributing.md)。
