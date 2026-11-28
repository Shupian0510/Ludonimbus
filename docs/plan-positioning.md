# 定位调整：个人云端游戏数据中心（草案 v0.1 · 2026-11-27）

> **来源**：你在 `1.0.0-rc14测试反馈.md`（Obsidian 库）里写的「更清晰的方针、用户体验定位」。
>
> **你的方向，一句话**：Ludonimbus 不再是「自动帮用户把存档无缝同步好」的软件，而是**个人云端游戏数据中心**。存档、时长、成就、截图都在你自己的云盘里，随时取出来用在新设备、重装的游戏、另一个平台的同一个游戏上。**在 Steam 里玩自动同步**（Windows 上把游戏加入 Steam 就有类原生云存档）；**别的平台、别的版本、模拟器之间的转换由用户在需要时发起**，界面讲清楚在做什么。

本文：我的看法（第 1 节）、新规则（第 2 节）、和 1.0 现有设计哪里冲突（第 3 节）、九个调研问题的结果（第 4 节）、要改的东西和要你定的事（第 5、6 节）。

---

## 1. 我的看法：同意，而且改动不大

1. **自动同步已经是很多工具的标配，转换和「数据中心」没人做。** 第 4.5 节列了十几个工具：OpenSave、Hoard、EmuSync、EmuDeck Sync、decky-ludusavi、Decky Cloud Save，RetroDECK 也在接 Ludusavi。它们都做「同一个游戏在几台设备间自动同步」，而且免费。没有一个做跨平台转换（Xbox ↔ Steam、Switch ↔ PC），也没有一个把时长、成就、截图和存档放进同一个游戏库，更没有百度、夸克这些国内网盘。新定位押在只有我们有的东西上。
2. **静默写入的风险是真的。** rc14 里电车炫客的 Xbox 版和 Steam 版关联以后，Steam 立刻显示了 Ludonimbus 在管的云存档，这正是「自动共用」的副作用。Ludusavi 数据库没覆盖的路径、按标题找存档时混进别的商店版本的文件（plan-v2 §6「哪些文件是 Xbox 版的」）都是同一类问题：猜错了，自动写入就会覆盖别人的存档。
3. **积木都在，主要改默认值和「谁可以自动写」。** 「发到…」「设为当前」、写前保留、撤销、`not_into_containers`、`backup_only`、只在云端的游戏页、Switch 端的「版本 → 恢复」都已经做好。要改的是：哪些情况下允许不经用户选择就写进本机。不需要重写。

---

## 2. 新规则

**上传总是自动的，写进游戏只在 Steam 里自动。**

上传只是在云端多一个版本，不覆盖任何东西，所以任何时候都可以自动做。写进本机的存档会覆盖东西，所以只在用户期待它发生的地方自动：从 Steam 启动的游戏，和 Steam 云一样。

| 场景 | 上传（备份） | 写进本机 |
|---|---|---|
| 从 Steam 启动、经过包装器（加入 Steam 的游戏、「Ludonimbus 管」的 Steam 游戏） | 退出后自动 | **启动前自动**，只取同一条线（同一个版本）的最新版本 |
| 「Ludonimbus 管」但不用包装器的 Steam 游戏 | 自动 | 游戏没在运行时自动（现状） |
| Steam 云在管的游戏（「只备份」） | 自动 | 从不（现状） |
| 不经 Steam 启动：模拟器直接开、Xbox 应用、Epic、Switch 实机 | 退出后自动（托盘程序）、Switch 上按一下 | **从不**。显示「云端有更新」，按钮「取回」 |
| 另一个平台、另一个版本、另一个模拟器的存档 | — | **从不**。游戏页提示「Xbox 版有更新的进度」，按钮「发到…」，有规则时自动转换 |
| 游戏删了又装回来、换了新设备 | — | 从 Steam 启动的：启动前自动。其余：游戏页「取回」 |
| 本机的存档全都不见了（删了，或 SD 卡没插；2026-11-28） | **从不**把空的当成最新版本上传 | 从 Steam 启动的：启动前自动。其余（玩完后、后台）：从不，显示「本机存档不见了」，按钮「取回」 |
| 暂停同步时（Deck 面板；2026-11-28） | 不传，先排在本机队列里，继续后上传 | 从不；包装器直接启动游戏 |
| 应用里的「全部同步」 | 全部上传 | 先列出会被覆盖的游戏（只列能直接取回的同版本存档），确认后取回；跨平台的不在里面 |

- 「取回」和「发到…」就是现有的「设为当前」：写前保留现在的存档，写完核对，可以撤销（plan-v2 §6 安全网、§9）。
- 「云端有更新」不弹窗，是游戏卡上的状态点和游戏页的一行（UI 规则：提示放状态，确认框只给覆盖和删除）。
- 「共用进度」（在 Xbox 版玩完，Steam 版启动时自动接上）保留成**高级选项**：版本条菜单里「和 Xbox 版共用进度」，打开以后就是现在的行为。代码已经写好、测过，留着给想要它的人；默认关。

---

## 3. 和 1.0 现有设计的冲突

| plan-v2 里的 | 现在的行为 | 新规则下 |
|---|---|---|
| 决定 1：格式相同的版本默认自动共用 | Steam / Epic / GOG / Xbox 的同一个游戏「问一次」后共用；同一规则的平台对直接共用 | 只有**同一个版本**（同一平台、同一种安装）在不同设备间共用。不同商店、不同平台：同一张卡、各自一条线 |
| 决定 7：几个平台默认共用一个当前进度 | 默认共用 | 默认各自；共用是高级选项（第 2 节最后一条） |
| §6 三档里的「直接共用」 | 包括 Switch 实机和 Switch 模拟器、标了「已实测」的规则平台对 | Switch 实机 ↔ 模拟器、模拟器 ↔ 模拟器都是「发到…」。`nxsave-<title id>` 照样把它们放在同一张卡上 |
| §6「问一次」 | 推断同格式就加入，第一次写入时问一次，之后不再问 | 不加入。推断出的绑定（`join::suggestion`）用来给「发到…」算文件对应关系，确认框里预览 |
| `join::auto`（别的设备确认过的绑定直接用） | 自动加入 | 只在用户打开了「共用进度」时 |
| 托盘程序在游戏退出后的同步（`Trigger::Passive`） | `Resolution::Auto`，本机没改、云端更新时会取回 | 只上传；云端更新时显示「云端有更新」 |
| Switch 端 A「同步」 | 哪边新就传哪边，包括下载 | A 只备份；下载只从 X「取回」，先确认（第 4.6 节） |
| §10.3「当前」 | 一条共用线上的当前进度 | 每个版本自己的当前进度；游戏页列出各平台最新的一份 |

**代码里要动的地方**：

- 不经包装器启动的游戏退出后的同步：`app/src-tauri/src/watcher.rs:155`、`crates/core/src/catchup.rs:174`（`Trigger::Passive`），改成只上传。
- 自动加入：`crates/core/src/join.rs:520` 的 `join::auto`（在 `sync/run.rs:112` 调用），改成只在打开「共用进度」时；`crates/core/src/cloud/library.rs:316` 的 `line_to_join`（「直接共用」）收窄到同一个版本。
- 规则写进 `sync/run.rs`，和 `backup_only`、`not_into_containers` 放在一起，按触发方式判断：托盘和追补（`Trigger::Passive`）在不经 Steam 启动的游戏上不写本机。应用里的「立即同步」（`commands/saves.rs:42`）和插件里点的同步（`localapi/routes.rs:391`）是用户点的，同一个版本的更新照取，这就是「取回」。
- 包装器（`crates/runner`）和 `steamcloud.rs` 不变。`join::accept`、`join::leave` 之后的同步是用户点的，不变。

**已经在云端共用一条线的游戏**（rc 测试里关联过的）：不自动拆开。版本条菜单里本来就有「和其他版本分开存」；新装的 1.0 默认不再并线。1.0 之前没有正式用户用过共用（0.7 没发布），所以不需要迁移。

**现在的做法**（2026-11-27，写入方针）

- **只上传的同步**：`sync/run.rs` 的 `only_uploads` + `upload_only`，和 `backup_only`、`not_into_containers` 放在一起。`Trigger::Passive`（托盘在游戏退出后、包装器被结束后的追补、它们欠下的同步 `sync::owe_passive`）在云端有同一条线的更新版本时不写本机，游戏状态是「云端较新」；两边都改了照旧是冲突，不写。例外：不用包装器的「Ludonimbus 管」Steam 游戏（后台本来就替它取，第 8.3 节）。包装器启动前和退出后、「立即同步」、插件里点的同步照取（就是「取回」）。`sync::upload` 是「全部同步」用的只上传版本；`sync::pulls_by_itself` 说一个游戏会不会自己取到更新（加入 Steam 并且经过包装器的、「Ludonimbus 管」的 Steam 游戏）。
- **通知**：`app/src-tauri/src/notify.rs`。不另外请求云端，看已有的检查（游戏库启动时、游戏结束后、每 10 分钟的 `check_game`）。窗口隐藏或最小化、游戏不会自己取到、状态是「云端较新」时，每个游戏的每个云端版本弹一次（Windows 是操作中心里的通知，Linux 用 `notify-send`），几秒内超过 3 个合成一条。点了打开这个游戏的页面，操作栏的主按钮这时是「取回」。设置里「云端有更新时通知」可以关。
- **启动时提醒、全部同步**：游戏库在第一次检查完所有游戏后，有需要选择的游戏（冲突；或「云端较新」且不会自己取到的）时显示一行「N 个游戏和云端不一样」+「处理」，对话框里每个游戏「取回」，「更多」里「用本机的」「这台设备单独存」，只是旧版本的游戏有「全部取回」。顶部「全部同步」（和分组的「全部同步」）先逐个只上传，再对需要选择的游戏打开同一个对话框。只看游戏自己的线，不含别的平台、别的版本。
- **加入存档线**：存档线记下开这条线的安装是哪一种（`Line.kind`：`steam`、`epic`、`gog`、`xbox`、`pc`、`switch`、`emu:<模拟器>`、`recomp:<规则>`，`upgrade::line_kind`）。`library::line_to_join` 和合并重复记录（`same_saves`）只把同一种安装、同样文件的版本并成一条线；没有 `kind` 的旧线照旧只看 `def`。Switch 游戏添加时接过云端 key（`games::adopt_cloud_key`）也只接同一个模拟器（或都是 Switch 本机）的。别的版本（另一家商店、Switch 本机和模拟器、两个模拟器）在同一张卡上各自一条线。
- **共用进度**：版本条「本机」菜单里「和 X 版共用进度」（带「?」），只在 `join::suggestion` 说合得上时出现（`share_offer`），确认后就是 `join::accept`。同样文件、不同种安装的线现在也会被建议。确认的绑定按安装的种类记在线上（`LineBind.kind`），别的设备上同一种安装的版本照着自动加入（`join::auto`）；别的种类不会。游戏页下面那一行「问一次」不再显示。

---

## 4. 调研结果

### 4.1 Linux → Steam Deck：Decky 能不能替代 Millennium，启动和退出能不能自动同步

**结论：能，而且已经这样做了。** 0.7 做的 Linux 版（plan-phase5 §3、§4）就是这个结构：

- **自动同步不靠任何插件。** 包装器写进 Steam 的启动选项：`"<runner>" --game <key> -- %command%`。Game Mode 里从 Steam 启动的游戏照样经过它：启动前取、退出后传，Proton 照常工作。Deck 上后台是 `ludonimbus-agent`（systemd 用户服务）。
- **Decky 只负责在 Game Mode 里显示**状态、版本、恢复（`plugin-decky/`），对应 Windows 上 Millennium 插件的位置。SteamOS 上 Decky 是事实标准（插件商店），Millennium 面向桌面版 Steam 客户端。
- Millennium 自己也支持 Linux（通过 `libXtst.so.6` 代理注入），桌面 Linux 上可以照 Windows 的方式用。**风险**：Steam 2026-11-11 的更新把 Linux 客户端改成 64 位、跑在 Steam Runtime 容器里，注入类工具可能受影响，要等社区测试。我们的自动同步不受影响（启动选项是 Steam 自己的功能）。
- **为什么不用 Decky 的生命周期钩子做同步**：Decky 能收到游戏启动和结束（`RegisterForAppLifetimeNotifications`、`RegisterForGameActionStart`），但不能让启动等下载完成。Decky Cloud Save 就是这样：启动时拉、退出后推，拉和游戏读档会抢。OpenSave 也用包装器（`opensave wrap <game> -- %command%`）加 systemd 用户服务加 Decky 插件，和我们的结构一样。
- **没验证的**：真机。你没有 Deck，plan-platforms 里的办法照旧：外接 SSD 装 Bazzite 测 Game Mode，加上社区测试者。

### 4.2 Windows 掌机：自动识别、手柄操作、界面

- 现在的「自动」只看 `SteamGamepadUI` / `SteamDeck` 环境变量（`crates/core/src/handheld.rs:12`），也就是只认 Steam 大屏幕模式。**ROG Xbox Ally 这类机器默认进的是 Xbox 全屏体验（FSE），现在认不出来。**
- 微软给了接口：`IsGamingFullScreenExperienceActive`（Xbox 开发文档「handheld」一节）。FSE 打开时 Windows 独占手柄的 Xbox 键，短按打开 Game Bar。**要做**：「自动」再看这个接口；FSE 下我们的掌机界面不能依赖 Xbox 键。
- 同一节文档有「handheld launchers」一页（第三方启动器怎么出现在 FSE 里），还没读，做的时候先看。
- 新定位下，从 Xbox 应用或 FSE 启动的游戏不经过 Steam：自动上传，写入要用户点。和 4.1 一样，加入 Steam 就有自动同步。
- **能在这台电脑上测的**：FSE 已经对其他 Windows 11 设备开放预览，可以在这台电脑上看掌机界面在 FSE 里的表现。真掌机靠社区。

### 4.3 掌机玩家玩第三方游戏的渠道

| 渠道 | 常见的 | 存档在哪、怎么进 Steam |
|---|---|---|
| 模拟器整合包 | EmuDeck（Deck 和 Windows）、RetroDECK（Flatpak） | 整合包自己的文件夹；Steam ROM Manager 把游戏加进 Steam |
| 单个模拟器 | RetroArch、Ryujinx / Eden / Citron、Cemu、Dolphin、PCSX2、RPCS3、PPSSPP、Xenia、shadPS4、DuckStation | 模拟器文件夹里按游戏 id 或 ROM 名；ES-DE 之类前端 |
| 重编译 / 反编译移植 | Zelda64Recomp（姆吉拉）、Ship of Harkinian（时之笛）、2 Ship 2 Harkinian、Ghostship（马 64）、SpaghettiKart（马车 64）、Starship（星际火狐）、Dusklight（黄昏公主）、Longshot、Sonic Unleashed Recomp；Linux 掌机上的 PortMaster | 各自的文件夹或游戏旁边；GitHub 发布，RecompLauncher 目前只是目录站 |
| 别的商店 | Linux：Heroic（Epic / GOG / Amazon）、Lutris、Bottles。Windows 掌机：Xbox 应用 / Game Pass（FSE）、Epic、GOG Galaxy、EA、Ubisoft、Playnite | Wine / Proton 前缀里；Heroic 能自己加进 Steam |

**对我们的意义**：Deck 上这些游戏大多会被**别的工具**加进 Steam（SRM、EmuDeck、Heroic），那些快捷方式里没有我们的包装器，所以从 Steam 启动也不会自动同步。**候选功能**：「接管已有的非 Steam 快捷方式」，把包装器加进别的工具建的快捷方式（加到启动选项前面，原来的保留）。要先看扫描现在对这些快捷方式做了什么。

### 4.4 Ludusavi 数据库能覆盖多少

- **PC 游戏：覆盖好。** 数据库来自 PCGamingWiki，19,000 多个游戏，每个游戏有 Windows / Linux / macOS 的路径（Wiki 上有的话）。在 Linux 上它会去 Proton 前缀里找 Windows 版的存档：Steam 游戏的 `compatdata`、非 Steam 快捷方式的前缀、Heroic 和 Lutris 的 Wine / Proton 前缀；已知存在注册表里的存档会导出 `.reg`。
- **缺口**：Heroic 的 Amazon 游戏和手动添加的游戏不支持；按商店账号 id 命名的文件夹（`<storeUserId>`）在别的设备上名字不同；注册表我们不恢复。
- **模拟器和重编译：基本不覆盖。** 模拟器按游戏 id 或 ROM 名存在自己的文件夹里，PCGamingWiki 不记这些；重编译移植大多不在 Wiki 上。社区有非官方的附加清单（BloodShed-Oni 的 `ludusavi-extra-manifests`、hblamo 的 `ludusavi-emudeck-manifest`），覆盖多少没核实。所以我们自己的 `adapters/`（模拟器、重编译）和 `rules/` 要留着。
- **对新定位的意义**：Ludusavi 擅长的正好是「同一个 PC 游戏在 Windows / Linux / Proton 之间」，也就是适合自动同步的那部分。它不知道两个版本是不是同一种格式，也不管模拟器：这正是你说的要让用户自己发起的那部分。

### 4.5 OpenSave 怎么做跨平台，掌机玩家现在用什么

**OpenSave**（Liquid-co/OpenSave，MIT，Go，2.4.1 · 2026-10-02，约 270 星）：

- **点对点**：局域网里自动发现、直连；跨网络用房间码经中继（`relay.opensave.org`，可以自己搭，2.4 起端到端加密）。两台设备要同时在线；可选把副本镜像到 Google Drive / Dropbox / OneDrive / WebDAV / NAS，默认关。
- **找存档**：内置 Steam、Proton / Wine 前缀（Heroic、Bottles、Lutris）、模拟器（RetroArch、Dolphin、Ryujinx、yuzu、Citra、PCSX2、RPCS3、PPSSPP、Cemu、Xenia）的规则，加上 Ludusavi 数据库；也能手动指定文件夹。跨设备认同一个游戏靠 Steam AppID（要手动开）或手动关联；Switch 游戏按 title id 在模拟器之间认成一个。
- **冲突**：和共同的基准版本比（不看时间，和我们的版本向量是一个思路），两边都改了才算冲突；可以留本机、留远端、两份都留（另一份放到分支）。
- **Deck**：Flatpak 桌面应用、命令行 systemd 用户服务（Game Mode 下也跑）、Decky 插件、`wrap` 包装器启动前拉取。
- **没有**：存档格式转换。不同系统之间的路径怎么对应，页面上没写。

**掌机玩家现在用的**：

| 工具 | 方式 | 备注 |
|---|---|---|
| Steam 云 | Steam 游戏自己的 | 非 Steam 游戏没有 |
| Ludusavi + rclone | 手动备份 / 恢复到自己的云盘 | decky-ludusavi 插件在 Game Mode 里双向同步 |
| Decky Cloud Save（及分支 SDH-GameSync） | rclone；启动时拉、退出后推，冲突自动按本机 / 远端解决 | Deck 专用；Steam 或插件崩了会漏传 |
| EmuDeck Sync | rclone；每次游戏前后同步；Drive / Dropbox / OneDrive / SFTP / Box / Nextcloud / SMB | 只管 EmuDeck 的模拟器，Windows 也有 |
| RetroDECK | 计划用 Ludusavi 做云同步（0.11），2026 年 6 月的博客说方案还在定 | |
| Syncthing | 点对点镜像文件夹 | 复古掌机圈很常见；不认游戏、不管版本，坏档也会同步过去 |
| OpenSave | 见上 | |
| Hoard | Rust，AGPL；自己的云（据它说免费 2 GB、3 台设备）或自己搭服务器；后台进程，Game Mode 下也跑 | 对比页是它自己写的 |
| EmuSync | GPL；存到自己的 Drive / Dropbox / OneDrive；有 Decky 插件 | |
| SaveState | GPL；模拟器支持深；Windows / Linux / Android | |

**结论**：「同一个游戏在几台设备间自动同步」已经有很多免费方案，而且越来越好。我们在这一项上没有优势，也不需要和它们比。我们独有的是：跨平台转换、存档以外的数据（时长、成就、截图、游戏本体备份）都在一个游戏库里、国内网盘、Windows 上和 Steam 界面深度结合（Millennium 插件）。新定位正好押在这些上面。

### 4.6 Switch 大气层端：界面和一键同步到云端

**现有的工具**：

- **JKSV**（GPL）：事实上的标准。备份、恢复所有类型的存档；能上传到 Google Drive（要用户自己建 Google Cloud 项目；2025 年改版后可以用手机登录）和 WebDAV；可以开「自动上传」，备份完上传并删掉 SD 卡上的副本。
- **uNX Save Sync**（MIT，2026 年 4 月还在更新）：多台 Switch 经一个中心服务器同步，服务器按用户和标题记修订号。
- 任天堂自己的「存档数据云备份」：要 NSO 会员；下载会覆盖主机上的存档，不能恢复。

**Ludonimbus-NX 现在**：文字界面，经局域网里的电脑同步。A 同步（哪边新传哪边）、Y 全部同步、X 云端的版本 → 恢复一个；两边都改了让用户选，覆盖前在云端留一份。不能直连云盘；只有英文；游戏要先在 Switch 上存过一次档。**也就是说，「在 Switch 上从云端恢复存档」已经有了，经过电脑。**

**新定位下的交互**（文字界面就能做，以后换 borealis 界面结构不变）：

- 列表每行：游戏名、一个状态（已备份 / Switch 较新 / 云端有更新 / PC 版有更新的进度）。
- **A 备份**：一键，不弹窗（只是在云端多一个版本）。
- **X 取回…**：列出这个游戏能取回的存档：Switch 自己最新的、保留的、别的平台最新的（「PC 版 · 2 小时前 · 需要转换」），自动存档折成一行。选一份 → 确认框「覆盖 Switch 上的存档？现在的会先保留到云端」→ 写入 → 头部一行「换成了 PC 版的存档 · 撤销」。
- **Y 全部备份**。
- 没有规则的跨平台存档不列，和桌面一样。

**直连云盘**（plan-phase5 的 S2，原定 0.8）：先 WebDAV 和 S3（就是 HTTPS 请求），再 Google Drive（设备码登录，和 JKSV 用手机登录一样）。百度、夸克要经电脑上的 OpenList，继续走局域网。

**你说的「Switch 插到电脑上从电脑恢复」**：Switch 的存档只能由主机上的程序写，电脑经 USB（DBI 的 MTP）只能碰 SD 卡。可行的做法：电脑上选好要恢复的存档 → 写到 SD 卡的 `switch/Ludonimbus/inbox/` → 下次在 Switch 上打开 Ludonimbus，提示「有 1 个从电脑发来的存档」→ 确认后写入。不需要网络，也不需要电脑和 Switch 同时开着。DBI 的 MTP 能不能直接写存档没核实。

### 4.7 Steam 云和 Ludonimbus 能不能同时开，能不能帮用户关掉 Steam 云

- **能同时开，就是 1.0 里的「只备份」**（plan-v2 §8.2）：Steam 云照常同步，Ludonimbus 只存版本，从不自动写。没有冲突，用户得到版本历史和回档。两边都写才会冲突：Steam 会问保留哪一份，或者直接换回去。
- **单个游戏的开关存在哪里，没有官方说法。** 社区说法是 `userdata/<账号>/7/remote/sharedconfig.vdf`，也有很多人报告这个开关在重启 Steam 后自己又打开了。改文件要先关 Steam，而且不可靠。
- **更好的办法：经插件让 Steam 自己改。** Steam 客户端的 JS 接口里有 `SteamClient.Apps.ToggleAppSteamCloudEnabled(appId)`，游戏详情里有 `bCloudAvailable`、`bCloudEnabledForAccount`、`bCloudEnabledForApp`（见 `plugin-decky/node_modules/@decky/ui` 的类型定义）。Windows 上的 Millennium 插件和 Deck 上的 Decky 插件都能调用。所以：
  - **装了插件**：游戏页「交给 Ludonimbus」，确认框里说清楚「会关掉这个游戏的 Steam 云存档」，确认后插件在 Steam 里关掉，**不用重启 Steam**，再读一次核实。接口是「切换」不是「设置」：先读，开着才切，切完再读。
  - **没装插件**：照现在的做法，「打开属性」（`steam://gameproperties/<appid>`），对话框每 2 秒读一次，关掉了自动打勾。
  - `GetCloudPendingRemoteOperations(appId)` 和游戏的 `cloud_status` 也许能代替读 `localconfig.vdf` 的 `last_sync_state`，同样要实测。
- 因为有人报告开关会自己打开，现有的「复查」（每次同步前看一次，发现开着就退回「只备份」）必须留着。
- **没验证的**：这些接口在现在的 Steam 客户端里能不能用、切换后是否持久。要在你的 Steam 上测，按惯例由你来。

---

## 5. 要改的东西

按对 1.0 的影响排：

| | 内容 | 大小 |
|---|---|---|
| **P1** | 不经 Steam 启动的游戏，没人选择的同步只上传（第 3 节的调用方）；游戏卡和游戏页显示「云端有更新」+「取回」 | 中 |
| **P2** | 默认不跨平台、不跨商店并线；「Xbox 版有更新的进度 → 发到…」；「共用进度」变成版本条里的高级选项 | 中 |
| **P3** | 应用顶部「全部同步」：全部上传；能直接取回的同版本存档先列出来确认 | 小 |
| **P4** | Switch 端：A 备份、X 取回（带确认和撤销）、Y 全部备份 | 小 |
| **P5** | 经插件关掉单个游戏的 Steam 云（Millennium 和 Decky） | 中，要实测 |
| **P6** | Windows 掌机：FSE 下自动进掌机界面 | 小 |
| 以后 | Switch 直连云盘（S2）；电脑经 SD 卡给 Switch 发存档；给别的工具建的非 Steam 快捷方式加包装器 | |

README、文档站的说法跟着改：从「自动同步」改成「个人云端游戏数据中心；在 Steam 里自动同步」。要等 1.0 发布时一起改（README 跟着 `main`）。

## 6. 你的决定（2026-11-27）

1. P1 到 P6 **都做进 1.0**。
2. 「共用进度」**保留成高级选项**；在支持的场景（两个版本的存档通用）可以出现提示。
3. 不经 Steam 启动的同一个版本：云端有更新时**不自动取**。Ludonimbus 在后台时弹**系统通知**，点了可以取回；启动应用时发现本机和云端不一样，**提醒处理**，或者**分成不同使用者**（这台设备单独存，plan-v2 第 7 节）。
4. P5（经插件关 Steam 云）**放进 1.0**。

## 7. 第二轮要求（2026-11-27，同一天）

| | 你说的 | 怎么做 |
|---|---|---|
| **S1** | 存档记录来源，按来源分开显示，用户能清楚地比对另一个平台 / 版本 / 模拟器的存档和现在玩的，从别处取回或发过去 | 「存档」页签按来源分组：「这里」在最上（当前、保留的、自动存档折叠），下面每个来源一组（Xbox 版、Switch、Ryujinx、另一台设备单独存的……），组头是程序的真实图标、名字、设备、最新时间。每行写清楚比对信息：时间、设备、游戏版本、大小、和现在的比新还是旧。别处的行主操作「取回到这里」（要转换的写明，没有规则的灰掉并在「?」里说原因） |
| **S2** | 发现别的平台有更新的存档，在 Steam 启动时弹窗让用户选，描述写清楚，选起来没有负担 | 包装器启动前（桌面对话框、掌机全屏提示）：「Xbox 版有更新的进度」，两行对比（哪个设备、什么时候、玩了多久），两个按钮「用 Xbox 版的进度」（这里现在的先保留，可以撤销）和「继续用 Steam 版的」；两个版本存档通用时多一个「以后自动接上」（就是「共用进度」）。同一份存档选过一次不再问 |
| **S3** | Windows 掌机的识别 | 即 P6 |
| **S4** | 学 OpenSave 怎么找 Ludusavi 没覆盖的游戏的存档，和它的 Deck 支持 | 读 OpenSave 的源码（MIT），把有用的找存档办法（Unity / Unreal 的惯例位置、模拟器规则等）作为「找存档」的建议加进来，注明出处。和破解补丁有关的存档位置不做（项目不碰盗版相关的东西） |
| **S5** | 多台 Switch 互相更新、发送存档，比 JKSV 好用；可以通过电脑设置 Switch 的登录 | 每台 Switch 是一台设备，同一个游戏在几台 Switch 上是同一个版本：别的 Switch 有更新时显示状态，X「取回」。Switch 上的用户和设置在电脑的 Switch 面板里配。Switch 直连云盘要在 Switch 上再实现一遍存储格式，先出设计和工作量评估 |
| **S6** | 「添加游戏」体验不好：Ludusavi 数据库里搜游戏添加，意义不明，「添加」按钮没反应；Ludonimbus 加入 Steam 以后自己也被认成游戏 | 修这两个问题；把「添加游戏」按「数据中心」的意图重新理一遍：扫描本机为主，手动添加时「按名字找存档位置」是其中一步，不再是一个意义不明的页签 |
| **S7** | rc14：电车炫客的 Steam 页面上「LUDONIMBUS 云存档」的位置偏了 | Millennium 插件在 Steam 装的游戏的播放栏里没对齐（比别的统计项低，右边被截掉，上面有一块模糊）。一起修 |

---

**参考**（2026-11-27 读的）

- OpenSave：[opensave.org](https://www.opensave.org/)、[cli](https://www.opensave.org/cli.html)、[GitHub](https://github.com/Liquid-co/OpenSave)
- 同类工具对比（Hoard 作者写的）：[hoard.services](https://hoard.services/guides/game-save-sync-comparison)
- Ludusavi：[README](https://github.com/mtkennerly/ludusavi)、[manifest](https://github.com/mtkennerly/ludusavi-manifest)
- Millennium：[README](https://cdn.jsdelivr.net/gh/steamclienthomebrew/millennium@main/README.md)；Steam 2026-11-11 更新：[GamingOnLinux](https://gamingonlinux.com/2026/11/steam-update-for-november-11-brings-a-64bit-linux-client-steam-achievement-sets-and-more)
- Decky Cloud Save 分支：[SDH-GameSync](https://gh.r-mj.com/AkazaRenn/SDH-GameSync)；[decky-ludusavi](https://github.com/GedasFX/decky-ludusavi)
- EmuDeck：[cloud sync](https://emudeck.github.io/cloudsync)；RetroDECK：[2026 年 6 月博客](https://retrodeck.readthedocs.io/en/latest/blog/2026/06/02/june-2026-cloudy-days-satellite-nights/)
- Xbox 全屏体验：[handheld launchers](https://devdocs.xbox.com/build/gdk-and-engines/handheld/handheld-launchers)、[Windows 博客](https://blogs.windows.com/windowsexperience/2025/11/24/xbox-full-screen-gaming-experience-now-available-for-windows-11-handhelds-and-in-preview-for-pcs/)
- Switch：[JKSV](https://github.com/J-D-K/JKSV)、[uNX Save Sync](https://www.gamebrew.org/wiki/UNX_Save_Sync_Switch)、[任天堂云备份](https://en-americas-support.nintendo.com/app/answers/detail/a_id/41208)
- 重编译 / 反编译移植：[heldgames 列表](https://heldgames.com/guides/recompiled-games-list)
- Steam 单个游戏的云开关：[Steamworks 文档](https://partner.steamgames.com/doc/features/cloud)、[社区讨论](https://steamcommunity.com/discussions/forum/1/3814039462154488700)
