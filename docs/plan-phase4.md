# Ludonimbus 0.6 方案（草案 v0.1 · 2026-11-23）

> **进度（2026-11-23）**：第 0 节、C1、C2、I 已实现，测试清单见 [testing-v0.6.md](testing-v0.6.md)，等你在真实 Steam 上实测。实现和下文的差别：
> - 截图由托盘程序上传（游戏退出约 15 秒后、启动 1 分钟后、之后每 30 分钟），不在包装器里做：包装器不退出，Steam 就一直显示游戏在运行，第一次补传很多截图时会拖很久。
> - 截图的设置放在「设置 → Steam」，详情页的「截图」标签里也能直接切换。
> - 恢复接口用 `POST /v1/games/{key}/restore/{id}`（版本 id 放在路径里），不读请求体。
> - J（游戏本体上云）、K（成就）的会话已启动，各自在 `game-backup`、`achievements` 分支上进行。

> **背景**：0.5.0 已发布。下一个大版本先补全基础同步功能，再尝试两个进阶功能。进阶功能各在一个单独的会话里调研和实现，结论写进各自的方案文档，这里只记录分工。
>
> | 方向 | 一句话 | 在哪做 |
> |---|---|---|
> | **C1. 游戏时间** | 每台电脑记录自己的游戏时间，上传到云端，所有设备相加；导入 Steam 已有的时间 | 本文 |
> | **C2. 截图** | Steam 截图上传到云端，换电脑后出现在新电脑的 Steam 截图里，也能在 Ludonimbus 里看 | 本文 |
> | **I. Steam 里的版本列表** | 在 Steam 游戏页点云存档状态，直接列出版本历史，可以恢复 | 本文 |
> | **J. 游戏本体上云** | 游戏文件夹备份到云盘，在 Ludonimbus 或 Steam 里下载；先测速 | 单独会话，方案见 `docs/plan-game-backup.md` |
> | **K. 成就** | 用成就数据库给非 Steam 游戏记录成就，在 Steam 里显示，游戏内弹窗 | 单独会话，方案见 `docs/plan-achievements.md` |

---

## 0. 公共改动：云端的「通用文件」接口

游戏时间、截图、本体备份、成就都要往云端存「存档快照以外的东西」。现在 `CloudProvider` 的方法都绑定快照、设备、游戏定义；Google Drive 直接实现 `CloudProvider`，没有走 `Store`。

做法：`CloudProvider` 加一个方法 `files(&self) -> &dyn Store`，返回同一个云端文件夹的路径式读写。

- 走 `Store` 的云盘（WebDAV、S3、百度、夸克、本地文件夹）直接返回内部的 store。
- Google Drive 实现 `Store`：路径按段解析成文件夹 id（复用现有的文件夹缓存），`list` 多取 `mimeType`、`md5Checksum`、`modifiedTime` 作为变更标记。

所有新功能只通过 `Store` 读写，测试用本地文件夹或测试 WebDAV 服务，不用为每种云盘单独实现。

云端新增的目录：

```text
games/<key>/playtime/<device-id>.json          游戏时间（每台设备只写自己的文件）
games/<key>/screenshots/<device-id>/<文件名>.jpg
games/<key>/screenshots/<device-id>/thumbnails/<文件名>.jpg
```

都不进存档快照，不受保留策略清理。

---

## 1. C1：游戏时间

### 1.1 已实测的 Steam 数据（2026-11-23，只读）

- 非 Steam 快捷方式的游戏时间在 `userdata\<账号>\config\localconfig.vdf` 的 `UserLocalConfigStore > Software > Valve > Steam > apps > <id>`，键是快捷方式 appid 的**有符号 32 位**写法（例如时之笛 appid `4079546474` 写作 `-215420822`）。
- 字段：`Playtime`（分钟，累计）、`Playtime2wks`（分钟，两周内）。非 Steam 快捷方式没有 `LastPlayed`。
- 这个文件只在本机，Steam 不会跨设备同步非 Steam 游戏的时间。

### 1.2 记录

- **从 Steam 启动（包装器）**：`ludonimbus-run` 记录游戏从启动到退出的时间（不含启动前、退出后同步存档的时间）。开始时间写进 `running/<key>.json`，包装器被强制结束时，托盘程序用这个开始时间补记一条。
- **直接启动**：托盘程序的进程监视（每 3 秒）发现游戏在运行、但不是包装器启动的，记录从发现到消失的时间。包装器在跑的游戏不重复记。
- 每次游戏结束立刻写进本机的 `%APPDATA%\Ludonimbus\playtime\<key>.json`，有云盘时上传（失败不影响存档同步，下次再传）。

### 1.3 文件格式

```jsonc
// games/<key>/playtime/<device-id>.json，本机副本在 %APPDATA%\Ludonimbus\playtime\<key>.json
{
  "version": 1,
  "device": "<device-id>",
  "deviceName": "DESKTOP-MAIN",
  // 导入的 Steam 时间（这台电脑在 Ludonimbus 记录之前玩的时间），只导入一次
  "steamBaseline": { "minutes": 988, "account": "1436336854", "appid": 4079546474, "importedAt": "…" },
  "sessions": [
    { "start": "2026-11-23T10:00:00Z", "end": "2026-11-23T11:02:13Z", "secs": 3733, "via": "steam" }   // steam | direct
  ],
  "updatedAt": "…"
}
```

总时间 = 所有设备的 `steamBaseline.minutes × 60 + Σ sessions.secs`；最后游玩 = 所有设备最晚的 `end`。

### 1.4 导入 Steam 已有的时间

- 时机：这台电脑第一次为这个游戏建立游戏时间记录时（应用启动、包装器启动前），游戏关联了 Steam 快捷方式就读一次 `localconfig.vdf`。
- 只在「还没有导入过，且没有经过 Steam 启动的记录」时导入。否则 Steam 的累计时间里已经包含了我们记过的时间，再导入会重复计算。
- 导入后不再读 Steam 的时间：之后的时间都以 Ludonimbus 的记录为准，Steam 自己也在累计，但不参与相加。

### 1.5 显示

- **Ludonimbus**：游戏库卡片显示总时间；详情页显示总时间、最后游玩、各设备明细。
- **Steam 游戏页**：Steam 自己的「游戏时间」只算这台电脑。插件在旁边加一项「所有设备」，显示总时间，鼠标悬停显示各设备明细。
- 其他设备的文件按变更标记缓存在本机（`playtime\remote\`），游戏库离线也能显示上次的总数。

### 1.6 暂不做

- 把总时间写回 `localconfig.vdf`：需要关闭 Steam 才能写，收益只是 Steam 原生数字变化。先看插件的「所有设备」是否够用。

---

## 2. C2：截图

### 2.1 已实测的 Steam 数据（2026-11-23，只读）

- 截图在 `userdata\<账号>\760\remote\<gameid>\screenshots\*.jpg`，缩略图在同目录 `thumbnails\`（宽 200 像素）。
- 非 Steam 快捷方式的 `<gameid>` 是 64 位游戏 id：`(appid << 32) | 0x02000000`。例如 appid `4079546474` → `17521518688375668736`，和 `steam://rungameid/` 里的一样。
- 索引 `760\screenshots.vdf`（文本 VDF）：每个游戏一组，每张图记 `filename`、`thumbnail`、`width`、`height`、`gameid`、`creation`（Unix 时间）等；末尾 `shortcutnames` 记非 Steam 快捷方式的名字。

### 2.2 上传

- 时机：包装器在游戏退出、存档上传之后；托盘程序在直接启动的游戏退出之后；详情页「立即同步截图」。
- 本机快捷方式截图文件夹里、云端这台设备的文件夹里还没有的截图，连同 Steam 生成的缩略图一起上传。没有缩略图时只传原图。
- 本机记录已上传和已下载的文件名（`%APPDATA%\Ludonimbus\screenshots\<key>.json`），用户在 Steam 里删掉的截图不会被重新下载。

### 2.3 下载到 Steam

- 只下载**其他设备**上传、这台电脑从没下载过的截图，放进本机快捷方式对应的截图文件夹（按本机的 `SteamLink`，每台电脑 appid 可能不同），缩略图一起放进 `thumbnails\`。
- 同名文件已存在时跳过。
- `screenshots.vdf` 由 Steam 管理，运行时会被覆盖：0.6 先只放文件。**需要实测** Steam 重新打开后会不会自动收录文件夹里的新图；不会的话，再加「Steam 没在运行时把新图写进 `screenshots.vdf`」（宽高读 JPEG 文件头，时间取文件名）。

### 2.4 在 Ludonimbus 里看

详情页加截图墙：显示本机和云端的缩略图，点开看大图（按需下载原图到缓存），可以打开所在文件夹。

### 2.5 设置

「设置 → 截图」三个选项：**不同步**（默认，截图很占空间）/ **只上传** / **上传并下载到其他电脑**。显示云端截图占用的空间。

---

## 3. I：Steam 里的版本列表

- 点 Steam 游戏页的「Ludonimbus 云存档」状态项，弹出 Steam 风格的对话框（Millennium 的 `showModal`），列出版本：时间、设备、文件数和大小、备注、锁定标记、「当前」。
- 每个版本可以「恢复」（二次确认；游戏运行中或正在同步时禁用），对话框底部有「立即同步」「在 Ludonimbus 中打开」。
- 本机接口新增：

| 方法 | 路径 | 说明 |
|---|---|---|
| GET | `/v1/games/{key}/snapshots` | 云端版本列表（和应用里的版本页相同），外加本机等待上传的数量 |
| POST | `/v1/games/{key}/restore` | 请求体 `{ "id": "<版本 id>" }`，在后台恢复，立即返回 `{ started: true }`；游戏运行中或正在同步时 409 |
| GET | `/v1/games/{key}/playtime` | 总时间、各设备明细（状态接口里也带一份摘要） |

恢复走和应用里相同的 `sync::restore`，进行中状态显示「正在同步」。

---

## 4. 顺序

| 步骤 | 内容 |
|---|---|
| 1 | 第 0 节：`CloudProvider::files()`，Google Drive 实现 `Store`（进阶功能的会话也依赖它） |
| 2 | C1 游戏时间：记录、导入 Steam 时间、云端同步、应用和插件显示 |
| 3 | C2 截图：上传、下载到 Steam、截图墙、设置 |
| 4 | I 版本列表：本机接口、插件对话框 |
| 5 | 测试清单 `testing-v0.6.md`、文档站、CHANGELOG；你实测后发布 0.6.0 |

J、K 在各自的分支上进行，完成后再决定合并进 0.6 还是之后的版本。

## 5. 验收

- 两台设备（或用两个设备 id 模拟）各玩一次，总时间正确相加；第一次启用时导入了 Steam 原有的时间，没有重复计算。
- 在 A 电脑用 Steam 截的图，B 电脑同步后出现在 Ludonimbus 截图墙里，Steam 重新打开后出现在 Steam 的截图里。
- 在 Steam 游戏页打开版本列表，恢复一个旧版本，存档内容正确，Ludonimbus 里的版本历史多一条「恢复自…」。

## 6. 待验证

| 项目 | 说明 |
|---|---|
| Steam 会不会收录直接放进截图文件夹的新图 | 见 2.3，需要你在真实 Steam 上测 |
| Steam 运行时 `localconfig.vdf` 何时写盘 | 只影响导入的准确度（偏少，不会重复），见 1.4 |
| Millennium 的 `showModal` 在当前 Steam 版本的样式 | 插件对话框，你实测时看一眼 |
