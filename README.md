<p align="center">
  <img src="app/public/app-icon.svg" width="96" alt="Ludonimbus">
</p>

<h1 align="center">Ludonimbus</h1>

<p align="center">给 Steam 里的「非 Steam 游戏」补上云存档：模拟器、重制版、第三方游戏。<br>
启动前自动拉取，退出后自动上传，多设备、多版本，删了游戏再装回来存档还在。</p>

> English: Ludonimbus brings Steam-Cloud-like save sync to non-Steam shortcuts (emulators, recompilations, third-party games). Saves are pulled before launch and uploaded after exit, with per-device version history and conflict handling. Google Drive or any synced folder as storage. Built on Ludusavi's save database.

> 状态：**0.1 第一版开发中**，欢迎试用和反馈。目前只支持 Windows。

## 能做什么

- **像 Steam 云存档一样工作**：Steam 快捷方式会先启动 Ludonimbus 的小程序，游戏启动前拉取云端较新的存档，退出后上传新存档。
- **删游戏不丢存档**：存档按「相对游戏目录」记录。重装游戏、换文件夹、换电脑后，第一次启动就会自动恢复。
- **模拟器更新不会断**：Steam 快捷方式里只写游戏 id，游戏路径由 Ludonimbus 管理。解压新版本到新文件夹时会自动找到并询问；快捷方式被改动或重建时会提醒并一键修复。
- **版本历史**：每个版本都记录时间、设备名、触发方式，可以恢复、导出 zip、锁定、写备注。自动清理旧版本，手动备份永久保留。
- **冲突时两份都留**：两台设备都改过存档时，让你选择用哪一份，没选的那份也保存在版本历史里。
- **离线也能玩**：连不上云端就直接启动，退出后的存档先存在本机，恢复联网后自动补传。
- **Steam 风格的界面**：游戏库网格 / 列表视图，直接使用你在 Steam 里设置的封面、横幅和 Logo。
- **云盘**：一键连接 Google Drive（存档在你自己的 Drive 里，网页版也能看到）；或者任意文件夹（Google Drive / OneDrive 电脑版同步的文件夹、NAS、移动硬盘）。
- **存档位置识别**：内置模拟器 / 重制版规则；普通 PC 游戏可以从 [Ludusavi](https://github.com/mtkennerly/ludusavi) 的数据库里搜索（上万款游戏）；也可以手动指定文件夹。

### 内置规则

| 游戏 | 运行方式 | 存档 |
|---|---|---|
| 塞尔达传说：时之笛 | Longshot | `%APPDATA%\Longshot\save` |
| 火焰纹章：万缕千丝 | fefw recomp | `<游戏目录>\save` |
| Bloodborne | Lantern Engine（shadPS4） | `<游戏目录>\user\savedata\1\CUSA00207` |

规则写在 [`adapters/builtin.yaml`](adapters/builtin.yaml) 里，欢迎提 PR 补充更多模拟器和游戏。

## 使用

1. 安装后打开 Ludonimbus，在「设置 → 云盘」里连接 Google Drive，或者选一个同步文件夹。
2. 「添加游戏 → Steam 快捷方式」，选择你在 Steam 里添加过的非 Steam 游戏。
3. 在游戏详情页打开「Steam 自动同步」（需要暂时关闭 Steam，Ludonimbus 会自动关闭并重新打开它）。
4. 以后照常从 Steam 启动游戏就行。

关闭窗口后 Ludonimbus 会留在托盘：如果你不通过 Steam 直接打开游戏，它也会在游戏退出后自动备份。

## 工作原理

```
Steam 快捷方式 ──> ludonimbus-run --game <id> ──> 启动前同步 ──> 启动游戏并等待整个进程树退出 ──> 退出后上传
                                   │
桌面应用（Tauri）──────────────────┤  core：存档扫描 / 快照 / 同步判定 / 云盘
                                   │
                         Google Drive 或同步文件夹：
                         Ludonimbus/devices/<设备>.json
                         Ludonimbus/games/<游戏>/snapshots/<版本>.zip
```

- 每个版本是一个只追加、不可修改的 zip 快照，带父版本指针和设备信息；最新的主线版本就是「云端当前版本」。
- 启动前的判定规则见 [`crates/core/src/sync.rs`](crates/core/src/sync.rs) 顶部的表格。
- 设计文档：[`docs/plan-phase1.md`](docs/plan-phase1.md)；测试清单：[`docs/testing-v0.1.md`](docs/testing-v0.1.md)。

## 从源码构建

需要：Rust（stable，MSVC）、Node.js 20+、Visual Studio C++ 生成工具、WebView2（Windows 10/11 自带）。

```powershell
# 1. 准备随应用分发的程序（启动包装器 + Ludusavi 官方发布版）
./scripts/prepare-sidecars.ps1

# 2. 开发模式运行
cd app
npm install
npm run tauri dev

# 3. 打包安装程序（NSIS），Google OAuth 客户端从 secrets.local.env 或环境变量读取
cd ..
./scripts/build-release.ps1
```

只调界面时可以 `npm run dev` 直接在浏览器里打开，会使用模拟数据（`app/src/mock.ts`）。

命令行工具也能完成大部分操作，方便排查问题：

```powershell
cargo run -p ludonimbus-cli -- status
cargo run -p ludonimbus-cli -- steam shortcuts
cargo run -p ludonimbus-cli -- snapshots fe-fortunes-weave
```

运行测试：`cargo test --workspace --exclude ludonimbus-app`

### Google Drive

发布者需要在 Google Cloud 创建一次「桌面应用」类型的 OAuth 客户端，并在构建时注入 Client ID，见 [`docs/google-oauth-setup.md`](docs/google-oauth-setup.md)。普通用户只需要登录。

## 项目结构

```
crates/core      同步引擎：存档扫描、快照、云盘、Steam 快捷方式、同步判定
crates/cli       ludonimbus 命令行
crates/runner    ludonimbus-run，Steam 快捷方式实际启动的程序
app/             桌面应用（Tauri 2 + React + TypeScript）
adapters/        内置存档规则
docs/            设计文档
```

## 路线图

- [x] 第一版：Google Drive / 同步文件夹、3 个游戏自动同步、版本历史、冲突处理、图形界面
- [ ] Millennium 插件：在 Steam 游戏页里显示云存档状态和版本，不关 Steam 就能开关自动同步
- [ ] 更多模拟器规则（Ryujinx / Eden、RPCS3、Dolphin、RetroArch、PCSX2…）
- [ ] 更多云盘（rclone、WebDAV / OpenList）
- [ ] 游戏本体备份到云盘，在 Steam 里「从云端安装」
- [ ] SteamGridDB 封面、成就

## 致谢

- [Ludusavi](https://github.com/mtkennerly/ludusavi)（MIT）：存档位置数据库与查找，发布包中附带其官方可执行文件。
- [Tauri](https://tauri.app/)

## 许可

[MIT](LICENSE) · [主页](https://shupian0510.github.io/Ludonimbus/) · [隐私政策](https://shupian0510.github.io/Ludonimbus/privacy.html)
