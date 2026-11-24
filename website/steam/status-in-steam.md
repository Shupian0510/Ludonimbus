# 在 Steam 里显示云存档状态

装好之后，Steam 游戏库里每个由 Ludonimbus 管理的游戏，「开始游戏」那一栏会多出一项「Ludonimbus 云存档」，和「最后运行日期」「游戏时间」并排显示。

![Steam 游戏页里的 Ludonimbus 云存档状态](/screenshots/zh-CN/steam-play-bar-status.png)

- 从 Steam 启动、游戏中、退出上传时都会实时更新，例如「已是最新 · 5 分钟前」「游戏运行中，退出后自动上传」「正在上传存档…」「存档冲突，点击选择保留哪一份」。
- 点右边的按钮立即同步。点状态本身会列出云存档的[版本历史](../sync/versions.md#restore-from-steam)，可以直接恢复；存档冲突或快捷方式失效时打开 Ludonimbus 处理。
- 鼠标悬停能看到更多信息：上次同步的时间和设备、云端最新的版本、等待上传的版本数。
- 只对 Ludonimbus 管理的游戏显示，普通 Steam 游戏不受影响。

插件还会：

- 把 Steam 自己的「游戏时间」换成所有电脑的总时间，见[游戏时间](../features/playtime.md)；
- 显示「成就 x/y」和右栏的「成就」区块，解锁时弹出 Steam 的成就提醒，见[成就](../features/achievements.md)；
- 游戏文件不在本机、云端有备份时显示「本机没有游戏文件 · 点击从云端安装」，见[游戏本体备份](../features/game-backup.md)。

## 安装

1. 打开「设置 → 在 Steam 中显示云存档状态」，点「一键安装」（已经装了 Millennium 时是「启用插件」）。
2. 确认窗口里会说明要做什么，点「下载并安装」。
3. Ludonimbus 会：
   - 从 [Millennium](https://github.com/SteamClientHomebrew/Millennium) 的 GitHub 官方发布页下载最新正式版，**先校验 SHA-256** 再安装到 Steam 文件夹；
   - 把 Ludonimbus 插件放进 Millennium 并启用；
   - 安装时暂时关闭 Steam，完成后自动重新打开。

![设置 → 在 Steam 中显示云存档状态](/screenshots/zh-CN/settings-steam-plugin.png)

这和 Millennium 官方安装程序做的事情一样。之后 Millennium 自己负责更新；Ludonimbus 更新时，插件也会自动更新，下次打开 Steam 时生效。

::: info Millennium 是什么
Millennium 是开源的 Steam 客户端插件框架，**不是 Valve 官方组件**。Ludonimbus 不附带它，只在你点「一键安装」时从官方发布页下载。
:::

## 需要注意

- **需要 Ludonimbus 在托盘中运行**。没运行时状态栏显示「Ludonimbus 未运行」，不影响 Steam 和游戏本身，自动同步也照常工作。
- 只在 Steam 的桌面模式里显示，大屏幕模式里不显示。
- 检测到旧版 Millennium（2.x）时不支持这个插件，请先用 Millennium 官方安装程序更新到最新版。

## 移除

在同一个地方点「移除插件」。Steam 里将不再显示云存档状态，自动同步不受影响。Millennium 本身会保留，卸载它请使用 Millennium 的官方安装程序。

## 安全吗

插件只负责显示和转发点击：它通过本机接口向托盘里的 Ludonimbus 询问状态，接口只监听本机，并且需要每次启动随机生成的令牌，网页里的脚本无法调用。插件不接触你的网盘账号。
