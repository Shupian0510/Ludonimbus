# 一键加入 Steam

不在 Steam 里的游戏（Epic、GOG、Xbox 应用的游戏，解压即玩的游戏，模拟器游戏……）可以一键加进 Steam。加进去的快捷方式从第一次启动起就会自动同步存档，还能用 Steam 的游戏时长、截图和手柄设置。

## 加入

可以单个加，也可以一次加好几个：

- **单个**：在游戏详情页点「加入 Steam…」。需要先设置好游戏程序。
- **批量**：在「扫描本机」添加游戏后，上方会提示「刚添加的 N 个游戏还不在 Steam 里」，点「加入 Steam…」。

在弹出的窗口里：

1. 选择 **Steam 账号**（有多个账号时）。
2. 打开或关闭**自动配封面**：从 SteamGridDB 选评分最高的封面、背景、Logo 和图标，需要先在设置里填写 SteamGridDB API Key，见[封面与 SteamGridDB](./artwork.md)。
3. 点「加入 Steam」。Steam 正在运行时会暂时关闭，完成后自动重新打开。

<!-- screenshot: add-to-steam -->

有个别游戏没能加入时，会提示「有 N 个没能加入，其他的已经加好了」，并在游戏名后显示原因，可以点「重试」。

## Xbox 应用（PC Game Pass）的游戏 {#xbox}

Xbox 游戏自己的程序受系统保护，其他程序不能直接启动。Ludonimbus 会用游戏文件夹里的 `gamelaunchhelper.exe` 启动它，并等游戏本身退出后再上传存档。

如果详情页提示「这是 Xbox 应用安装的游戏，它自己的程序受保护」，点「改用 gamelaunchhelper.exe」。

Xbox 游戏一般自带 Xbox 云存档；Ludusavi 数据库里有存档位置的，Ludonimbus 也会再同步一份并保留版本历史。

## 移除

Ludonimbus 建的快捷方式，可以在游戏详情页点「从 Steam 移除快捷方式」。会删除这个快捷方式和它的封面；游戏、存档和 Ludonimbus 里的记录都不受影响，以后可以再加入。

从 Ludonimbus 移除游戏时，它建的 Steam 快捷方式也会一起删除。
