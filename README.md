<p align="center">
  <img src="app/public/app-icon.svg" width="112" alt="Ludonimbus">
</p>

<h1 align="center">Ludonimbus</h1>

<p align="center">
  <b>给 Steam 里的非 Steam 游戏补上云存档</b><br>
  模拟器、重制版、第三方 PC 游戏，像 Steam 云存档一样自动同步。
</p>

<p align="center">
  <a href="https://github.com/Shupian0510/Ludonimbus/releases/latest"><img alt="最新版本" src="https://img.shields.io/github/v/release/Shupian0510/Ludonimbus?label=%E6%9C%80%E6%96%B0%E7%89%88%E6%9C%AC&color=1a9fff"></a>
  <img alt="Windows 10/11" src="https://img.shields.io/badge/Windows-10%20%2F%2011-0078d4">
  <a href="LICENSE"><img alt="MIT" src="https://img.shields.io/badge/license-MIT-4c1"></a>
</p>

<p align="center">
  <a href="https://github.com/Shupian0510/Ludonimbus/releases/latest"><b>下载</b></a>
  &nbsp;·&nbsp;
  <a href="https://shupian0510.github.io/Ludonimbus/">主页</a>
  &nbsp;·&nbsp;
  <a href="#english">English</a>
  &nbsp;·&nbsp;
  <a href="https://github.com/Shupian0510/Ludonimbus/issues">反馈问题</a>
</p>

---

Steam 云存档只对 Steam 商店里的游戏有效。你加进 Steam 的模拟器、重制版和其他游戏，存档只留在本机：重装游戏、换文件夹、更新模拟器或者换电脑时，很容易就丢了。

Ludonimbus 让这些游戏也拥有和 Steam 云存档一样的体验：从 Steam 启动前自动拉取最新存档，退出后自动上传，每个版本都留有记录。存档放在**你自己的** Google Drive、NAS、WebDAV / S3 存储或任意文件夹里，没有任何第三方服务器。

## 功能

- **和 Steam 云存档一样**：照常从 Steam 点「开始游戏」。启动前拉取云端较新的存档，退出后上传新存档，Steam 照常记录游戏时间。
- **在 Steam 里直接看到状态**：游戏页的「开始游戏」栏会显示「已是最新」「正在上传」「存档冲突」等状态，也可以一键同步。可选功能，在 Ludonimbus 里点一下就能装好。
- **删游戏不丢存档**：存档按游戏目录的相对位置记录。重装游戏、换文件夹、换电脑后，第一次启动就会自动恢复。
- **模拟器更新不断档**：解压新版本到新文件夹时会自动找到它；Steam 快捷方式被改动或重建时，提醒你一键修复。
- **版本历史**：每个版本都记录时间、设备和来源，可以恢复、导出 zip、锁定、写备注。旧版本自动清理，手动备份永久保留。
- **冲突时两份都留**：两台电脑都改过存档时让你选择用哪一份，没选的那份也留在版本历史里。
- **离线也能玩**：连不上云端就直接启动游戏，存档先保存在本机，联网后自动补传。
- **云盘随你选**：Google Drive（一键登录）、群晖等 NAS、坚果云、Nextcloud、阿里云 OSS / 腾讯云 COS 等对象存储；百度网盘、阿里云盘、夸克等可以通过 OpenList / AList 接入。换云盘时一键把历史版本复制过去。
- **Steam 风格的界面**：网格 / 列表游戏库，直接使用你在 Steam 里设置的封面、横幅和 Logo。

## 下载与安装

**系统要求**：Windows 10 / 11（64 位），已安装 Steam。

1. 从 [Releases](https://github.com/Shupian0510/Ludonimbus/releases/latest) 下载 `Ludonimbus_x.y.z_x64-setup.exe`。
2. 运行安装程序。装在当前用户下，不需要管理员权限。
3. 以后有新版本时，Ludonimbus 会提示你一键更新。

> 安装包暂未进行代码签名，Windows 可能提示「未知发布者」，点「更多信息 → 仍要运行」即可。

## 快速开始

1. **连接云盘**：在「设置 → 云盘」选一种：
   - **Google Drive**：在浏览器里登录即可。
   - **WebDAV**：群晖 / 威联通等 NAS、坚果云、Nextcloud，或者 [OpenList](https://github.com/OpenListTeam/OpenList) / AList（可以挂载百度网盘、阿里云盘、夸克等）。填地址、用户名和密码，先点「测试连接」。
   - **S3 兼容存储**：NAS 上的 MinIO，或阿里云 OSS、腾讯云 COS、Cloudflare R2。
   - **文件夹**：OneDrive / Google Drive 电脑版同步的文件夹、NAS 共享目录、移动硬盘。
2. **添加游戏**：「添加游戏 → Steam 快捷方式」，选择你加进 Steam 的游戏。内置规则支持的游戏会自动识别存档位置，其他游戏可以从 Ludusavi 数据库搜索或手动指定。
3. **开启 Steam 自动同步**：在游戏详情页打开「Steam 自动同步」。需要暂时关闭 Steam，Ludonimbus 会自动关闭并重新打开它。
4. **照常玩**：以后从 Steam 启动游戏就行。

关闭窗口后 Ludonimbus 会留在系统托盘。不通过 Steam、直接打开游戏时，它也会在游戏退出后自动备份。

## 在 Steam 里显示云存档状态

打开「设置 → 在 Steam 中显示云存档状态」，点「一键安装」。之后 Steam 游戏页的「开始游戏」栏里会多出一项云存档状态，和「最后运行日期」「游戏时间」并排显示：

- 从 Steam 启动、游戏中、退出上传时都会实时更新。
- 点右边的按钮立即同步，点状态本身会打开 Ludonimbus 里的这个游戏。
- 只对 Ludonimbus 管理的游戏显示，普通 Steam 游戏不受影响。

这个功能基于开源的 Steam 客户端插件框架 [Millennium](https://github.com/SteamClientHomebrew/Millennium)。如果你还没装 Millennium，Ludonimbus 会从它的 GitHub 官方发布页下载最新正式版，校验后再安装，和官方安装程序做的事情一样。Millennium 不是 Valve 官方组件；插件可以随时在同一个地方移除。

## 支持的游戏

**内置规则**（自动识别存档位置）：

| 游戏 | 运行方式 | 存档位置 |
|---|---|---|
| 塞尔达传说：时之笛 | Longshot | `%APPDATA%\Longshot\save` |
| 火焰纹章：万缕千丝 | fefw recomp | `<游戏目录>\save` |
| Bloodborne | Lantern Engine（shadPS4） | `<游戏目录>\user\savedata\1\CUSA00207` |

**其他游戏**：

- 普通 PC 游戏可以从 [Ludusavi](https://github.com/mtkennerly/ludusavi) 的数据库里搜索，覆盖上万款游戏。
- 也可以手动指定存档文件夹。

欢迎为 [`adapters/builtin.yaml`](adapters/builtin.yaml) 补充更多模拟器和游戏的规则。

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

开启自动同步后，Steam 快捷方式会先启动 Ludonimbus 的小程序 `ludonimbus-run`，由它在游戏前后完成同步；快捷方式的名称、图标、封面和游戏时间都不变。每个版本是一个只追加、不可修改的 zip 快照，记录了设备、时间和上一个版本，所以任何一次同步都可以撤回。

## 隐私与安全

- **存档只在你的电脑和你的云盘之间传输**，开发者没有服务器，也收集不到任何数据。
- **Google Drive 只能访问 Ludonimbus 自己创建的文件**，存档在你 Drive 里的「Ludonimbus」文件夹，网页版也能直接查看和下载。
- **WebDAV / S3 的密码只保存在本机**的 Windows 凭据管理器里，不会写进配置文件，断开时删除。
- **Steam 插件只负责显示**：它通过本机接口向托盘里的 Ludonimbus 询问状态，接口只监听本机，并且需要每次启动随机生成的令牌，不接触你的网盘账号。
- 详见[隐私政策](https://shupian0510.github.io/Ludonimbus/privacy.html)。

## 常见问题

<details>
<summary><b>不从 Steam 启动游戏也能同步吗？</b></summary>

可以上传，但不会在启动前拉取。Ludonimbus 在托盘里时，会发现游戏退出并自动备份。想要启动前也拉取最新存档，请从 Steam 启动。
</details>

<details>
<summary><b>两台电脑都玩过，存档冲突了怎么办？</b></summary>

启动游戏前会弹窗让你选择使用云端还是本机的那份。两份都会保存在版本历史里，选错了也可以随时恢复。
</details>

<details>
<summary><b>能用百度网盘、阿里云盘或夸克吗？</b></summary>

百度网盘一键登录已经开发完成，正在等待百度开放平台审核，通过后会在新版本中开放。在此之前，以及阿里云盘、夸克（没有面向个人开发者的官方接口），都可以通过 [OpenList](https://github.com/OpenListTeam/OpenList) / AList 接入：先在 OpenList 里挂载你的网盘，再在 Ludonimbus 里用 WebDAV 连接 OpenList，文件夹写在挂载的网盘下面。速度和稳定性取决于对应网盘和 OpenList 的驱动。坚果云自带 WebDAV，可以直接连接。
</details>

<details>
<summary><b>换云盘之后，以前的存档怎么办？</b></summary>

连接新云盘后，设置页会询问是否把原来云盘里的存档复制过去。复制会保留全部历史版本，原来的云盘不会被修改；中途断开也没关系，再点一次会从没复制的部分继续。
</details>

<details>
<summary><b>卸载 Ludonimbus 之前需要做什么？</b></summary>

在每个游戏的详情页关闭「Steam 自动同步」，Steam 快捷方式会恢复成原来的样子。云端的存档和版本都会保留，以后重新安装 Ludonimbus 并连接同一个云盘就能找回。
</details>

<details>
<summary><b>支持 macOS 或 Linux 吗？</b></summary>

目前只支持 Windows。
</details>

## 路线图

- [x] Google Drive / 同步文件夹、自动同步、版本历史、冲突处理、自动更新
- [x] 在 Steam 游戏页显示云存档状态，一键安装插件

**接下来**

- [x] WebDAV（NAS、坚果云、Nextcloud、OpenList）、S3 兼容存储，换云盘时一键复制存档
- [ ] 百度网盘一键登录（已开发完成，等待百度审核）
- [ ] 更方便地添加游戏：扫描本机已安装的游戏和已有的存档；认不出的游戏一步步引导你找到存档
- [ ] 不在 Steam 里的游戏一键加入 Steam，自动开启云存档，从 SteamGridDB 挑选封面
- [ ] 同步 Steam 截图和游戏时间，换电脑也能找回
- [ ] 更多模拟器规则（Ryujinx / Eden、RPCS3、Dolphin、RetroArch、PCSX2 …）

**以后再做**

- [ ] Steam 里的版本列表，不关 Steam 就能开关自动同步
- [ ] 游戏本体备份到云盘，在 Steam 里「从云端安装」
- [ ] 成就

## 参与开发

欢迎提交 [Issue](https://github.com/Shupian0510/Ludonimbus/issues) 和 Pull Request，尤其是新的模拟器 / 游戏存档规则。从源码构建和开发说明见 [`docs/development.md`](docs/development.md)。

## 致谢

- [Ludusavi](https://github.com/mtkennerly/ludusavi)（MIT）：存档位置数据库与查找，安装包附带它的官方程序。
- [Millennium](https://github.com/SteamClientHomebrew/Millennium)（MIT）：Steam 客户端插件框架。Ludonimbus 不附带它，只在你点「一键安装」时从官方发布页下载。
- [Tauri](https://tauri.app/)

## 许可

[MIT](LICENSE) © Shupian0510

---

<h2 id="english">English</h2>

**Ludonimbus brings Steam-Cloud-like save sync to non-Steam shortcuts**: emulators, recompilations and third-party PC games you added to Steam.

- **Works like Steam Cloud**: launch from Steam as usual. Newer saves are pulled before the game starts and new saves are uploaded after it exits.
- **Status inside Steam** (optional): the play bar of a game's library page shows "up to date", "uploading", "conflict" and so on, with a sync button. Ludonimbus installs the open-source [Millennium](https://github.com/SteamClientHomebrew/Millennium) plugin framework and its own plugin in one click.
- **Saves survive reinstalls**: saves are tracked relative to the game folder and restored on the first launch after reinstalling, moving the game or switching PCs.
- **Version history and conflict handling**: every snapshot records time and device; conflicts keep both sides.
- **Offline friendly**: if the cloud is unreachable the game starts anyway and saves are uploaded later.
- **Your own storage**: Google Drive (Ludonimbus can only see files it created), any WebDAV server (NAS, Nextcloud, OpenList…), S3-compatible storage, or any folder. Switching clouds copies your whole history over. There are no developer servers.

**Install**: Windows 10/11 64-bit. Download `Ludonimbus_x.y.z_x64-setup.exe` from [Releases](https://github.com/Shupian0510/Ludonimbus/releases/latest); the app updates itself afterwards. Then connect a cloud in Settings, add your Steam shortcuts and turn on "Steam auto-sync" for each game.

Built on [Ludusavi](https://github.com/mtkennerly/ludusavi)'s save database. Licensed under [MIT](LICENSE). Developer notes: [`docs/development.md`](docs/development.md).
