# Switch 客户端的局域网协议（协议版本 2）

> Switch 上的自制程序（[Ludonimbus-NX](https://github.com/Shupian0510/Ludonimbus-NX)，C++ + libnx）通过这台电脑同步存档时用的接口。设计见 [plan-platforms.md](plan-platforms.md) 4.3，电脑这边的实现在 [nxlan.rs](../crates/core/src/nxlan.rs)，测试在 [nx_lan.rs](../crates/core/tests/nx_lan.rs)（按 Switch 的角色走一遍）；Switch 那边的 `tests/host` 对真正的 HTTPS 服务再走一遍。

## 思路：电脑是 Switch 的本地中转站

云盘可能很慢（夸克经 OpenList 中转，每个请求几秒），所以电脑**从不让 Switch 等云盘**，只有一个例外（写入前的冲突检查）：

- **状态马上回答**：用电脑记着的云端游戏列表（存档库的那份缓存）和每个游戏最近一次查到的最新版本。旧了就在后台重新查，告诉 Switch「过一会儿再问」（`fresh: false`）。
- **上传先存在电脑上**：Switch 的存档进电脑的待上传队列（`pending/<游戏>/`，和电脑断网时攒下的版本同一个队列），马上回复；电脑在后台传到云端，失败了隔一阵重试（5 秒起，最长 5 分钟），电脑重启后接着传。
- **写入前按版本检查冲突**：Switch 说「接在哪个版本后面」（`parent`），电脑当场查一次云端（加上队列里还没传的），对不上就拒绝（`409`）。所以状态是几秒前的也不会出错。
- **下载是后台任务**：电脑从云端取回，Switch 每秒问一次进度，取好了再一次拿走。
- 电脑只连一次云盘，所有请求共用（10 分钟后重新连）。

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
| 选定用户的账号存档（`FsSaveDataType_Account`） | `nxsave-<title id>/…` |
| 设备存档（`FsSaveDataType_Device`） | `nxdevice-<title id>/…` |

`<title id>` 是 16 位**小写**十六进制（`0100770008dd8000`）；URL 里的 `{title}` 也是这个格式。zip 里的文件名就是这样的路径（`/` 分隔，不以 `/` 开头），只能包含这个游戏的两个路径根。

## 接口

### `GET /nx/v1/hello`

```json
{ "app": "ludonimbus", "version": "0.7.0", "protocol": 2, "name": "DESKTOP-MAIN", "pairing": true }
```

- `protocol`：Switch 配对前核对，不一致时提示更新电脑上的 Ludonimbus 或 Switch 上的应用。
- `pairing`：电脑上正显示配对码。

### `POST /nx/v1/pair`

```json
{ "code": "482915", "id": "<Switch 的设备 id>", "name": "Switch" }
```

- `id`：Switch 第一次运行时生成并存在 SD 卡上（字母、数字、`-`，不超过 64 个字符）。
- 成功：`{ "token": "<64 位十六进制>", "name": "<电脑名>" }`，Switch 保存令牌和证书指纹。
- 配对码 5 分钟有效，输错 5 次作废，配对成功一次后作废：`403`。

### `POST /nx/v1/status?client=<客户端版本>`

Switch 报告自己有存档的游戏：

```json
{ "games": [ { "title": "0100770008dd8000", "lastSyncedId": "01J…", "changed": true } ] }
```

- `lastSyncedId`：这个游戏上次和电脑同步（上传或下载）的版本 id；没同步过省略。
- `changed`：自那次同步以后 Switch 上的存档变了没有（Switch 自己比较，例如文件大小加 CRC32）。

**马上**返回电脑现在知道的：

```json
{ "fresh": false, "error": null,
  "games": [ { "title": "0100770008dd8000", "key": "mhgu", "name": "MONSTER HUNTER GENERATIONS ULTIMATE",
               "status": "cloudNewer", "pending": false,
               "head": { "id": "01J…", "deviceName": "PC", "createdAt": "…", "…": "…" } } ] }
```

| `status` | 意思 | Switch 该做什么 |
|---|---|---|
| `upload` | 云端还没有这个游戏的存档 | 上传 |
| `synced` | 一样 | 无 |
| `localNewer` | Switch 改过，云端没变 | 上传 |
| `cloudNewer` | 云端有更新的版本，Switch 没改 | 下载 `head.id` |
| `conflict` | 两边都改过 | 让用户选：上传（用 Switch 的）或下载（用云端的） |
| `checking` | 电脑正在向云端查这个游戏 | 等一会儿再问 |
| `unknown` | 电脑查不到（云端连不上，见 `error`） | 显示问题 |

- `fresh: false`：有些游戏的最新版本是电脑在后台重新查的（超过 30 秒就算旧），或者云端游戏列表还从没取过。Switch 大约每 2 秒再问一次（不用重新读存档），直到 `fresh: true`。在那之前给出的状态是按旧信息算的。
- `pending: true`：电脑上还有这个游戏的版本没传到云端（Switch 显示「正在上传」，也可以继续每 2 秒问一次）。状态已经把它算作最新版本。
- `error`：电脑最近一次和云端打交道时的问题（没有就是 `null`）。
- 这次调用也会（在后台，最多 10 分钟一次）把 Switch 登记为一台设备（设备名用配对时的名字，系统 `switch`）。

### `GET /nx/v1/games/{title}/snapshots`

版本列表（新的在前），和电脑上的版本历史相同，加上队列里还没传到云端的（`"pending": true`）。这个请求会当场查一次云端。

```json
{ "key": "mhgu", "headId": "01J…", "snapshots": [ { "id": "01J…", "parent": "01J…", "createdAt": "…", "deviceName": "PC", "trigger": "auto-exit", "fileCount": 3, "totalSize": 1234, "comment": "", "locked": false, "pending": false } ] }
```

### `POST /nx/v1/games/{title}/snapshots?parent=<id>&name=<游戏名>[&backup=1|&force=1]`

请求体是 zip（同样只能含这个游戏的两个路径根，最大 512 MB）。电脑先查一次云端的最新版本（加上队列里的），再把它做成一个新版本（设备名是这台 Switch）放进待上传队列，马上回复，之后在后台传到云端。

- 成功：`{ "id": "01J…", "key": "nx-0100770008dd8000", "pending": true }`，Switch 把 `lastSyncedId` 记成 `id`：这个版本已经由电脑负责送到云端。
- `parent`：Switch 的 `lastSyncedId`。云端（含队列）已经有版本，而 `parent` 不是其中最新的那个（或者省略了）：**`409`**，`{ "error": "…", "head": { 最新版本 } }`，什么都不存。Switch 把这个游戏标成冲突，让用户选。云端还没有版本时 `parent` 随意。
- `force=1`：用户选了「用 Switch 的」：不检查，接在当前最新版本后面（`parent` 由电脑填）。被盖过的版本留在版本历史里。
- `backup=1`：**只作备份**。Switch 上的存档马上要被下载覆盖时，先把它存成一个备份版本（和电脑上「恢复前」的版本一样）：在版本历史里能看到、能恢复，但不会成为最新版本，其他设备也不会拉取它，也不做冲突检查。云端或队列里已经有内容完全相同的版本时不再存，返回那个版本的 id 和 `"existing": true`。备份后 Switch **不**改 `lastSyncedId`。
- `name`：云端还没有这个游戏时用作游戏名（从 NACP 读）。电脑会马上在云端建这个游戏的定义（key `nx-<title id>`，存档规则 `{nxsave-<title id>}/**`、`{nxdevice-<title id>}/**`，身份 `nx:<TITLE ID>`），之后电脑上用 Ryujinx 或 Eden 添加同一个游戏时会自动关联。

### 下载：`POST /nx/v1/downloads` → `GET /nx/v1/jobs/{job}` → `GET /nx/v1/jobs/{job}/file`

1. `POST /nx/v1/downloads`，`{ "title": "0100770008dd8000", "id": "01J…" }` → `{ "job": "<16 位十六进制>" }`。电脑开始在后台取这个版本（还在队列里的直接从电脑上拿）。
2. Switch 每秒问一次 `GET /nx/v1/jobs/{job}`：`{ "state": "working" | "done" | "failed", "seconds": 12, "error": "…" }`（`seconds` 是已经用的时间，Switch 显示出来；用户可以不再等，电脑照样做完）。
3. `done` 后 `GET /nx/v1/jobs/{job}/file`：`application/zip`，只含 `nxsave-<title id>/…` 和 `nxdevice-<title id>/…`。没好时 `409`，不存在 `404`。取好的文件保留 10 分钟。

Switch 写入存档后 `fsdevCommitDevice`，再把 `lastSyncedId` 记成那个版本的 id。

### 冲突怎么处理（Switch 端的做法）

状态是 `conflict`，或者上传时收到 `409`，让用户选：

| 用户选 | Switch 依次做 |
|---|---|
| 用云端的 | ① 上传自己的存档，带 `backup=1`（`parent` 填 `lastSyncedId`）② 下载 `head.id` ③ `lastSyncedId` = `head.id` |
| 用 Switch 的 | 上传，带 `force=1`，`lastSyncedId` = 返回的 `id` |

恢复旧版本 = 下载那个版本（先做一次 `backup=1` 上传）+ `force=1` 上传，让它成为最新版本。

「云端较新」时如果 Switch 记录的 `changed` 不可靠（例如存档被别的工具改过），也可以在下载前先做一次 `backup=1` 上传：内容没变时什么都不会多存。

## 存档库

Switch 和电脑上 Ryujinx、Eden 的同一个游戏（同一个 title id）共用同一个云端 key，所以 Switch 上传的存档就是模拟器下次启动前会拉取的存档，反过来也一样。电脑上的模拟器同步这个游戏时，也会先把队列里 Switch 的版本传上去。电脑上没有这个游戏的模拟器条目时，它在游戏库里是「Switch」分组里的一张灰色卡。

同名但存档不通用的其他平台版本（例如 Cemu 里的 Wii U 版）**不会**合成一个游戏（见 [plan-phase5.md](plan-phase5.md) §5 的「存档格式」）。
