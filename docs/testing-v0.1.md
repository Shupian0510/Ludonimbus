# 0.1 测试清单

第一版已完成的部分，以及建议的测试顺序。测试中遇到问题，日志在 `%APPDATA%\Ludonimbus\logs\`（`runner.log` 是从 Steam 启动时的记录，`sync.log` 是同步记录，`watcher.log` 是后台监视）。

## 准备

1. 运行 `target\release\bundle\nsis\Ludonimbus_0.1.0_x64-setup.exe` 安装（装在当前用户下，不需要管理员权限）。
2. 先手动备份一份三个游戏的存档文件夹，以防万一：
   - 时之笛：`%APPDATA%\Longshot\save`
   - 火焰纹章：`<游戏目录>\save`
   - Bloodborne：`<游戏目录>\user\savedata`

## 1. 连接云盘

- **Google Drive**：按 [google-oauth-setup.md](google-oauth-setup.md) 创建 OAuth 客户端，在「设置 → 云盘」填入 Client ID / Client secret，点「连接 Google Drive」。浏览器里登录后，设置页应显示你的邮箱和 Drive 用量。
- **或者先用文件夹测试**：「选择文件夹…」选一个空文件夹（或者 Google Drive 电脑版同步的文件夹）。

## 2. 添加三个游戏

「添加游戏 → Steam 快捷方式」，三个游戏都应显示「已识别」，逐个点「添加」。

- 游戏库的封面、详情页的横幅和 Logo 应该是你在 Steam 里设置的那些。
- 详情页 →「存档文件」里应能看到存档文件列表（时之笛约 14 个文件，火焰纹章 4 个，Bloodborne 16 个）。
- 点「立即同步」：状态变成「已同步」，「版本历史」里出现第一个版本。

## 3. 打开 Steam 自动同步

详情页打开「Steam 自动同步」开关 → 确认关闭 Steam → Ludonimbus 修改快捷方式后会重新打开 Steam。

- Steam 里三个游戏的图标、封面、游玩时间应该都不变。
- 从 Steam 启动游戏，玩一会儿存个档，退出。回到 Ludonimbus，版本历史里应多出一个「退出游戏后自动」的版本。
- 需要特别留意：**Steam 是否正常显示「正在运行」并记录游玩时间、Steam 游戏内覆盖层（Shift+Tab）是否可用。**
- 时之笛和 Bloodborne 都是启动器再拉起游戏，请确认退出游戏后才上传（`runner.log` 里会有 `job is empty` / `watched processes exited`）。

## 4. 验收：删游戏后找回存档

建议用火焰纹章测试：

1. 把 `fefw-recomp-0.2.6` 整个文件夹改名（模拟删除），再解压一份新的到 `fefw-recomp-0.2.7`（没有 save 文件夹）。
2. 从 Steam 启动火焰纹章 → 应弹出「游戏位置变了」，指向新文件夹 → 确认。
3. 进游戏后存档应该还在（启动前已从云端恢复）。

Bloodborne 可以用同样的方法（把 `BloodbornePC\user\savedata` 改名），时之笛把 `%APPDATA%\Longshot\save` 改名。

## 5. 其他场景

- **恢复旧版本**：版本历史里选一个旧版本 →「恢复」。当前存档会先存成「恢复前保护」版本。
- **快捷方式被改**：在 Steam 里把某个游戏的「目标」改回原来的 exe → Ludonimbus 游戏库里该游戏出现黄色警告 → 详情页「一键修复」。
- **不经过 Steam 启动**：Ludonimbus 开着（托盘里），直接双击游戏 exe，玩完退出 → 应出现「后台检测到退出」的版本。
- **离线**：断网后从 Steam 启动 → 等待约 8 秒后直接进游戏；退出后联网，下次同步时自动补传。
- **多台电脑**：另一台电脑装 Ludonimbus、连同一个云盘，「添加游戏 → 云端已有」关联游戏，同步后存档会拉下来；版本历史里能看到两台设备的名字。

## 已知限制（0.1）

- Google Drive 需要先在 Google Cloud 创建 OAuth 客户端（发布版内置后用户就不用填了）。
- 修改 Steam 快捷方式需要暂时关闭 Steam（Millennium 插件完成后可以免关闭）。
- 同步结果目前不弹 Windows 通知，只在 Ludonimbus 里显示；冲突和出错会弹对话框。
- 从 Ludusavi 数据库添加的 PC 游戏不支持存在注册表里的存档。
- 只支持 Windows。
