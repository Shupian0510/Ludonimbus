# Ludonimbus 多平台方案（草案 v0.1 · 2026-11-23）

> **背景**：路线图上的功能做完后再开始。优先级：Steam Deck → Linux（Proton、Bazzite）→ macOS → Switch 1 代的 Atmosphère 自制系统客户端；Switch 实机和电脑模拟器存档互通是进阶功能。
>
> **测试设备**：除了现在这台 Windows 电脑，你手里只有 Switch，没有 Deck、Linux 机器和 Mac。每个平台怎么测见第 6 节，这也影响了第 7 节的顺序建议。
>
> | 方向 | 一句话 | 能不能实测 |
> |---|---|---|
> | **P. 准备** | CI 加 Linux / macOS 检查，平台相关代码收拢，云端格式写成规范 | 不需要设备 |
> | **L. Linux 基础移植** | 找 Steam、包装器、进程、路径、模拟器、分发 | Bazzite 装在外接 SSD 上 |
> | **D. 游戏模式** | Steam Deck、Bazzite 掌机版：后台服务 + Decky 插件 | Bazzite 掌机版（bazzite-deck） |
> | **M. macOS** | 同 L，加上苹果签名 | 只有 CI 和社区测试 |
> | **S. Switch** | S0 模拟器之间互通 → S1 经电脑同步 → S2 直连云盘 → S3 自动同步 | 能，你自己完整实测 |

---

## 0. P：现在就能做的准备（不占路线图）

- **CI**：加 `ubuntu-latest`、`macos-latest` 两个任务，跑 `cargo check` 和 `cargo test`（core、cli、runner；Tauri 应用在 Linux 上要装 webkit2gtk 等依赖）。目的是不让新代码继续加深对 Windows 的依赖。
- **平台相关代码收进一个模块**：`crates/core/src/platform/`（`windows.rs`、`linux.rs`、`macos.rs`），统一放：找 Steam、包装器怎么挂进快捷方式、进程追踪、文件占用检测、默认路径根、包装器和附带程序的文件名。移植时只补各平台的实现。
- **云端格式写成规范**：`docs/cloud-format.md`，写清 `games/<key>/game.json`、`games/<key>/snapshots/`、快照 zip（`files/<root>/<相对路径>` + `snapshot.json`，见 [snapshot.rs](../crates/core/src/snapshot.rs)）和 `files()` 下的通用文件。Switch 客户端是 C++ 写的，两边按同一份规范实现；`snapshot.json` 加 `formatVersion` 字段，再准备一组固定的样例文件，Rust 和 C++ 两边的测试都跑。

---

## 1. L：Linux 基础移植

### 1.1 现在绑定 Windows 的地方

| 位置 | 现在 | Linux 上 |
|---|---|---|
| 找 Steam [steam.rs:24](../crates/core/src/steam.rs) | 读注册表 `SteamPath` | 依次查 `~/.local/share/Steam`、`~/.steam/steam`（符号链接）、Flatpak 版 `~/.var/app/com.valvesoftware.Steam/.local/share/Steam` |
| 包装器怎么挂 [steam.rs:165](../crates/core/src/steam.rs) | 把快捷方式的 `Exe` 换成 `ludonimbus-run.exe` | 改写启动选项，见 1.2 |
| 进程追踪 [process.rs:61](../crates/core/src/process.rs) | Job Object 跟踪整棵进程树 | `prctl(PR_SET_CHILD_SUBREAPER)`：启动器退出后，它派生的游戏进程挂回包装器，包装器等到全部退出 |
| 进程名 | Windows API | `/proc/<pid>/comm` 只有 15 个字符，Proton 里的游戏还要看 `/proc/<pid>/cmdline` 里的 `.exe` 名 |
| 文件占用检测 [process.rs:220](../crates/core/src/process.rs) | `ERROR_SHARING_VIOLATION` | Linux 没有强制锁，跳过 |
| 路径根 [spec.rs:97](../crates/core/src/spec.rs) | `{appdata}` 等指向 Windows 文件夹 | 分原生程序和 Proton 两种，见 1.3 |
| 程序文件名 [paths.rs](../crates/core/src/paths.rs) | 写死 `ludonimbus-run.exe` | 按平台取名 |
| 模拟器识别 [emulators.yaml](../adapters/emulators.yaml) | 按 `.exe` 名匹配 | 加 Linux 程序名、AppImage、Flatpak，见 1.4 |

Ludusavi 和 OpenList 都有 Linux / macOS 版本，按平台打包对应的附带程序即可。托盘在 KDE（SteamOS 和 Bazzite 的桌面模式都是 KDE）上可以直接用。

### 1.2 包装器：改写启动选项，不换 Exe

快捷方式如果设置了用 Proton 运行，Steam 会用 Proton 去执行 `Exe`；把 `Exe` 换成原生的 `ludonimbus-run` 就会失败。Linux 上改成写启动选项：

```text
"<包装器路径>" --game <key> -- %command%
```

Steam 会把 `%command%` 换成完整的启动命令（用 Proton 时是 Proton 加游戏 exe），包装器先下载存档，再执行这条命令，结束后上传。和 `gamemoderun %command%` 一个套路，原生程序和 Proton 都适用。

- 用户原来的启动选项里已经有 `%command%`（例如 `MANGOHUD=1 gamemoderun %command%`）时，包装器紧挨着放在 `%command%` 前面，环境变量和前缀工具留在它前面，原样保留其余部分；关闭时还原。旧版本写的“包装器在最外层”的形式照样认得、还原得了。
- 判断一个快捷方式有没有被包装，从查 `Exe` 改成查启动选项。
- 包装器要放在固定位置的独立文件里（例如 `~/.local/share/Ludonimbus/bin/ludonimbus-run`），不能放在 AppImage 里面：AppImage 每次启动都要挂载，而且用户可能会挪动它，启动选项里的路径就失效了。

### 1.3 Windows 游戏在 Proton 里的存档路径

同一个游戏，在 Windows 电脑上是原生程序，在 Linux 上可能通过 Proton 运行。每台设备各自判断：快捷方式的 exe 是 `.exe`，或 Steam 的 `config/config.vdf` 里 `CompatToolMapping` 给这个 appid 指定了兼容工具，就按 Proton 处理。

Proton 前缀在 Steam 主库的 `steamapps/compatdata/<appid>/pfx`。非 Steam 快捷方式的 appid 每台机器不一样，按本机的算，不影响同步：快照里的文件按逻辑根存放，和设备无关。

| 路径根 | Proton 里的位置（`pfx/drive_c/users/steamuser/` 下） |
|---|---|
| `{home}` | `.` |
| `{appdata}` | `AppData/Roaming` |
| `{localappdata}` | `AppData/Local` |
| `{documents}` | `Documents`（老前缀是 `My Documents`） |
| `{install}` | 直接用 Linux 路径（Proton 的 Z: 盘就是根目录） |

要处理的细节：

- **大小写**：快照里的路径比较已经不区分大小写（[snapshot.rs](../crates/core/src/snapshot.rs) 里都转成小写比较）。写文件时还要沿用目标文件夹里已有文件的大小写，不然会同时出现 `SAVE.DAT` 和 `save.dat`，游戏读到哪个不确定。
- **文件名里的特殊字符**：Linux 原生程序的存档文件名里可能有 `:`、`?` 这类 Windows 不允许的字符，恢复到 Windows 时跳过并提示。Proton 游戏不会有这个问题。
- **前缀还没建好**：游戏在这台设备上从没用 Proton 运行过时，前缀不存在。先恢复存档会不会干扰 Proton 建前缀，要实测（见第 9 节）；不行就提示用户先启动一次游戏。

### 1.4 Linux 上的模拟器

- **安装方式**：AppImage（Ryujinx、Eden、Citron 等提供）、Flatpak（Flathub 上的 Dolphin、PCSX2、RetroArch 等）、发行版的包。
- **存档位置**：原生程序一般在 `~/.config/<模拟器>` 或 `~/.local/share/<模拟器>`；Flatpak 在 `~/.var/app/<应用 id>/` 下面。模拟器规则要支持按平台写候选位置，新增占位符：
  - `{config}`：Linux 上是 `XDG_CONFIG_HOME`，macOS 上是 `~/Library/Application Support`
  - `{data}`：Linux 上是 `XDG_DATA_HOME`
  - `{flatpak-<应用 id>}`：`~/.var/app/<应用 id>`
  - `roots` 里可以写 `windows:` / `linux:` / `macos:` 三组候选位置
- **识别 Flatpak 启动的游戏**：快捷方式的 exe 是 `flatpak`，参数里是 `run <应用 id> <游戏文件>`，按应用 id 认模拟器。
- **EmuDeck**（Deck 上最常见的模拟器配置工具）：它把存档挪到 `~/Emulation/saves/` 下，再用符号链接接回模拟器原来的位置。
  - 恢复时写进链接指向的文件夹，**不能删掉链接再重建**，否则会弄坏 EmuDeck 的目录结构。
  - 扫描存档时跟随符号链接，但要防止链接成环。
  - 它的快捷方式由 Steam ROM Manager 生成，exe 是启动脚本，参数是游戏文件路径；按脚本名认模拟器。具体位置以实物为准。
- Switch 模拟器在 Linux 上的目录结构和 Windows 相同，[switch.rs](../crates/core/src/switch.rs) 的逻辑可以直接用。

### 1.5 分发

- **AppImage + 安装脚本**：SteamOS 和 Bazzite 的系统分区只读，只能装到 home 目录。脚本负责：把 AppImage 放到固定位置、装好独立的包装器、注册开机启动（XDG autostart 或 systemd 用户服务）、添加桌面图标。
- **自动更新**：Tauri 的更新器支持 AppImage；包装器由应用跟着一起更新。
- **普通发行版**：顺带出 Tauri 生成的 `.deb` / `.rpm`。Bazzite 是不可变系统，不推荐往系统里装 rpm，用 AppImage。
- **不做 Flatpak**：沙箱要访问 Steam 文件夹、读别的 Flatpak 应用的数据，包装器还要被 Steam 当作启动命令调用，限制太多。

---

## 2. D：游戏模式（Steam Deck、Bazzite 掌机版）

### 2.1 后台服务

游戏模式里没有托盘，也不能弹窗口（会盖在游戏上面）。

- 需要一个**不带界面的后台程序** `ludonimbus-agent`：本地 API、进程监视、截图上传。现在 [agent.rs](../app/src-tauri/src/agent.rs) 和 `watcher.rs` 都依赖 Tauri 的 `AppHandle`，要先挪进一个独立的 crate（例如 `crates/agent`），Tauri 应用和后台程序共用。
- 用 `systemd --user` 服务运行，登录后自动启动。游戏模式开机自动登录，服务会跟着起来。
- Steam 菜单里的「退出游戏」会给整个进程树发 SIGTERM，包装器也在里面，可能来不及上传。包装器要接住 SIGTERM，等游戏退出后尽快上传；被强制结束时，后台服务按 `running/<key>.json` 补传，和 Windows 上包装器被强制结束时的处理相同。
- Deck 经常在游戏中途休眠。醒来后网络要过几秒才恢复，上传和下载要能重试。

### 2.2 Decky 插件

游戏模式（大屏幕界面）的插件标准是 Decky Loader，不是 Millennium（Millennium 改的是桌面版 Steam 客户端）。Bazzite 用 `ujust setup-decky` 安装 Decky。

- 前端也是 TSX，用 `@decky/ui` 组件；后端是 Python，只负责读 `agent.json` 并转发请求到本地 API（API 2 已经有状态、立即同步、版本列表、恢复、游戏时间）。
- [plugin-millennium/frontend](../plugin-millennium/frontend) 里的逻辑大部分能搬过去，界面要按手柄操作重做：
  - 快捷菜单（QAM）面板：当前游戏的云存档状态、立即同步、版本列表和恢复
  - 游戏页上的状态标记
  - 同步结果、冲突的通知
- **冲突处理只用手柄也要能完成**。
- Linux 桌面模式下，如果 Millennium 能用，现有插件直接复用（待验证）。

### 2.3 首次设置

- 主要在桌面模式里用 Ludonimbus 的窗口完成：登录云盘、添加游戏。
- 游戏模式里登录云盘：浏览器跳转很别扭。Google Drive 可以用**设备码登录**（屏幕上显示网址和代码，在手机上完成），这和 Switch 直连云盘（4.4）是同一套，做一次两边都能用。

---

## 3. M：macOS

- **Steam**：`~/Library/Application Support/Steam`，快捷方式文件格式相同。启动选项是否支持 `%command%` 要实测；不支持就沿用 Windows 的换 Exe 做法。
- **路径根**：`{appdata}` → `~/Library/Application Support`。读 `~/Documents` 会弹系统权限框，在引导里提前说明。
- **进程追踪**：macOS 没有 subreaper，用进程组加定时扫描。
- **程序是 `.app` 包**：快捷方式可能指向 `.app` 或里面的 `Contents/MacOS/<程序>`，两种都要认；模拟器也是（Ryujinx、Dolphin、PCSX2、RetroArch 都有 mac 版）。
- **签名**：SignPath 只管 Windows。mac 要苹果开发者账号（99 美元/年）做 Developer ID 签名和公证，包装器也要签。不签的话，用户要去「系统设置 → 隐私与安全性」点「仍要打开」。
- **CrossOver**：瓶子里的 Windows 游戏和 Proton 前缀是同一个思路，优先级低。
- 没有设备：CI 跑编译和测试，正式版标「实验性」，靠社区测试。

---

## 4. S：Switch

### 4.1 用户看到的流程

在 Switch 上从相册打开 Ludonimbus（`.nro`），看到有存档的游戏列表和各自的状态：已同步 / 本机较新 / 云端较新 / 冲突。可以单个上传、下载、全部同步，也能看版本列表、恢复旧版本。电脑上用 Ryujinx 或 Eden 玩同一个游戏，回到 Switch 打开 Ludonimbus 同步一下，就能接着玩。

### 4.2 S0：先让模拟器之间互通（只动 Windows，现在就能做）

现在 [switch.rs](../crates/core/src/switch.rs) 已经把 Ryujinx 的存档统一成 `{nxsave-<title id>}`（当前用户的存档）和 `{nxdevice-<title id>}`（设备存档），换电脑也对得上。但：

- 游戏定义绑定 Ryujinx；
- yuzu 系（Eden、Citron、Sudachi、Suyu、yuzu）是连用户档案整个目录一起同步（[emulators.yaml](../adapters/emulators.yaml) 开头的说明）。

S0 要做的：

- **游戏身份改成 title id**：`game.json` 记录 Switch 的 title id，和用哪个模拟器无关。在另一台设备上添加同一个 Switch 游戏时，按 title id 找云端已有的游戏并自动关联，不靠名字。
- **`{nxsave-…}` / `{nxdevice-…}` 由每台设备上的后端解析**：
  - Ryujinx：现有逻辑（查存档索引，没有就新建）
  - yuzu 系：`nand/user/save/0000000000000000/<用户 ID>/<title id>/`；设备存档的用户 ID 是全零。没有索引，比 Ryujinx 简单。用户 ID 在路径里的写法（字节顺序）要对照真实文件夹确认
  - Switch 实机：S1 实现
- 一台电脑同时装了几个模拟器时，按快捷方式实际启动的那个。
- 已经按 Ryujinx 添加的游戏：`game.json` 补上 title id，云端的 key 不变。

做完后 Ryujinx 和 Eden 之间就能互通，对现在的 Windows 用户也有用；你在这台电脑上就能测。

### 4.3 S1：Switch 客户端，经电脑同步（局域网）

**仓库和技术**：单独开仓库（例如 `Ludonimbus-NX`，MIT），C++ + libnx（devkitPro 的 devkitA64 工具链）。不要把 Rust 的 core 搬上去：Rust 的 Switch 目标是 tier 3、没有标准库，成本远高于用 C++ 重写这层薄客户端。界面可以用 borealis（仿系统界面，支持手柄和触屏，协议待确认）。

**为什么先经电脑**：电脑上的 Ludonimbus 已经会用所有云盘（Google Drive、WebDAV、S3、百度、夸克）和冲突处理。Switch 只跟电脑说话，C++ 这边只要实现「读写存档 + 一个局域网接口」，体积小，出错的地方少。代价是同步时电脑要开着 Ludonimbus；Switch 存档互通的另一端本来就是电脑上的模拟器，这个代价不大。

**电脑这边**：

- 本地 API 现在只听 `127.0.0.1`，令牌每次启动都换。新增一个局域网接口，**默认关闭**，在设置里打开「允许 Switch 连接」。
- **配对**：电脑上显示 6 位配对码，在 Switch 上输入，换到一个长期有效的设备令牌，两边各自保存。连接用 TLS，Switch 在配对时记下电脑证书的指纹，以后只认这个证书（局域网里的明文令牌能被同网段的设备截获）。
- **发现**：局域网广播，找不到时手动输入 IP。
- 电脑把 Switch 当作另一台设备：它的设备 id 存在 Switch 的 SD 卡上，电脑代它创建快照，设备名显示「Switch」。冲突判断规则和电脑之间相同：Switch 记住每个游戏、每个用户最后同步的快照 id，状态由电脑计算。
- 接口（草案）：

```text
GET  /nx/v1/games                        Switch 上报自己有存档的 title id 和存档摘要，返回每个游戏的状态
GET  /nx/v1/games/{title}/snapshots      版本列表
GET  /nx/v1/games/{title}/snapshots/{id} 下载某个版本的存档（zip）
POST /nx/v1/games/{title}/snapshots      上传（zip + 用户、游戏版本、上一个快照 id），电脑建快照并传到云端
```

**Switch 这边（libnx）**：

- **用户**：`accountListAllUsers` 列出所有用户，用系统的选用户界面（`pselShowUserSelector`）选一个，记在设置里。
- **列出存档**：`fsOpenSaveDataInfoReader`，只看账号存档和设备存档，跳过 BCAT、缓存、临时、系统存档。
- **读写**：按 application_id + 用户 + 类型调用 `fsOpenSaveDataFileSystem` 挂载，用普通文件接口读写。写完调 `fsdevCommitDevice` 提交；提交之前出错，原存档不变。
- **新建、扩容**：Switch 上还没有这个游戏的存档时，从 NACP（`nsGetApplicationControlData`）读存档大小和日志大小，再 `fsCreateSaveDataFileSystem`；要写的数据超出容量时 `fsExtendSaveDataFileSystem`。
- 游戏名、图标、版本号也从 NACP 和内容元数据里读。
- **不需要 `prod.keys`**：系统直接给出解密后的文件。
- zip 用 devkitPro 自带的 zlib，边读边写，不整个放进内存。从相册以小程序模式启动时内存比较紧，大存档的游戏要按住 R 打开游戏，以完整模式进入。

**用户对应**：一个 Switch 用户对应云端的 `nxsave-<title id>`（就是模拟器那边的当前用户档案）。Switch 上多个用户各自同步，等有人需要再做。

### 4.4 S2：Switch 直连云盘（电脑不开也能同步）

- **WebDAV、S3**：libcurl + mbedtls（devkitPro 里有），S3 的签名（SigV4）要用 C++ 实现。能覆盖 NAS 和自建存储。
- **夸克**：经 OpenList 接入，而 OpenList 跑在电脑的 `127.0.0.1` 上，所以夸克只能走 S1 的局域网方式。
- **Google Drive**：设备码登录。已核实（2026-11-23，Google 文档）：设备码流程支持 `drive.file` 和 `drive.appdata`，客户端类型要选「电视和受限输入设备」，每次登录都会给 refresh token，Google 对每个用户的 refresh token 数量有上限。待验证：同一个 Cloud 项目里新建的这类客户端，能不能看到桌面客户端用 `drive.file` 创建的文件。
- **百度网盘**：开放平台有设备码授权模式，能不能用在这里待验证。
- 直连以后，快照链、冲突判断这些逻辑也要在 C++ 里实现一遍（清理旧版本留给电脑做），所以第 0 节的格式规范和样例文件是前提。

### 4.5 S3：自动同步（以后）

做一个系统模块（sysmodule）监视游戏启动和退出：退出后自动上传，启动时如果云端较新就提醒。系统模块的内存很紧，也没法挡住游戏启动去先下载，复杂度高，放到最后。

### 4.6 风险和坑

- **游戏版本**：模拟器上 1.2 版游戏的存档，恢复到装着 1.0 版的 Switch，可能读不了甚至损坏。`snapshot.json` 记录游戏版本；恢复版本更新的存档之前先警告。Switch 端从内容元数据读版本；模拟器那边怎么拿到已安装的更新版本，待验证。
- **存档容量**：有些游戏的存档会超过默认容量，见 4.3 的新建和扩容。
- **存档里的用户信息**：存档文件本身是明文，大多数游戏不检查用户和主机。个别游戏可能会检查，实测时留意。
- **封机风险**：客户端不连任天堂的服务器；建议在 emuMMC（虚拟系统）上使用。
- **对外措辞**：一律说「homebrew / Atmosphère 自制系统」，README、网站、发布说明里不出现「破解」之类的字眼；不分发任何密钥；名字和图标不用任天堂的商标。单独开仓库，万一收到 DMCA 不会连累主仓库。
- **参考实现**：JKSV、Checkpoint 这类存档管理器已经存在多年，是可参考的先例，但都是 GPL-3.0（2026-11-23 查过），只能看思路，代码不能搬进 MIT 项目。
- **分发**：GitHub Releases 发 `.nro`，以后可以上 Homebrew App Store。

---

## 5. 各平台都要改的公共部分

- **路径根和模拟器规则**按平台解析（1.3、1.4），Windows 上的行为不变。
- **快照格式**：加 `formatVersion`、游戏版本（Switch 用）；路径一律用 `/`。
- **设备名**：Deck、Switch 这类设备要有好认的默认名。
- **文档和网站**：每个平台加安装页；Switch 的页面放在 Switch 仓库里。

---

## 6. 测试

| 平台 | 自动测试 | 实机测试 |
|---|---|---|
| Linux 桌面 | CI（ubuntu） | Bazzite 装在一块外接或闲置的 SSD 上，和 Windows 双系统；同时能测 Proton |
| 游戏模式 | — | Bazzite 掌机版（bazzite-deck）在台式机上就是游戏模式 + Decky；和 Deck 真机的差别在休眠、触屏、性能 |
| Steam Deck 真机 | — | 招募社区测试者（GitHub Discussions） |
| macOS | CI（macos，Apple Silicon） | 社区测试，正式版标「实验性」 |
| Switch | Rust 和 C++ 跑同一组格式样例 | 你自己：Switch + 这台电脑上的 Ryujinx / Eden |

虚拟机（Hyper-V）里可以测不涉及显卡的流程（找 Steam、改快捷方式、路径解析），但游戏模式要真显卡，虚拟机跑不了，所以实机测试用 Bazzite 双系统。

---

## 7. 顺序（默认做法，有不同意见可以改）

1. **P 准备**：随时做，可以和路线图并行。
2. **S0 Switch 模拟器互通**：你的顺序里 Switch 在最后。但 S0 只动 Windows 上的模拟器部分，你能自己测，对现在的用户也有用，建议提前做；S1 以后保持最后。
3. **L Linux 基础移植**：用 Bazzite 双系统测。
4. **D 游戏模式**：先做后台服务，再做 Decky 插件。
5. **M macOS**：以实验版发布。
6. **S1 → S2 → S3**。

---

## 8. 验收

- **L**：在 Bazzite 桌面模式里，一个经 Proton 运行的 Windows 游戏和一个 Linux 原生模拟器，都能和 Windows 电脑双向同步；恢复进 EmuDeck 的符号链接文件夹后，链接还在。
- **D**：游戏模式里从 Steam 启动游戏，启动前下载、退出后上传；只用手柄能在 Decky 面板里查看状态、立即同步、恢复旧版本、处理冲突；用 Steam 菜单的「退出游戏」也能上传。
- **M**：CI 通过；至少一位社区测试者完成一个模拟器游戏的来回同步。
- **S0**：同一个 Switch 游戏先在 Ryujinx 上玩，再在 Eden 上接着玩（同一台电脑或两台），反过来也行。
- **S1**：Switch 和电脑上的 Ryujinx / Eden 双向同步；两边都改过时报冲突；恢复版本更新的存档前有警告。
- **S2**：电脑关着，Switch 通过 Google Drive 或 WebDAV 和另一台设备同步。

---

## 9. 待验证

- 同一个 Google Cloud 项目里的「电视和受限输入设备」客户端，能不能看到桌面客户端用 `drive.file` 创建的文件
- Proton 前缀还不存在时先恢复存档，会不会干扰 Proton 建前缀
- Steam 的「退出游戏」从 SIGTERM 到强制结束，给包装器留多少时间
- macOS 上非 Steam 快捷方式的启动选项是否支持 `%command%`
- Millennium 在 Linux 桌面版 Steam 上能否运行（能的话现有插件直接复用）
- yuzu 系存档路径里用户 ID 的写法（字节顺序）
- 模拟器那边怎么拿到游戏已安装的更新版本号
- 百度网盘的设备码授权能不能用在 Switch 上
- EmuDeck 的启动脚本位置和存档链接结构（以实物为准）
- borealis 的协议是否适合 MIT 项目

---

## 10. 需要你做的事

- **Linux 测试环境**：准备一块外接或闲置的 SSD 装 Bazzite（桌面版和掌机版可以切换）。安装时只选这块盘，别动 Windows 所在的盘。
- **Switch 测试**：用 emuMMC，先备份 SD 卡；告诉我电脑上用 Ryujinx 还是 Eden，还是都用。
- **macOS**：决定要不要买苹果开发者账号（99 美元/年）；不买就以未签名的实验版发布。
- **S2 时**：在 Google Cloud 里新建一个「电视和受限输入设备」类型的客户端。
- **测试者**：选一个招募 Deck / Mac 测试者的地方（GitHub Discussions 或以后的 Discord）。
