# Ludonimbus 是什么

Steam 云存档只对 Steam 商店里的游戏有效。你加进 Steam 的模拟器、重制版和其他游戏，存档只留在本机：重装游戏、换文件夹、更新模拟器或者换电脑时，很容易就丢了。

Ludonimbus 让这些游戏也拥有和 Steam 云存档一样的体验：**从 Steam 启动前自动拉取最新存档，退出后自动上传，每个版本都留有记录。** 存档放在**你自己的** Google Drive、NAS、WebDAV / S3 存储或任意文件夹里，没有任何第三方服务器。

![游戏库](/screenshots/zh-CN/library-grid.png)

## 适合哪些游戏

- **模拟器游戏**：Dolphin、Cemu、Ryujinx、RPCS3、PCSX2 等 19 个模拟器，大部分可以按游戏单独同步。见[模拟器](../supported/emulators.md)。
- **重编译 / 移植游戏**：Ship of Harkinian、Zelda 64: Recompiled、Unleashed Recompiled 等 140 多个，按游戏程序自动认出。见[重编译 / 移植游戏](../supported/recomps.md)。
- **第三方 PC 游戏**：Epic、GOG、Xbox 应用（PC Game Pass）装的游戏，以及其他 PC 游戏。存档位置来自 Ludusavi 数据库，覆盖上万款游戏。见 [PC 游戏](../supported/pc-games.md)。

Steam 商店里买的游戏已经有 Steam 云存档，不需要 Ludonimbus。

## 工作原理

```text
Steam「开始游戏」
  ↓
ludonimbus-run：从你的云盘拉取较新的存档
  ↓
启动游戏，等它和启动器完全退出
  ↓
把新存档作为一个新版本上传
```

给一个游戏开启 [Steam 自动同步](../steam/auto-sync.md)后，它的 Steam 快捷方式会先启动 Ludonimbus 的小程序 `ludonimbus-run`，由它在游戏前后完成同步。快捷方式的名称、图标、封面和游戏时间都不变，你照常在 Steam 里点「开始游戏」就行。

每次上传都是一个新**版本**：一个只追加、不会被修改的 zip 快照，记录了设备、时间和上一个版本。所以任何一次同步都可以撤回，见[版本历史](../sync/versions.md)。

关闭窗口后 Ludonimbus 会留在系统托盘。不通过 Steam、直接打开游戏时，它也会发现游戏退出并自动备份；只是没法在启动前拉取，想要启动前也拉取最新存档，请从 Steam 启动。

## 存档放在哪里

存档放在你在「设置 → 云盘」里选的地方，里面是一个 `Ludonimbus` 文件夹：

```text
Ludonimbus/devices/<设备>.json            每台电脑一个小文件
Ludonimbus/games/<游戏>/game.json         游戏信息
Ludonimbus/games/<游戏>/snapshots/*.zip   每个版本一个 zip
```

zip 就是普通的压缩包，不用 Ludonimbus 也能打开。开发者没有服务器，收集不到任何数据，详见[隐私政策](../privacy.md)。

## 它不会做什么

- 不同步游戏本体、模拟器的密钥、固件、着色器缓存和即时存档（state），只同步存档。
- 不管 Steam 云已经在同步的游戏。
- 不修改你的存档内容：上传是打包复制，恢复是把某个版本的文件放回原处（当前存档如果云端还没有，恢复前会先备份成一个版本）。

## 下一步

- [下载与安装](./install.md)
- [快速上手](./quick-start.md)：连接云盘 → 添加游戏 → 开启 Steam 自动同步 → 照常玩
