# Steam 插件与本机接口

Ludonimbus 0.2 起可以在 Steam 游戏库的游戏页里显示云存档状态。分三部分：

```text
Steam 客户端
  └─ Millennium（第三方开源插件框架，Ludonimbus 一键安装）
       └─ Ludonimbus 插件（plugin-millennium/，打包成 ludonimbus.star）
            ├─ 前端（TypeScript）：在「开始游戏」栏插入状态项，轮询状态
            └─ 后端（Lua）：读 agent.json，调用本机接口
                              │  HTTP，127.0.0.1，Bearer token
桌面应用（托盘）  ┐
                  ├─ 本机接口（crates/core/src/localapi）── core 同步引擎
ludonimbus-agent ┘   （没有窗口的后台服务：Steam Deck 游戏模式的 Decky 插件，或没开托盘应用的 Linux 桌面）
```

插件只负责显示和转发点击：没有网盘凭据，不做同步判定。Ludonimbus 没在运行时状态栏显示「Ludonimbus 未运行」，不影响 Steam 和游戏本身。

## 本机接口

本机接口只有一份实现：core 的 `localapi`。托盘里的桌面应用（`app/src-tauri/src/agent.rs`）和不带界面的 `ludonimbus-agent`（`crates/agent`）都用它，两边不同的地方通过 `localapi::Host` 提供（见下面「两个程序」）。

启动时在 `127.0.0.1` 的随机端口上监听，并写入数据文件夹里的 `agent.json`（Windows 是 `%APPDATA%\Ludonimbus\agent.json`，Linux 是 `~/.config/Ludonimbus/agent.json`）：

```json
{ "port": 54262, "token": "<64 位十六进制，每次启动都会变>", "pid": 48080, "version": "1.0.0", "apiVersion": 5, "headless": false }
```

`headless` 为 `true` 时是 `ludonimbus-agent` 写的。桌面应用启动时直接写这个文件，正常退出时删除（只删自己写的）；被强制结束时文件会留下，插件连不上就当作「未运行」。`ludonimbus-agent` 每 5 秒看一次：文件里的进程不在了（或没有文件）就写自己的，桌面应用在运行时让开。

### 安全和限制

- 只监听回环地址。
- 每个请求必须带 `Authorization: Bearer <token>`，否则 401。token 只在 `agent.json` 里（当前用户的数据文件夹；Linux 上文件权限是 0600）。
- 带 `Origin` 头的请求一律 403：网页里的脚本发出的跨域请求都会带它，所以浏览器里的网页无法调用。
- `Host` 必须是 `127.0.0.1:<端口>` 或 `localhost:<端口>`，防 DNS rebinding。
- 一个连接一个请求（回答带 `Connection: close`）。同时最多处理 16 个连接，更多的在系统里排队等着被接受；请求要在 10 秒内发完，请求行和头最多 16 KiB（超过 431），请求体最多 16 KiB（没有接口要请求体；超过 413，`Transfer-Encoding` 411）。

### 接口（v1）

除了 `ping`，`{key}` 不在 Ludonimbus 里时都是 404。版本 id（`restore`、`newer/{id}`）只能是字母和数字，否则 400。出错时返回 `{ "error": "<说明>" }`。

| 方法 | 路径 | 说明 |
|---|---|---|
| GET | `/v1/ping` | `{ app: "ludonimbus", version, apiVersion, lang, headless }` |
| GET | `/v1/steam/apps/{appid}?account={id}&refresh=1` | Steam 快捷方式（或 Steam 安装的游戏，API 4）对应游戏的云存档状态。`account` 是 Steam 账号 id（`userdata` 下的文件夹名），同一个快捷方式在多个账号下时用来区分。`refresh=1` 时，如果缓存的检查结果超过 60 秒、或者之后又同步过，就重新向云端检查一次 |
| POST | `/v1/games/{key}/sync?resolution=cloud\|local` | 在后台开始「立即同步」，立即返回 `{ started: true }`。游戏运行中或已在同步时返回 409。`resolution`（API 3）是冲突时用哪一份，其他值 400 |
| POST | `/v1/games/{key}/open` | 显示 Ludonimbus 窗口并打开这个游戏（只有桌面应用） |
| GET | `/v1/games/{key}/snapshots` | （API 2，0.6）云端版本列表，新的在前，外加 `headId`、`lastSyncedId`、`deviceId`、`pending`，以及现在不能恢复的原因 `busy`（游戏运行中、正在同步） |
| POST | `/v1/games/{key}/restore/{id}` | （API 2）在后台恢复这个版本，立即返回 `{ started: true }`，进行中状态是 `syncing`。游戏运行中或已在同步时 409。和应用里的「恢复到这个版本」走同一个 `sync::restore` |
| GET | `/v1/steam/playtime?account={id}` | （API 2）每个关联了快捷方式的游戏在所有设备上的总时间（分钟）：`{ apps: [{ appid, key, minutes }] }`，插件用它替换 Steam 显示的游戏时间 |
| GET | `/v1/games/{key}/playtime` | （API 2）所有设备的游戏时间：`totalSecs`、`lastPlayed`、各设备明细 |
| GET | `/v1/games/{key}/gamebackup` | （API 2，只有桌面应用）游戏本体备份的状态：`{ exeMissing, checked, latest: { versionId, createdAt, totalSize, deviceName } \| null, job: { kind, phase, done, total, … } \| null, lang }`。不等云端：本体缺失时在后台查一次最新备份（缓存 5 分钟），还没查到时 `checked` 为 `false`。见 [plan-game-backup.md](plan-game-backup.md) |
| POST | `/v1/games/{key}/gamebackup/install` | （API 2，只有桌面应用）显示 Ludonimbus 窗口，打开这个游戏的「从云端恢复」对话框（安装位置在应用里选） |
| GET | `/v1/games/{key}/newer` | （API 3）同一个游戏的另一个版本（例如 Steam 版和重编译版）在云端有更新的存档时返回 `{ key, name, label, snapshot }`，否则 `null` |
| POST | `/v1/games/{key}/newer/{id}/apply` | （API 3）在后台把这份存档应用到这个游戏，立即返回 `{ started: true }`；`{id}` 不是 `newer` 给的那份时 404，游戏运行中或正在同步时 409 |
| POST | `/v1/games/{key}/newer/{id}/dismiss` | （API 3）这份不再提示 |
| GET | `/v1/steam/plugin/poll?session=&kind=&version=&caps=&account=` | （API 5）插件每 2 秒问一次：报上自己能做什么（`caps`：`cloud`、`launchOptions`），取走等着它的请求。返回 `{ requests: [{ id, appid, account, kind: "read" \| "cloud" \| "launchOptions", on?, value? }], watch: [appid…], resend, apiVersion }`。见下面「经插件改 Steam 的设置」 |
| POST | `/v1/steam/plugin/done/{id}?session=&ok=1\|0&error=&<报告>` | （API 5）插件做完一个请求，带上 Steam 现在的设置 |
| POST | `/v1/steam/apps/{appid}/report?session=&account=&<报告>` | （API 5）插件主动报告 `watch` 里的游戏在 Steam 里的设置（变了才报） |
| POST | `/v1/games/{key}/steam-cloud/take-over` | （API 5）Steam 游戏页上的「交给 Ludonimbus」：只给「只备份」的游戏，插件连着时在后台接管，立即返回 `{ started: true }`；不是只备份、插件没连上、游戏在运行或在同步时 409 |

`<报告>` 是 Steam 的 app details 里的几项：`available`、`accountEnabled`、`appEnabled`（`bCloudAvailable`、`bCloudEnabledForAccount`、`bCloudEnabledForApp`，1 或 0）、`status`（`eCloudStatus`）、`launchOptions`（`strLaunchOptions`，转义后的；读不到时不带）。

在后台做的事（同步、恢复、应用另一个版本的存档）结束后，失败时把错误写进这个游戏的 `lastError`；成功且没有剩下要做的事时清掉它；版本还在排队上传、或者没连上云端时，保留同步自己记下的原因。

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
  "lang": "zh-CN",
  "playtime": { "totalSecs": 74520, "lastPlayed": "…", "devices": [ /* 各设备明细 */ ] },   // 0.6
  "store": null,                   // API 4：Steam 安装的游戏由谁管存档：steam | backup | managed
  "backlog": null,                 // API 4：玩完后还没传完的：{ state: "uploading" | "waiting", versions, bytes }
  "apiVersion": 5
}
```

`apiVersion` 从 0.6 起是 2，0.7 是 3，1.0 是 5（4 是 1.0 的测试版）。插件按它决定点击状态项是打开版本列表（2 起）还是打开 Ludonimbus（旧版应用）。

`phase` 的来源：提供接口的程序自己正在同步（桌面应用里界面、本机接口、托盘监视共用一把锁，同一个游戏不会同时跑两个同步）→ `syncing`；`ludonimbus-run` 在 `running/<key>.json` 里写的阶段 → `preLaunch` / `playing` / `uploading`；否则看游戏进程是否在运行。

### 两个程序

| | 桌面应用（托盘） | `ludonimbus-agent` |
|---|---|---|
| `ping` / `agent.json` 的 `headless` | `false` | `true` |
| 「正在同步」 | 和窗口、托盘监视共用的那一份（`app/src-tauri/src/busy.rs`） | 只有这个进程自己的 |
| 「游戏在运行」 | `watcher::game_running` | 包装器的标记文件 + 游戏进程（core 里同样的判断） |
| 后台的同步结束时 | 通知窗口（`sync-finished` 事件） | — |
| `open`、`gamebackup`、`gamebackup/install` | 有 | 404（没有窗口） |
| 成就 | 托盘里的检测线程往事件队列里放新解锁 | 读得到成就列表和图标，但不检测新解锁，`events` 一直是空的 |

### 经插件改 Steam 的设置（API 5，1.0）

单个游戏的 Steam 云开关存在哪里没有官方说法，Steam 运行时还会覆盖自己的文件，所以 Ludonimbus 请插件让 Steam 自己改（`SteamClient.Apps`，两个插件都能用）。实现在 `crates/core/src/localapi/plugin.rs`，插件端是 `plugin-millennium/frontend/hands.ts` 和 `plugin-decky/src/hands.ts`。

- **问答**：插件是本机接口的客户端，所以 Ludonimbus 把请求放进队列，插件每 2 秒 `poll` 一次时取走。读、切换云开关、设启动选项三种：先读（云开关是「切换」不是「设置」），不一样才改，等 Steam 显示出改动（最多 5 秒），再读一次，用 `done` 带回 Steam 现在的设置。请求只交给登录同一个 Steam 账号的插件；10 秒没被取走、或取走后 25 秒没回答，就算插件没回应。
- **报告**：`poll` 的 `watch` 是游戏库里 Steam 装的游戏。插件对它们注册 `RegisterForAppDetails`，设置一变就 `report`。报告写进数据文件夹的 `steam-plugin.json`，别的程序（包装器）也读得到。只有发报告的那个插件还在 `poll`（60 秒内）时报告才算数；Steam 关了、插件没了，就回到读 Steam 的文件。这个程序第一次听到某个插件时 `resend` 为真，插件把报告全部重发一遍。
- **谁用**：`steamcloud::recheck`（每次同步前、包装器启动前）先看报告，没有才读文件，读到开着照旧退回「只备份」；游戏页的状态和启动选项是否完好也先看报告。接管（`take_over_through`）：读 Steam → Steam 没同步完（`eCloudStatus` 说了算，没说才看文件）就拒绝 → 存「接管前」→ 插件关掉云开关并核实 → 插件设启动选项并核实（失败就把云开关开回去）→ 改条目。插件能关开关但不能设启动选项时，关掉开关后照旧改 `localconfig.vdf`（要关 Steam）。交还（`hand_back_through`）先让 Steam 打开云开关，再把启动选项放回去。
- **应用里**：插件连着时，接管对话框就是一个确认框（「会关掉这个游戏的 Steam 云存档」，细节在「?」里），不用重启 Steam；「全部接管」也不再要求关掉 Steam 的总开关。交还前确认一次（会重新打开 Steam 云存档）。没有插件、或插件是旧版：和以前一样手动关开关。
- **Steam 里**：「只备份」的游戏，点状态项弹出的版本列表里有「交给 Ludonimbus…」，Steam 自己的确认框，确认后在应用里接管，列表一直跟到结束；Decky 面板里是同样的按钮。插件能设启动选项时用包装器，否则在后台同步。
- 旧插件从不 `poll`，什么都不报；旧应用对 `poll` 回 404，插件就每 15 秒才问一次，别的都不做。

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
- 游戏时间（0.6，`frontend/steamplaytime.ts`）：Steam 显示的游戏时间来自 `window.appStore.GetAppOverviewByAppID(appid).minutes_playtime_forever`（MobX 可观察字段）。插件每 30 秒取一次 `/v1/steam/playtime`，把这个字段改成所有设备的总时间，游戏页的「游戏时间」、游戏库悬停和按游戏时间排序都显示总数。只改显示：Steam 照常自己计时，不写它的文件；Steam 自己更新这个字段时（它计过时的一局之后）取两者中较大的，所以 Ludonimbus 没看到的时间不会被盖掉；游戏不再由 Ludonimbus 管理或插件停止时，放回 Steam 自己的值。
- 「开始游戏」栏里 Ludonimbus 的项目排在 Steam 自己的项目后面，顺序是云存档、成就。这一行放不下时会换行，而第二行不显示，所以最重要的云存档排在最前，窗口窄时先藏起来的是后面的项。成就区块只插在右栏最外层的容器里：Steam 把「录像及截图」等区块包在右栏里的另一个容器中，那个容器不算单独的一栏。
- 对齐（rc14 修）：Steam 的游戏开着 Steam 云时，「云状态」把它的另外四种状态隐藏着叠在一个 grid 里（`PlayBarCloudStatusContainer` / `SuperimposedGridItems`），这一行因此比一个统计项高一倍。状态项和 Steam 的一样贴着这一行的顶部，不居中（以前居中，在这样的页面上掉到下面、被截掉一半）。它从 120px 起、最多长到自己需要的宽度，放不下时文字用「…」截断，不会因为长被挤到看不见的第二行。Steam 装的游戏旁边就是 Steam 自己的「云状态」，所以标题只写「Ludonimbus」；「只备份」时只显示「已备份 · 2 小时前」，没有同步按钮。这些在一个按 Steam 的类名和样式（客户端的 `ui.css`）搭的模拟「开始游戏」栏里核对过。

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

设计见 [plan-achievements.md](plan-achievements.md)。本机接口里成就的端点（`crates/core/src/localapi`；事件队列在 `localapi::events`，桌面应用的检测线程 `app/src-tauri/src/achievements.rs` 往里放新解锁）：

| 方法 | 路径 | 说明 |
|---|---|---|
| GET | `/v1/steam/apps/{appid}/achievements?account={id}` | 游戏页的成就进度和列表；没有关联来源时 `source` 为 `null`；Steam 安装的游戏用 Steam 自己的成就，返回 `{ "managed": false }` |
| GET | `/v1/games/{key}/achievements/{id}/icon` | 本地奖杯图标，`{ dataUrl }`；没有时 404 |
| GET | `/v1/games/{key}/icon` | 游戏的图标，`{ dataUrl }`，给没有图标的成就用；没有时 404 |
| GET | `/v1/achievements/events?after={seq}` | 游戏运行中新解锁的成就：`{ seq, events, lang }`；`after=0` 只返回当前序号（插件重新加载后不补弹旧的）。序号从启动时间开始，重启应用后继续变大 |
| POST | `/v1/achievements/events/{seq}/ack?shown=steam\|failed&reason=` | 插件报告弹窗结果；3.5 秒内没有 `steam`（或插件 10 秒内没来取过事件）时，桌面应用用自己的置顶小窗口提醒。`shown` 是别的值、`seq` 不是数字时 400 |

插件里：

- `frontend/toast.ts` 每 2 秒取一次新事件，用 Steam 前端的 `window.NotificationStore` 弹出 Steam 自己的成就通知：在 `GetNotificationTargets()` 里按 `strTest === "Achievement"` 找到通知类型和 protobuf 类，照 Steam 的 `TestAchievement` 构造数据；这个游戏的 Overlay 在运行时用 `AddAppOverlayNotification`（游戏内），否则用 `OnNotification`（桌面）。都是没有文档的内部接口，按名字查找，找不到时把原因报给应用。
- `frontend/achievements.ts` 放在 Steam 给自己游戏显示成就的位置：「开始游戏」栏的「成就 x/y」（Steam 的 `GameStat MiniAchievements` 等类名，每一行统计都放，紧跟在云存档项之后）和右栏最上面的「成就」区块（`findClassModule` 找 `AppDetailsSection` / `RightColumnSection` / `SectionHeader`；右栏里还有包住「录像及截图」等区块的子容器，只在最外层放一个）。点开是插件自己画的列表，每 20 秒或收到解锁时刷新。
