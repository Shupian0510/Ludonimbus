# 常见问题与故障排查

## 常见问题

### 不从 Steam 启动游戏也能同步吗？

可以上传，但不会在启动前拉取。Ludonimbus 在托盘里时，会发现游戏退出并自动备份。想要启动前也拉取最新存档，请从 Steam 启动。

### 两台电脑都玩过，存档冲突了怎么办？

启动游戏前会弹窗让你选择使用云端还是本机的那份。两份都会留在游戏的「存档」页签里，选错了随时可以设回去。见[存档冲突](../sync/conflicts.md)。

### 能用百度网盘、阿里云盘或夸克吗？

夸克网盘可以扫码一键连接（实验性），见[夸克网盘](../clouds/quark.md)。坚果云自带 WebDAV，可以直接连接。百度网盘和阿里云盘没有一键登录，可以通过 [OpenList](https://github.com/OpenListTeam/OpenList) / AList 接入：先在 OpenList 里挂载你的网盘，再在 Ludonimbus 里用 WebDAV 连接 OpenList，文件夹写在挂载的网盘下面。见 [WebDAV → OpenList / AList](../clouds/webdav.md#openlist)。

### 界面能换成英文吗？

可以。「设置 → 语言 / Language」里选 English，立刻生效。默认跟随 Windows 的显示语言。见[设置项说明](./settings.md#language)。

### 夸克网盘提示「登录已失效」怎么办？

在「设置 → 云盘」点「重新扫码登录」，用夸克 App 再扫一次。不用重新下载组件，存档不受影响。见[夸克网盘](../clouds/quark.md#登录失效)。

### Xbox 应用（PC Game Pass）的游戏能用吗？

能加进 Steam。Xbox 游戏自己的程序受系统保护，其他程序不能直接启动，Ludonimbus 会用游戏文件夹里的 `gamelaunchhelper.exe` 启动它，并等游戏本身退出后再上传存档。手动选择时也请选 `Content` 文件夹里的 `gamelaunchhelper.exe`。见[一键加入 Steam](../steam/add-to-steam.md#xbox)。

### 扫描时为什么有些游戏显示「Steam 云已经在同步」？

这些游戏现在装在 Steam 里，Steam 云存档已经在同步它们，不需要 Ludonimbus 再管。如果你在 Steam 里卸载了它、改在别的平台玩，它会出现在「可以直接添加」里。

### 换云盘之后，以前的存档怎么办？

连接新云盘后，设置页会询问是否把原来云盘里的存档复制过去。复制会保留全部历史版本，原来的云盘不会被修改。见[换云盘与复制存档](../clouds/switching.md)。

### 删了游戏、重装系统，存档还在吗？

在。存档在你的云盘里，重新安装 Ludonimbus、连接同一个云盘、添加同一个游戏后，第一次启动会自动恢复。

### Ludonimbus 会改我的 Steam 吗？

只改你开启了自动同步、或者让它加进 Steam 的那些非 Steam 快捷方式。关闭自动同步会恢复原来的快捷方式。Steam 插件是可选的，需要你自己点「一键安装」。

### 卸载 Ludonimbus 之前需要做什么？

在每个游戏的详情页关闭「Steam 自动同步」，Steam 快捷方式会恢复成原来的样子。云端的存档和版本都会保留。见[下载与安装 → 卸载](../guide/install.md#uninstall)。

### 支持 macOS、Linux 或 Steam Deck 吗？

Windows 10 / 11 是主要平台。从 1.0 开始有 Linux 和 Steam Deck 版（预览），见 [Linux 和 Steam Deck](../platforms/linux.md)。macOS 暂时不支持。

## 故障排查

### 日志在哪里

日志在 `%APPDATA%\Ludonimbus\logs\`（「设置 → 关于 → 数据目录」点一下打开，再进 `logs`）：

| 文件 | 记录什么 |
|---|---|
| `runner.log` | 从 Steam 启动游戏时的同步过程 |
| `sync.log` | 每次同步的结果 |
| `watcher.log` | 托盘里的后台监视（不经过 Steam 打开的游戏） |
| `agent.log` | 给 Steam 插件用的本机接口 |
| `steam-plugin.log` | 安装、更新 Steam 插件 |

反馈问题时附上相关的日志会很有帮助（发出前可以看一眼，去掉你不想公开的路径或名字）。

### 常见问题和处理

| 现象 | 处理 |
|---|---|
| 左侧栏显示「云盘需要重新连接」 | 登录过期或密码改了，到「设置 → 云盘」重新连接 |
| 「测试连接」报证书错误 | 局域网 NAS 没有受信任的 HTTPS 证书，改用 `http://` 地址 |
| 坚果云 / Nextcloud 提示用户名或密码错误 | 要用应用密码，不是登录密码，见 [WebDAV](../clouds/webdav.md) |
| S3 提示 Bucket 不存在或签名错误 | 检查 Endpoint、区域和 Bucket 名称，见 [S3 兼容存储](../clouds/s3.md#troubleshooting) |
| 从 Steam 启动没有同步 | 详情页的「Steam 自动同步」是否显示「从 Steam 启动时会自动同步」；快捷方式被改过时点「一键修复」 |
| 「找不到 ludonimbus-run.exe」 | 重新安装 Ludonimbus |
| Steam 里不显示云存档状态 | 确认 Ludonimbus 在托盘里运行；只在桌面模式、只对 Ludonimbus 管理的游戏显示；插件刚更新时要重新打开 Steam |
| Steam 状态栏显示「Ludonimbus 未运行」 | 打开 Ludonimbus（关闭窗口后它会留在托盘） |
| 提示旧版 Millennium（2.x） | 先用 Millennium 官方安装程序更新到最新版 |
| 「玩一下再找」没有发现变化 | 确认在游戏里真的存了档并退出了游戏；或者自己选择文件夹 |
| 存档很大、可能含有缓存 | 点「查看」看里面是什么，在游戏「设置 → 排除」里排除缓存文件夹 |
| Ryujinx 换电脑后提示要关闭模拟器 | 新建这台电脑上的存档时 Ryujinx 不能开着，关掉后再启动 |
| 杀毒软件拦截 | 安装包没有代码签名，个别杀毒软件可能误报，可以把 Ludonimbus 的安装目录加入信任 |

### 还是不行 {#still-stuck}

到 [GitHub Issues](https://github.com/Shupian0510/Ludonimbus/issues) 反馈，写上 Ludonimbus 版本（「设置 → 关于」）、用的云盘、出问题的游戏和步骤，附上相关日志。
