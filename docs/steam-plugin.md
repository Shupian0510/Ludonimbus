# Steam 插件与本机接口

Ludonimbus 0.2 起可以在 Steam 游戏库的游戏页里显示云存档状态。分三部分：

```text
Steam 客户端
  └─ Millennium（第三方开源插件框架，Ludonimbus 一键安装）
       └─ Ludonimbus 插件（plugin-millennium/，打包成 ludonimbus.star）
            ├─ 前端（TypeScript）：在「开始游戏」栏插入状态项，轮询状态
            └─ 后端（Lua）：读 agent.json，调用本机接口
                              │  HTTP，127.0.0.1，Bearer token
桌面应用（托盘）── 本机接口（app/src-tauri/src/agent.rs）── core 同步引擎
```

插件只负责显示和转发点击：没有网盘凭据，不做同步判定。Ludonimbus 没在运行时状态栏显示「Ludonimbus 未运行」，不影响 Steam 和游戏本身。

## 本机接口

托盘里的 Ludonimbus 启动时在 `127.0.0.1` 的随机端口上监听，并写入 `%APPDATA%\Ludonimbus\agent.json`：

```json
{ "port": 54262, "token": "<64 位十六进制，每次启动都会变>", "pid": 48080, "version": "0.2.0", "apiVersion": 1 }
```

正常退出时删除这个文件；被强制结束时文件会留下，插件连不上就当作「未运行」。

### 安全

- 只监听回环地址。
- 每个请求必须带 `Authorization: Bearer <token>`，否则 401。token 只在 `agent.json` 里（当前用户的 AppData）。
- 带 `Origin` 头的请求一律 403：网页里的脚本发出的跨域请求都会带它，所以浏览器里的网页无法调用。
- `Host` 必须是 `127.0.0.1:<端口>` 或 `localhost:<端口>`，防 DNS rebinding。

### 接口（v1）

| 方法 | 路径 | 说明 |
|---|---|---|
| GET | `/v1/ping` | `{ app: "ludonimbus", version, apiVersion }` |
| GET | `/v1/steam/apps/{appid}?account={id}&refresh=1` | Steam 快捷方式对应游戏的云存档状态。`account` 是 Steam 账号 id（`userdata` 下的文件夹名），同一个快捷方式在多个账号下时用来区分。`refresh=1` 时，如果缓存的检查结果超过 60 秒、或者之后又同步过，就重新向云端检查一次 |
| POST | `/v1/games/{key}/sync` | 在后台开始「立即同步」，立即返回 `{ started: true }`。游戏运行中或已在同步时返回 409 |
| POST | `/v1/games/{key}/open` | 显示 Ludonimbus 窗口并打开这个游戏 |
| GET | `/v1/games/{key}/snapshots` | （API 2，0.6）云端版本列表，新的在前，外加 `headId`、`lastSyncedId`、`deviceId`、`pending`，以及现在不能恢复的原因 `busy`（游戏运行中、正在同步） |
| POST | `/v1/games/{key}/restore/{id}` | （API 2）在后台恢复这个版本，立即返回 `{ started: true }`，进行中状态是 `syncing`。游戏运行中或已在同步时 409。和应用里的「恢复到这个版本」走同一个 `sync::restore` |
| GET | `/v1/games/{key}/playtime` | （API 2）所有设备的游戏时间：`totalSecs`、`lastPlayed`、各设备明细 |
| GET | `/v1/games/{key}/gamebackup` | （API 2）游戏本体备份的状态：`{ exeMissing, checked, latest: { versionId, createdAt, totalSize, deviceName } \| null, job: { kind, phase, done, total, … } \| null, lang }`。不等云端：本体缺失时在后台查一次最新备份（缓存 5 分钟），还没查到时 `checked` 为 `false`。见 [plan-game-backup.md](plan-game-backup.md) |
| POST | `/v1/games/{key}/gamebackup/install` | （API 2）显示 Ludonimbus 窗口，打开这个游戏的「从云端恢复」对话框（安装位置在应用里选） |

状态的返回值（没有关联到 Ludonimbus 的游戏返回 `{ "managed": false }`）：

```jsonc
{
  "managed": true,
  "key": "zelda-oot-longshot",
  "name": "塞尔达传说：时之笛",
  "autoSync": true,
  "steamEnabled": true,            // 快捷方式经过 ludonimbus-run
  "health": "ok",                  // ok | disabled | changed | runnerMoved | shortcutMissing | …
  "phase": "idle",                 // idle | syncing | preLaunch | playing | uploading
  "status": "synced",              // 同 sync::Status；还没检查过时为 null
  "message": null,
  "lastSyncAt": "2026-11-23T04:24:06Z",
  "lastSyncedDevice": "DESKTOP-MAIN",
  "cloudHead": { "deviceName": "LAPTOP", "createdAt": "…" },
  "pending": 0,
  "conflict": false,
  "lastError": null,
  "checkedAt": "…",
  "playtime": { "totalSecs": 74520, "lastPlayed": "…", "devices": [ /* 各设备明细 */ ] },   // 0.6
  "apiVersion": 2                                                                         // 0.6
}
```

`apiVersion` 从 0.6 起是 2。插件按它决定点击状态项是打开版本列表（2）还是打开 Ludonimbus（旧版应用）。

`phase` 的来源：应用自己正在同步（界面、本机接口、托盘监视共用一把锁，同一个游戏不会同时跑两个同步）→ `syncing`；`ludonimbus-run` 在 `running/<key>.json` 里写的阶段 → `preLaunch` / `playing` / `uploading`；否则看游戏进程是否在运行。

## 插件

源码在 `plugin-millennium/`，用 Millennium 官方的 [starlight](https://github.com/SteamClientHomebrew/Millennium/tree/main/starlight) 打包：

```powershell
./scripts/build-plugin.ps1     # npm ci → 类型检查 → starlight pack --release → app/src-tauri/plugin/ludonimbus.star
```

`prepare-sidecars.ps1` 会调用它，所以 CI 和发布构建都会带上插件；安装包里的位置是 `<安装目录>\millennium\ludonimbus.star`。插件版本号跟应用一致（`bump-version.ps1` / `check-versions.ps1` 都覆盖了）。

- 插件 id：`io.github.shupian0510.ludonimbus`（写进 Millennium 配置的 `enabledPlugins`）。
- 前端通过 `MainWindowBrowserManager.m_lastLocation.pathname` 得到当前游戏页的 appid，用 Millennium SDK 的 `playSectionClasses` 找到 Steam 的 `GameStatsSection`，插入一个和「最后运行日期」「游戏时间」同样样式的状态项。Steam 的类名是哈希过的，只能这样查；找不到时退而插在 PlayBar 下面。React 重绘会删掉它，`MutationObserver` 会补回来。
- Lua 后端返回 JSON 文本，但 Millennium 的 FFI 会先把它解析成对象再交给前端，所以前端两种都要接受（`parseReply`）。
- 状态项只借用 Steam 的类名（`GameStat`、`GameStatIcon`、`PlayBarLabel`、`PlayBarDetailLabel`）来获得原生的字体和颜色；只有找不到这些类名时才用插件自带的备用样式。
- 只在 `SP Desktop` 窗口里工作（不处理大屏幕模式）。普通 Steam 游戏、没有加进 Ludonimbus 的快捷方式不显示任何东西。
- 每 5 秒读一次缓存状态（很便宜），有同步在进行时改为 1.5 秒；每 2 分钟、切换游戏、同步结束时请求一次 `refresh`。
- 点击状态项（0.6）：用 Millennium SDK 的 `showModal` + `ModalRoot` / `DialogButton` 弹出版本列表（`frontend/versions.tsx`），可以恢复、立即同步、打开 Ludonimbus。存档冲突和快捷方式失效时仍然直接打开 Ludonimbus，因为只有应用里能处理。
- 「所有设备」游戏时间（0.6）：Ludonimbus 记录过游戏时间时，在状态项前面再插入一项，显示所有设备的总时间，悬停显示明细。Steam 自己的「游戏时间」只算这台电脑，插件不改它。

## 一键安装做了什么

「设置 → 在 Steam 中显示云存档状态 → 一键安装」（`app/src-tauri/src/steam_plugin.rs`，核心逻辑在 `crates/core/src/millennium.rs`）：

1. 检查 Steam 文件夹：`wsock32.dll` 和 `millennium\lib\millennium.dll` 都在 → 已安装 Millennium；有 `user32.dll` + `ext\` → 旧版 2.x，提示用官方安装程序更新，不自动处理。
2. 没装 Millennium 时：从 GitHub API 取最新正式版（不含 beta），下载 `millennium-<版本>-windows-x86_64.zip` 和同名 `.sha256`，校验后才继续。压缩包必须包含 `wsock32.dll` 和 `millennium/lib/millennium.dll`，且没有指向 Steam 文件夹外的路径，否则拒绝安装。
3. 关闭 Steam（`steam.exe -shutdown`），解压到 Steam 文件夹，复制插件到 `millennium\plugins\ludonimbus.star`，在 `millennium\config\config.json` 的 `plugins.enabledPlugins` 里加上插件 id（保留其他设置；配置文件损坏时不覆盖，直接报错）。Millennium 会在 Steam 退出时重写这个文件，所以必须在 Steam 关闭时改。
4. 如果 Steam 原来在运行，重新打开它。

这和 Millennium 官方安装程序的步骤相同（它也是下载 zip、校验 SHA-256、解压到 Steam 文件夹）。之后 Millennium 自己负责更新。

应用每次启动时，如果 Steam 里已经装了插件、但和安装包里的不一样，会直接替换文件（Steam 运行时也可以），下次打开 Steam 生效。

「移除插件」会删除插件文件并从 `enabledPlugins` 里去掉；Millennium 本身保留。

## 成就

设计见 [plan-achievements.md](plan-achievements.md)。本机接口多了 4 个端点（`app/src-tauri/src/achievements.rs`）：

| 方法 | 路径 | 说明 |
|---|---|---|
| GET | `/v1/steam/apps/{appid}/achievements?account={id}` | 游戏页的成就进度和列表；没有关联来源时 `source` 为 `null` |
| GET | `/v1/games/{key}/achievements/{id}/icon` | 本地奖杯图标，`{ dataUrl }` |
| GET | `/v1/achievements/events?after={seq}` | 游戏运行中新解锁的成就；`after=0` 只返回当前序号（插件重新加载后不补弹旧的） |
| POST | `/v1/achievements/events/{seq}/ack?shown=steam\|failed&reason=` | 插件报告弹窗结果；3.5 秒内没有 `steam`（或插件 10 秒内没来取过事件）时，应用用自己的置顶小窗口提醒 |

插件里：

- `frontend/toast.ts` 每 2 秒取一次新事件，用 Steam 前端的 `window.NotificationStore` 弹出 Steam 自己的成就通知：在 `GetNotificationTargets()` 里按 `strTest === "Achievement"` 找到通知类型和 protobuf 类，照 Steam 的 `TestAchievement` 构造数据；这个游戏的 Overlay 在运行时用 `AddAppOverlayNotification`（游戏内），否则用 `OnNotification`（桌面）。都是没有文档的内部接口，按名字查找，找不到时把原因报给应用。
- `frontend/achievements.ts` 在「开始游戏」栏加「成就 x / y」项（借用 Steam 的类名），点开是插件自己画的列表，每 20 秒或收到解锁时刷新。
