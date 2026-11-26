# Linux 和 Steam Deck（预览）

从 1.0 开始有 Linux 版，可以用在 Steam Deck（SteamOS）、Bazzite 和一般的 Linux 桌面上。

它是**预览**：同步和 Windows 版是同一套，但在真机上验证得还不够多。遇到问题请到 [GitHub Issues](https://github.com/Shupian0510/Ludonimbus/issues) 反馈。

## 安装

1. 从 [Releases](https://github.com/Shupian0510/Ludonimbus/releases/latest) 下载 `Ludonimbus-linux-x86_64.tar.gz` 并解压。在 Deck 上先切到桌面模式。
2. 在解压出来的文件夹里运行：

```bash
./install-linux.sh
```

它装在你的主目录里，不需要 root；系统分区只读的 SteamOS 和 Bazzite 也能装。

| 装了什么 | 在哪里 |
|---|---|
| 应用（桌面模式用） | `~/.local/share/ludonimbus/Ludonimbus.AppImage`，应用菜单里有「Ludonimbus」 |
| 从 Steam 启动游戏时同步存档的小程序 | `~/.config/Ludonimbus/bin/ludonimbus-run` |
| 后台服务（游戏模式用） | `~/.config/Ludonimbus/bin/ludonimbus-agent`，登录后自动启动 |

- **更新**：下载新版本，再运行一次 `./install-linux.sh`。
- **卸载**：`./install-linux.sh --uninstall`。设置和同步记录留在 `~/.config/Ludonimbus`。

## 第一次设置

在桌面模式里打开 Ludonimbus，和 Windows 上一样：[连接云盘](../clouds/index.md)、[添加游戏](../games/scan.md)、开启 [Steam 自动同步](../steam/auto-sync.md)。连接和 Windows 电脑同一个云盘，就能在两边接着玩，见[多台设备](../sync/devices.md)。

用 Proton 运行的 Windows 游戏，存档在 Proton 给这个游戏建的前缀里，Ludonimbus 会到那里去找。游戏在这台设备上还没用 Proton 运行过时，先启动一次，再同步。

## 游戏模式

游戏模式里没有窗口和托盘，同步由后台服务负责：从 Steam 启动游戏前拉取，退出后上传。用 Steam 菜单里的「退出游戏」结束游戏时，没来得及上传的存档由后台服务补传。

启动时需要你选择的提示（例如[存档冲突](../sync/conflicts.md)）用全屏窗口显示，可以用手柄选，见[掌机和手柄界面](./handheld.md)。

### Decky 插件 {#decky}

装了 [Decky Loader](https://decky.xyz/) 的话，可以在快捷菜单里加一个 Ludonimbus 面板：当前游戏的云存档状态、「立即同步」、处理冲突、存档列表和「设为当前」。

1. 从 [Releases](https://github.com/Shupian0510/Ludonimbus/releases/latest) 下载 `Ludonimbus-decky.zip`。
2. 在 Decky 的设置里打开开发者模式，选「从 ZIP 文件安装插件」，选这个文件。

面板显示「Ludonimbus 没有运行」时：在桌面模式里打开一次 Ludonimbus，或者重新运行 `./install-linux.sh` 确认后台服务已经装好。

## 和 Windows 版的差别

- [在 Steam 里显示云存档状态](../steam/status-in-steam.md)的插件只用于 Windows 上的桌面版 Steam；游戏模式里用 Decky 插件。
- Xbox 应用（Game Pass）的版本只在 Windows 上有。
- 没有应用内的一键更新，更新方法见上面。
