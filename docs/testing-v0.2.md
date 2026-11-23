# 0.2 测试清单：Steam 里的云存档状态

0.2 新增：托盘程序的本机接口、Steam 插件（Millennium）、设置页里的一键安装。实现说明见 [steam-plugin.md](steam-plugin.md)。

日志在 `%APPDATA%\Ludonimbus\logs\`：`agent.log`（本机接口）、`steam-plugin.log`（安装 / 更新插件）。Millennium 自己的日志在 `Steam\millennium\logs\`。

## 已经自动验证过的

- `cargo test -p ludonimbus-core millennium`：检测安装状态、改 `enabledPlugins` 时保留其他设置、配置文件损坏时不覆盖、插件安装 / 更新 / 移除、解析 GitHub 发布信息和校验文件、解压时拒绝内容不对或路径越界的压缩包。
- 本机接口（临时数据目录 + 临时文件夹当云端）：无 token / 错 token 401；带 `Origin` 或外部 `Host` 403；查询状态、`refresh`、立即同步（同步中 `phase` 为 `syncing`，重复请求 409，结束后变为「已同步」）、打开游戏、未知游戏 404。
- 插件前端（用真实打包结果在模拟的 Steam「开始游戏」栏里跑）：各种状态的文字和颜色、点击打开 Ludonimbus、立即同步后快速刷新、切换游戏、被重绘删掉后补回、普通 Steam 游戏不显示。

## 需要在真实 Steam 里确认的

### 1. 一键安装

1. 安装 0.2，打开「设置 → 在 Steam 中显示云存档状态」，应显示「Millennium：未安装」。
2. 点「一键安装」→ 确认。应依次显示「正在下载 Millennium… xx%」「正在安装（Steam 会暂时关闭）…」，然后 Steam 重新打开，提示「已安装 Millennium vX.Y.Z 和 Ludonimbus 插件」。
3. Steam 打开后：
   - Millennium 第一次启动可能有自己的欢迎提示，按它的说明操作即可。
   - Steam 左上角 Millennium 的插件列表里能看到「Ludonimbus 云存档」，并且是启用状态。
   - 设置页刷新后显示「已安装 / 已启用」。

### 2. 状态栏

打开时之笛的游戏页（Steam 库里点它），「开始游戏」右边应该多一项「LUDONIMBUS 云存档」：

- 平时：「已是最新 · x 分钟前」，鼠标悬停有上次同步时间、设备、云端最新版本。
- 点右边的同步图标：变成「正在同步…」，几秒后恢复。
- 点这一项本身：Ludonimbus 窗口弹出并打开这个游戏。
- 从 Steam 启动游戏：先显示「正在检查云存档…」，进入游戏后「游戏运行中，退出后自动上传」，退出时「正在上传存档…」，最后「已是最新 · 刚刚」。
- 退出 Ludonimbus（托盘 → 退出）：几秒内变成「Ludonimbus 未运行」；重新打开 Ludonimbus 后恢复，不需要重启 Steam。
- 打开一个普通 Steam 游戏：不应出现这一项。
- Bloodborne / 火焰纹章（快捷方式还没开 Steam 自动同步）：悬停提示里应说明「没有在 Steam 中开启自动同步」。

**需要特别留意**：状态项的位置和字体是否和「最后运行日期」「游戏时间」一致；窗口变窄时是否挤乱布局；Steam 更新后是否还能显示（Steam 改版后 Millennium 的类名查找可能失效，这时状态项会退到「开始游戏」栏下方单独一行，或者不显示）。

### 3. 移除与更新

- 「移除插件」→ Steam 重启后游戏页不再显示状态，Millennium 仍在。再点「启用插件」能恢复（这次不会下载 Millennium）。
- 以后的版本更新时：装完新版 Ludonimbus，下次打开 Steam 自动用上新插件（`steam-plugin.log` 里有 `updated the installed Steam plugin`）。
