# 开发说明 / Development

## 从源码构建

需要：Rust（stable，MSVC）、Node.js 20+、Visual Studio C++ 生成工具、WebView2（Windows 10/11 自带）。

```powershell
# 1. 准备随应用分发的文件：启动包装器、Ludusavi 官方发布版、Steam 插件
./scripts/prepare-sidecars.ps1

# 2. 开发模式运行
cd app
npm install
npm run tauri dev

# 3. 打包安装程序（NSIS）。Google OAuth 客户端从 secrets.local.env 或环境变量读取
cd ..
./scripts/build-release.ps1
```

只调界面时可以在 `app/` 里 `npm run dev`，直接在浏览器里打开，会使用模拟数据（`app/src/mock.ts`）。

只改 Steam 插件时运行 `./scripts/build-plugin.ps1`。插件源码在 `plugin-millennium/`，用 Millennium 官方的 starlight 工具打包成 `.star`，说明见 [steam-plugin.md](steam-plugin.md)。

## 测试与排查

```powershell
cargo test --workspace --exclude ludonimbus-app
```

命令行工具能完成大部分操作，方便排查问题：

```powershell
cargo run -p ludonimbus-cli -- status
cargo run -p ludonimbus-cli -- steam shortcuts
cargo run -p ludonimbus-cli -- snapshots fe-fortunes-weave
```

日志在 `%APPDATA%\Ludonimbus\logs\`：`runner.log`（从 Steam 启动时）、`sync.log`、`watcher.log`（托盘后台监视）、`agent.log`（本机接口）、`steam-plugin.log`（安装 / 更新插件）。

手动测试清单：[testing-v0.1.md](testing-v0.1.md)、[testing-v0.2.md](testing-v0.2.md)、[testing-v0.3.md](testing-v0.3.md)。

## 发布新版本

```powershell
./scripts/bump-version.ps1 -Version 0.2.1   # 改版本号，并在 CHANGELOG.md 加双语模板
# 写好 CHANGELOG.md 的中文和英文说明，提交
./scripts/release.ps1                       # 检查后打标签推送，GitHub Actions 编译、签名并发布
```

每次推送到 main 还会自动打一个开发版安装包（Actions 构建产物，保留 14 天）。完整说明见 [release-process.md](release-process.md)。

## Google Drive / 百度网盘

发布者需要在 Google Cloud 创建一次「桌面应用」类型的 OAuth 客户端，并在构建时注入 Client ID，见 [google-oauth-setup.md](google-oauth-setup.md)。普通用户只需要登录。百度网盘同理，发布者需要在百度网盘开放平台注册一次应用，见 [baidu-netdisk-setup.md](baidu-netdisk-setup.md)。

## 项目结构

```text
crates/core          同步引擎：存档扫描、快照、云盘、Steam 快捷方式、同步判定、Millennium 安装
crates/cli           ludonimbus 命令行
crates/runner        ludonimbus-run，Steam 快捷方式实际启动的程序
app/                 桌面应用（Tauri 2 + React + TypeScript），含给 Steam 插件用的本机接口
plugin-millennium/   Steam 插件（Millennium：Lua 后端 + TypeScript 前端）
adapters/            内置存档规则
docs/                设计文档、测试清单、主页
scripts/             构建与发布脚本
```

## 设计文档

- [plan-phase1.md](plan-phase1.md)：整体设计、阶段划分、路线图
- [plan-phase2.md](plan-phase2.md)：下一阶段（更多云盘、添加游戏、截图与游戏时间、一键加入 Steam）
- [steam-plugin.md](steam-plugin.md)：Steam 插件、本机接口和一键安装
- 同步判定规则：[`crates/core/src/sync.rs`](../crates/core/src/sync.rs) 顶部的表格

云端目录结构：

```text
Ludonimbus/devices/<设备>.json
Ludonimbus/games/<游戏>/game.json
Ludonimbus/games/<游戏>/snapshots/<版本>.zip
```

每个版本是一个只追加、不可修改的 zip 快照，带父版本指针和设备信息；最新的主线版本就是「云端当前版本」。
