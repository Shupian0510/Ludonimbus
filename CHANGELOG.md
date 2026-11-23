# 更新日志 / Changelog

每个版本的说明会原样显示在 GitHub Release 页面和应用内的更新提示里。每一节都必须同时包含 `### 中文` 和 `### English`，发版时会自动检查。发版流程见 [docs/release-process.md](docs/release-process.md)。

Each section is shown as-is on the GitHub Release page and in the in-app update prompt. Every section must contain both `### 中文` and `### English`; this is checked when releasing. See [docs/release-process.md](docs/release-process.md).

## 0.2.0

### 中文

- 新增：在 Steam 里显示云存档状态。在「设置 → 在 Steam 中显示云存档状态」点「一键安装」，Ludonimbus 会从 GitHub 下载并校验开源插件框架 Millennium，安装到 Steam，再启用 Ludonimbus 插件。之后 Steam 游戏页的「开始游戏」栏会显示「Ludonimbus 云存档：已是最新」这类状态，从 Steam 启动、游戏中、上传中都会实时显示，也可以直接点同步按钮，或点它打开 Ludonimbus。
- 插件只负责显示，不接触网盘账号；需要 Ludonimbus 在托盘中运行。可以随时在设置里移除插件，Ludonimbus 更新时插件也会自动更新。
- 改进：同一个游戏不会再同时运行两次同步（界面、后台监视和 Steam 里的同步按钮共用一把锁）。

### English

- New: cloud save status inside Steam. In Settings → "Show cloud save status in Steam", click "Install". Ludonimbus downloads and verifies the open-source Millennium plugin framework from GitHub, installs it into Steam and enables the Ludonimbus plugin. The play bar of a game's library page then shows the status ("Ludonimbus cloud saves: up to date"), live while launching, playing and uploading, with a sync button; clicking it opens the game in Ludonimbus.
- The plugin only displays information and never touches your cloud account; Ludonimbus must be running in the tray. The plugin can be removed in Settings at any time and is updated together with Ludonimbus.
- Improved: the same game is never synced twice at once (the app, the background watcher and the sync button in Steam share one lock).

## 0.1.2

### 中文

- 新增：「游戏进程退出就上传，不等启动器关闭」，可以在每个游戏的「设置」里开关。适合启动器会一直开着的游戏：游戏一关就上传，关闭启动器时再检查一次。塞尔达传说：时之笛（Longshot）默认开启。
- 不经过 Steam、直接打开游戏时，托盘里的后台监视也遵循这个设置。
- 游戏设置页的「游戏进程」会显示内置规则里的进程名。

### English

- New: "Upload when the game process exits, without waiting for the launcher to close", configurable per game in its settings. Useful for launchers that stay open: saves are uploaded as soon as the game closes and checked again when the launcher closes. On by default for The Legend of Zelda: Ocarina of Time (Longshot).
- The tray watcher follows the same setting for games started outside Steam.
- The "Game process" field shows the process names from the built-in rule.

## 0.1.1

### 中文

- 修复：从 Steam 启动塞尔达传说：时之笛后，退出游戏要等最长 60 秒才上传，期间 Steam 一直显示「运行中」。原因是规则把 Longshot 一直留在磁盘上的锁文件当成「游戏还在写存档」；现在只有锁文件正被占用时才等待，最多 20 秒。
- 修复：在 Steam 里点「停止」强制结束后，存档没有上传。现在托盘里的 Ludonimbus 会发现并立即补传。
- 改进：从 Steam 启动游戏前的同步更快：Google Drive 文件夹 id 缓存到本地，设备信息等记录改到游戏退出后再更新。
- 改进：启动器运行超过 20 秒时，退出后不再额外等待 8 秒。
- 改进：游戏结束后界面自动刷新同步状态；每 10 分钟重新检查一次云端。

### English

- Fixed: after playing The Legend of Zelda: Ocarina of Time from Steam, the upload waited up to 60 s after exit while Steam kept showing the game as running. The rule treated Longshot's leftover lock file as "still writing saves"; Ludonimbus now only waits while a lock file is actually in use, for at most 20 s.
- Fixed: saves were not uploaded after pressing "Stop" in Steam. The Ludonimbus tray app now notices and uploads right away.
- Improved: faster sync before launching from Steam. Google Drive folder ids are cached locally, and bookkeeping happens after the game exits.
- Improved: no extra 8 s wait after exit when the launcher ran for more than 20 s.
- Improved: statuses refresh automatically after a game exits, and every game is re-checked against the cloud every 10 minutes.

## 0.1.0

### 中文

第一个版本。

- 像 Steam 云存档一样同步非 Steam 游戏的存档：从 Steam 启动前拉取，退出后上传。
- 云盘：一键连接 Google Drive（存档在你自己的 Drive 里），或者任意文件夹（Google Drive / OneDrive 电脑版同步的文件夹、NAS、移动硬盘）。
- 版本历史：每个版本记录时间、设备名和触发方式，可以恢复、导出 zip、锁定、写备注；自动清理旧版本，手动备份永久保留。
- 冲突处理：两台设备都改过存档时让你选择，没选的那份也保留在版本历史里。
- 离线可用：连不上云端时直接启动游戏，退出后的存档先存在本机，联网后自动补传。
- 模拟器更新不会断：快捷方式只记录游戏 id；解压新版本到新文件夹时自动找到；快捷方式被改动时提醒并一键修复。
- 内置规则：塞尔达传说：时之笛（Longshot）、火焰纹章：万缕千丝（fefw recomp）、Bloodborne（Lantern Engine）；PC 游戏可以从 Ludusavi 数据库搜索添加。
- Steam 风格的游戏库（网格 / 列表），使用你在 Steam 里设置的封面；托盘后台监视不经过 Steam 启动的游戏。
- 应用内自动更新。

注意：安装包暂未进行代码签名，Windows 可能提示「未知发布者」，点「更多信息 → 仍要运行」即可。修改 Steam 快捷方式时需要暂时关闭 Steam（Ludonimbus 会自动关闭并重新打开）。

### English

First release.

- Steam-Cloud-like save sync for non-Steam games: saves are pulled before launching from Steam and uploaded after the game exits.
- Storage: one-click Google Drive (saves live in your own Drive), or any folder (a Google Drive / OneDrive desktop sync folder, a NAS share, a USB drive).
- Version history: every version records the time, device and trigger; restore it, export it as a zip, lock it or add a note. Old automatic versions are cleaned up; manual backups are kept forever.
- Conflicts: when two devices both changed the saves, you choose which one to use; the other one stays in the history.
- Works offline: if the cloud is unreachable the game starts anyway; new saves are kept locally and uploaded later.
- Emulator updates don't break sync: Steam shortcuts only store a game id, a new version extracted into a new folder is found automatically, and edited shortcuts are detected and repaired with one click.
- Built-in rules: The Legend of Zelda: Ocarina of Time (Longshot), Fire Emblem: Fortune's Weave (fefw recomp), Bloodborne (Lantern Engine). PC games can be added from the Ludusavi database.
- Steam-style library (grid / list) using your Steam custom artwork; a tray watcher handles games started outside Steam.
- In-app auto-update.

Note: the installer is not code-signed yet, so Windows may show "Unknown publisher"; click "More info → Run anyway". Changing Steam shortcuts requires closing Steam briefly (Ludonimbus closes and reopens it).
