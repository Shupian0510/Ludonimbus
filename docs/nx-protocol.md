# Switch 客户端的局域网协议（协议版本 3）

> Switch 上的自制程序（[Ludonimbus-NX](https://github.com/Shupian0510/Ludonimbus-NX)，C++ + libnx）通过这台电脑备份、取回存档时用的接口。设计见 [plan-platforms.md](plan-platforms.md) 4.3 和 [plan-positioning.md](plan-positioning.md) 4.6、S5，电脑这边的实现在 [nxlan.rs](../crates/core/src/nxlan.rs)，测试在 [nx_lan.rs](../crates/core/tests/nx_lan.rs)（按 Switch 的角色走一遍）；Switch 那边的 `tests/host` 对真正的 HTTPS 服务再走一遍（两台 Switch）。

## 思路

**上传总是一键的，写进 Switch 只在用户选了以后。** Switch 上 A 只备份（在云端多一个版本，不覆盖任何东西），X 列出能取回的存档，用户选一份、确认，Switch 先把现在的存档留一份在云端再写入，L 撤销。

**电脑是 Switch 的本地中转站。** 云盘可能很慢（夸克经 OpenList 中转，每个请求几秒），所以电脑**从不让 Switch 等云盘**，只有一个例外（写入前的冲突检查）：

- **状态马上回答**：用电脑记着的云端游戏列表（存档库的那份缓存）和每条存档线最近一次查到的最新版本。旧了就在后台重新查，告诉 Switch「过一会儿再问」（`fresh: false`）。
- **上传先存在电脑上**：Switch 的存档进电脑的待上传队列（`pending/<key>/`），马上回复；电脑在后台传到云端，失败了隔一阵重试（5 秒起，最长 5 分钟），电脑重启后接着传。
- **写入前按版本检查**：Switch 说「接在哪个版本后面」（`parent`），电脑当场查一次云端（加上队列里还没传的）。
- **下载是后台任务**：电脑从云端取回，Switch 每秒问一次进度，取好了再一次拿走。
- 电脑只连一次云盘，所有请求共用（10 分钟后重新连）。

## 存档线：每个玩家一条

每台配对的 Switch 是一台设备（云端设备 id `nx-<Switch 的 id>`，名字在电脑上改）。Switch 上的每个使用者（账号）的存档单独备份：

| 规则 | |
|---|---|
| 同名就是同一个玩家 | 几台 Switch 上昵称相同（不分大小写、去掉两头空格）的使用者共用一条存档线：在一台上备份，另一台显示「Bedroom 有更新的进度」，X 取回 |
| 分成不同使用者 | 电脑上把某台 Switch 的某个使用者设为「分成不同使用者」，它单独一条线（plan-v2 第 7 节的独立存档） |
| 不同步 | 电脑上关掉的使用者，Switch 不读、电脑不答它的存档 |
| 默认 | 新出现的使用者同步、按名字共用 |

- 线在游戏卡上，id `switch-` 加 6 个字符（按名字共用时由昵称算出，分开时由 Switch 和使用者 id 算出），所以几台电脑、几台 Switch 不用商量就落到同一条线。线的名字是「Switch · 昵称」（分开时「Switch · 昵称 · Switch 名」），电脑的游戏页能看到、能跟随。
- 线的种类（`kind`）是 `switch`：只有 Switch 自己接着这些线走，同一个 title 的模拟器是另一个版本，不会自动加入。Switch 第一次备份一个云端还没有的游戏时建的卡，它的 `main` 线也是 `switch`（协议 2 的 Switch 存在那里）。
- **Switch 的线和电脑上模拟器的线分开**（plan-positioning 第 3 节：Switch 实机 ↔ 模拟器是「发到…」）。同一个 title id 照样在同一张卡上；第一个同步的使用者的状态里会提示「PC 版有更新的进度」，X 里能取回。
- 电脑内部用**线键** `<游戏 key>~<线 id>` 指一条线（`cloud::client::line_key`），队列、锁、缓存都按它分开，和电脑自己的游戏互不干扰。

## 连接

- 电脑上「设置 → Switch → 允许 Switch 连接」打开后才监听，默认关闭。
- **HTTPS**，端口 `47716`，电脑用自签名证书（`%APPDATA%\Ludonimbus\nx\cert.pem`，第一次打开时生成）。
- **发现**：向 UDP `47717` 广播 `LUDONIMBUS_DISCOVER`（19 字节），电脑回一个 JSON：

  ```json
  { "app": "ludonimbus", "name": "DESKTOP-MAIN", "port": 47716, "fingerprint": "b4bdeea6…" }
  ```

  收不到时让用户手动输入电脑上显示的地址。
- **证书固定**：`fingerprint` 是证书 DER 的 SHA-256（64 位小写十六进制）。配对时 Switch 显示前 16 位让用户和电脑屏幕核对，记下完整指纹；以后 TLS 握手时只接受这个证书（不做 CA 校验）。
- 除 `hello` 和 `pair` 以外，每个请求都带 `Authorization: Bearer <设备令牌>`。令牌错误或设备已被移除：`401`。
- 出错时返回 `{"error": "…"}` 和相应的状态码。
- 一个连接一个请求（`Connection: close`），请求体带 `Content-Length`（不接受 chunked：`411`）。令牌在读请求体之前检查；请求体最大：上传 512 MB，其他请求 1 MB，不带令牌的（`pair`）16 KB，超过返回 `413`。电脑同时最多处理 16 个连接（同一个地址 4 个），30 秒没有收发的连接会被关掉。
- 除上传外，电脑都马上回答；Switch 的超时设 60 秒（上传要等电脑查一次云端）。

## 存档的路径

和电脑上模拟器的存档用同一套路径根：

| Switch 上 | 路径根 |
|---|---|
| 一个使用者的账号存档（`FsSaveDataType_Account`） | `nxsave-<title id>/…` |
| 设备存档（`FsSaveDataType_Device`） | `nxdevice-<title id>/…` |

`<title id>` 是 16 位**小写**十六进制（`0100770008dd8000`）；URL 里的 `{title}` 也是这个格式。zip 里的文件名就是这样的路径（`/` 分隔，不以 `/` 开头），只能包含这个游戏的两个路径根。设备存档跟着每个使用者的版本一起传（Switch 上只有一份）；只有设备存档的游戏只列在第一个同步的使用者下面。

`<uid>` 是 Switch 使用者 id，32 位十六进制。

## 接口

### `GET /nx/v1/hello`

```json
{ "app": "ludonimbus", "version": "1.0.0", "protocol": 3, "name": "DESKTOP-MAIN", "pairing": true }
```

- `protocol`：Switch 配对前核对，不一致时提示更新电脑上的 Ludonimbus 或 Switch 上的应用。
- `pairing`：电脑上正显示配对码。

### `POST /nx/v1/pair`

```json
{ "code": "482915", "id": "<Switch 的设备 id>", "name": "Switch" }
```

- `id`：Switch 第一次运行时生成并存在 SD 卡上（字母、数字、`-`，不超过 64 个字符）。
- 成功：`{ "token": "<64 位十六进制>", "name": "<电脑名>" }`，Switch 保存令牌和证书指纹。
- 配对码 5 分钟有效，输错 5 次作废，配对成功一次后作废：`403`。同一台 Switch 重新配对时，电脑上给它起的名字和使用者设置保留。

### `POST /nx/v1/status?client=<客户端版本>`

Switch 报告自己的使用者和（电脑同步的使用者的）有存档的游戏：

```json
{ "users": [ { "uid": "<uid>", "nickname": "Shupian" } ],
  "games": [ { "title": "0100770008dd8000", "user": "<uid>", "lastSyncedId": "01J…", "changed": true } ] }
```

- `lastSyncedId`：这个使用者的这个游戏上次和电脑同步（上传或取回）的版本 id；没同步过省略。
- `changed`：自那次同步以后 Switch 上的存档变了没有（Switch 自己比较：文件大小加 CRC32）。

**马上**返回电脑现在知道的：

```json
{ "fresh": true, "error": null,
  "device": { "name": "Bedroom" },
  "users": [ { "uid": "<uid>", "nickname": "Shupian", "sync": true, "separate": false, "line": "Switch · Shupian" } ],
  "games": [ { "title": "0100770008dd8000", "user": "<uid>", "key": "mhgu", "name": "MONSTER HUNTER GENERATIONS ULTIMATE",
               "status": "cloudNewer", "from": "switch", "pending": false,
               "head": { "id": "01J…", "deviceName": "Living room", "createdAt": "…", "…": "…" },
               "newer": { "id": "01J…", "label": "Ryujinx", "deviceName": "DESKTOP", "createdAt": "…", "convert": false } } ] }
```

| `status` | 意思 | Switch 显示 |
|---|---|---|
| `upload` | 这个玩家的线上还没有这个游戏的存档 | 未备份 |
| `synced` | 一样 | 已备份（有 `newer` 时：PC 版有更新的进度） |
| `localNewer` | Switch 改过，云端没变 | Switch 较新 |
| `cloudNewer` | 线上有更新的版本，Switch 没改 | `from: switch`：「<head.deviceName> 有更新的进度」；否则：云端较新 |
| `conflict` | 两边都改过 | 两边都改了 |
| `checking` | 电脑正在向云端查 | 等一会儿再问 |
| `unknown` | 电脑查不到（云端连不上，见 `error`） | 显示问题 |

- `from`：最新版本是谁存的：`this`（这台 Switch）、`switch`（另一台 Switch）、`other`。
- `newer`：只给第一个同步的使用者（不分开的）：卡上别的版本（不是 Switch 玩家的线）里最新的一份比这个玩家的最新版本新，并且 Switch 能用（同为 `nx` 格式，或者游戏有跨平台规则，`convert: true`）。没有规则的平台不提示。
- `users`：电脑上为这台 Switch 的每个使用者设的；Switch 记下来，`sync` 变了就重新读存档再问一次。`device.name`：电脑上给这台 Switch 起的名字。
- `fresh: false`：有些线是电脑在后台重新查的（超过 30 秒就算旧；`newer` 用到的别的线 5 分钟），或者云端游戏列表还从没取过。Switch 大约每 2 秒再问一次（不用重新读存档），直到 `fresh: true`。
- `pending: true`：电脑上还有这个玩家的版本没传到云端（Switch 显示「正在上传」）。状态已经把它算作最新版本。
- 电脑不同步的使用者的游戏不在回答里。
- 这次调用也会（在后台，最多 10 分钟一次）把 Switch 登记为一台设备（名字是电脑上起的，系统 `switch`）。

### `GET /nx/v1/games/{title}/sources?user=<uid>`

X「取回…」列出的存档，按来源分组。这个请求会当场查云端（玩家的线、卡上别的线）。

```json
{ "key": "mhgu", "line": "switch-4k2m9q", "headId": "01J…", "forked": false,
  "items": [ { "id": "01J…", "line": "switch-4k2m9q", "group": "this", "label": "Bedroom", "deviceName": "Bedroom",
               "createdAt": "…", "trigger": "sync-now", "fileCount": 3, "totalSize": 1234, "convert": false, "head": true } ] }
```

| `group` | 是什么 |
|---|---|
| `this` | 这台 Switch 在这条线上最新的一份 |
| `kept` | 保留的存档（`label` 是起的名字） |
| `switch` | 每台别的 Switch 在这条线上最新的一份（`label` 是它的名字） |
| `platform` | 卡上别的版本的线（模拟器、PC 版……）各自最新的一份；`line` 是那条线，`convert: true` 是要按跨平台规则转换的 |
| `auto` | 这条线上其余的版本，新的在前，最多 60 个（Switch 折成一行） |

- `forked`：这条线有几个最新版本（两台 Switch 各自备份过，见下面的 `fork`）。

### `POST /nx/v1/games/{title}/snapshots?user=<uid>&parent=<id>&name=<游戏名>[&fork=1|&backup=1|&force=1][&from=<id>&fromLine=<线>&how=restore|copy|convert]`

请求体是 zip（同样只能含这个游戏的两个路径根，最大 512 MB）。电脑先查一次这条线的最新版本（加上队列里的），再把它做成一个新版本（设备名是这台 Switch）放进待上传队列，马上回复，之后在后台传到云端。线还不存在时（这个玩家第一次备份这个游戏），电脑在卡上建这条线；云端还没有这个游戏时，先建游戏（key `nx-<title id>`，存档规则 `{nxsave-<title id>}/**`、`{nxdevice-<title id>}/**`，身份 `nx:<TITLE ID>`，`name` 作游戏名）。

- 成功：`{ "id": "01J…", "key": "mhgu", "pending": true, "forked": false }`，Switch 把 `lastSyncedId` 记成 `id`。
- **`fork=1`**（A「备份」用这个）：接在 `parent` 后面，**从不拒绝**。线上已经有更新的版本（另一台 Switch 在这之后备份过）时，两个版本都是最新版本（`forked: true`），两台 Switch 各自看到对方「有更新的进度」，由用户用 X 决定用哪一份。
- 不带 `fork`、`backup`、`force`：`parent` 不是线上最新的那个（或者省略了而线上已有版本）时 **`409`**，`{ "error": "…", "head": { 最新版本 } }`，什么都不存。（协议 2 的写法，留着。）
- `backup=1`：**只作备份**。Switch 上的存档马上要被覆盖时先存一份（和电脑上「恢复前」的版本一样）：在版本历史里，但不会成为最新版本，也不做冲突检查。云端或队列里已经有内容完全相同的版本时不再存，返回那个版本的 id 和 `"existing": true`。备份后 Switch **不**改 `lastSyncedId`。
- `force=1`：接在所有最新版本后面（分叉就此合上）。取回一个不是最新的版本、撤销时用。
- `from`、`fromLine`、`how`：这份存档是从哪个版本取回的（`how`：`restore` 同一条线、`copy` 别的线、`convert` 按规则转换），记在新版本的 `origin` 里，触发方式记 `restore`。

### 下载：`POST /nx/v1/downloads` → `GET /nx/v1/jobs/{job}` → `GET /nx/v1/jobs/{job}/file`

1. `POST /nx/v1/downloads`，`{ "title": "…", "id": "01J…", "user": "<uid>", "line": "<线>", "base": "01J…" }` → `{ "job": "<16 位十六进制>" }`。电脑开始在后台取这个版本（还在队列里的直接从电脑上拿）。
   - `line`：版本在卡上的另一条线（`sources` 里的 `line`）；省略就是这个玩家自己的线。
   - 那条线不是 `nx` 格式（`convert: true`）：电脑按游戏的跨平台规则转换：规则认得的存档文件放到规则在 Switch 上的文件夹，Switch 上其余的文件（设置、设备存档）取自 `base`（Switch 刚留下的那份），那个平台自己的其余文件不带过来。
2. Switch 每秒问一次 `GET /nx/v1/jobs/{job}`：`{ "state": "working" | "done" | "failed", "seconds": 12, "error": "…" }`（用户可以不再等，电脑照样做完）。
3. `done` 后 `GET /nx/v1/jobs/{job}/file`：`application/zip`，只含 `nxsave-<title id>/…` 和 `nxdevice-<title id>/…`。没好时 `409`，不存在 `404`。取好的文件保留 10 分钟。

### `GET /nx/v1/games/{title}/snapshots?user=<uid>`

一条线的全部版本（新的在前），加上队列里还没传到云端的（`"pending": true`）。这个请求会当场查一次云端。

## Switch 上的三个按钮

| 按钮 | Switch 依次做 |
|---|---|
| A 备份 | 状态是 `upload`、`localNewer`、`conflict`（或者存档变了）：上传，带 `fork=1`、`parent` = `lastSyncedId`；`lastSyncedId` = 返回的 `id`。`synced`、`cloudNewer` 而存档没变：什么都不做 |
| Y 全部备份 | 对每一行做 A |
| X 取回… | ① `sources` 列出、用户选一份、确认「覆盖 Switch 上的存档？现在的会先保留到云端」 ② 上传现在的存档，带 `backup=1`（得到 `kept`） ③ 下载选中的版本（别的线带 `line`，要转换的带 `base` = `kept`） ④ 写入、提交 ⑤ 选中的就是这条线唯一的最新版本：`lastSyncedId` = 它；否则上传写入后的存档，带 `force=1` 和 `from`/`fromLine`/`how`，`lastSyncedId` = 返回的 `id` ⑥ 记下撤销：`kept`、来源、写入后的指纹；头部一行「Replaced … with <来源>'s save · L undo」 |
| L 撤销 | 存档自取回以后没变过：下载 `kept`、写入，上传带 `force=1`、`from` = `kept`，清掉撤销记录 |

所有覆盖之前，Switch 上原来的存档都在云端：取回前的 `backup=1`，撤销时它就是取回的那个版本。

## 和协议 2 的兼容

协议 2 的 Switch 应用（请求里没有 `user`、状态里没有 `users`）照旧能用：它的存档在电脑上这个 title 的游戏所跟的线上（和电脑的模拟器同一条线），状态、上传（含 `409`）、下载和以前一样。协议 2 的应用配对时核对 `protocol` 必须相等，所以新配对要先更新 Switch 上的应用；已经配对的照常连接。

## 存档库

Switch 的线和电脑上 Ryujinx、Eden 的同一个游戏（同一个 title id）在同一张卡上，各是各的线：电脑上的模拟器不会自动拉取 Switch 的存档，Switch 也不会自动写入模拟器的。电脑的游戏页可以让模拟器版本跟随「Switch · 昵称」那条线（共用进度）。电脑上没有这个游戏的模拟器条目时，它在游戏库里是「Switch」分组里的一张灰色卡。

同名但存档不通用的其他平台版本（例如 Cemu 里的 Wii U 版）**不会**合成一个游戏（见 [plan-phase5.md](plan-phase5.md) §5 的「存档格式」）；有跨平台规则的（星露谷物语）在同一张卡上，X 里标「需要转换」。

## 没做的

- Switch 设备在云端的记录不列它在各条线上同步到的版本（`installs` 为空），保留策略不会特意为它留着上次同步的版本（最新版本和保留的存档照常不删）。
- 转换只用规则的文件夹对应，不检查规则的 `newerOnly`（更新版本的游戏存的存档）。
- 电脑上「第一个同步的使用者」不能改；它只决定谁会看到「PC 版有更新的进度」。
