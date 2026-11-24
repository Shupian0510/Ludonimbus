# 成就测试清单

这个分支（`claude/achievements-730281`）新增非 Steam 游戏的成就：读取模拟器的奖杯文件或 RetroAchievements 的记录，在 Steam 游戏页显示进度，解锁时用 Steam 自己的成就弹窗提醒（游戏里显示在 Steam Overlay 上），记录在多台电脑之间同步。设计见 [plan-achievements.md](plan-achievements.md)。

日志在 `%APPDATA%\Ludonimbus\logs\`：`achievements.log`（发现解锁、弹窗结果）、`agent.log`（本机接口）、`runner.log`（从 Steam 启动时）。

## 已经自动验证过的

- **解析**（`crates/core/src/achievements/`，用构造的样本文件）：PS4 `trophy00.trp` 的解密（用编造的测试密钥加密再解开；没有密钥时只取图标）、RPCS3 的 `TROPCONF.SFM` 和 `TROPUSR.DAT`（含时间换算）、shadPS4 的奖杯 XML 和每个用户的解锁文件、Lantern 的 `trophies.log`（几种可能的行格式）和引擎日志里的 `Runtime: trophy N unlocked`、从 `trophy00.trp` 取图标、RetroAchievements 接口文档里的返回样例（不保存触发逻辑 `MemAddr`）。
- **合并和云端**（`tests/achievements_flow.rs`）：两台模拟设备共用一个本机文件夹云盘；只有游戏运行中新出现的解锁才弹窗，关联前已有的不弹；同一个成就取最早的解锁时间；本机奖杯被重置、本地数据库被删掉后，云端这台设备的记录也不会少。
- **你这台电脑上的真实数据**（只读，`tests/achievements_live.rs`）：Bloodborne 自动认出 Lantern，奖杯组 `NPWR05818_00`（从游戏的 `npbind.dat` 读出），40 个奖杯都有图标，已解锁 0 个。仓库里没有奖杯密钥，所以这个测试在没有密钥时只检查编号和图标，有密钥（shadPS4 的 `TrophyKey` 或设置里填的）时才检查名称。
- **应用**：给插件的事件队列（插件重新加载后不补弹旧事件、按序号取新事件、ack 后不再走兜底窗口、失败时走兜底窗口）。
- **插件**：类型检查通过；`toast.ts` 用模拟的 `NotificationStore` 测过：按 `strTest: "Achievement"` 找到通知类型，游戏的 Overlay 在运行时投到 Overlay，否则走桌面通知，Steam 改版找不到接口时报出原因。
- **界面**：浏览器预览（示例数据）里点过游戏页的「成就」标签（已关联、未关联两种）、设置里的「成就」、RetroAchievements 连接、兜底小窗口，中英文，控制台没有报错。
- **没验证的**：真实 Steam 里的弹窗（桌面和游戏内）、Overlay 能不能通过 `ludonimbus-run` 加载到游戏里、兜底小窗口在真实应用里的样子和位置、RetroAchievements 真实账号、Lantern 的 `trophies.log` 的真实格式、Google Drive 上的成就文件（成就走主会话的 `CloudProvider::files()`，这里只在本机文件夹云盘上测过）。

## 需要在真实环境确认的

### 0. 安装测试版

```powershell
./scripts/build-release.ps1
```

覆盖安装生成的安装包（版本号还是 0.5.0）。应用启动时会把 Steam 里的 Ludonimbus 插件换成新版，**需要你自己重启一次 Steam** 插件才会生效。「设置 → 在 Steam 中显示云存档状态」应显示已启用。

### 1. Steam 成就弹窗（最关键）

1. **桌面**：打开 Steam，不开游戏。在 Ludonimbus「设置 → 成就」点「测试弹窗」。
   - 期望：Steam 右下角弹出成就通知（Steam 的成就音效），标题是「Ludonimbus 测试成就」，游戏名和图标是你某个已加入 Steam 的游戏。
   - `achievements.log` 里有 `popup for ludonimbus-test shown in Steam (desktop)`。
   - 如果没弹出 Steam 的通知：日志里会写 `not shown in Steam (原因)`，几秒后右上角应弹出 Ludonimbus 自己的小窗口。把这行日志发给我。
   - 如果日志写着 `shown in Steam` 却什么也没看到：看一下 Steam「设置 → 通知」里成就通知是否打开、是否开了免打扰。
2. **游戏内**：从 Steam 启动 Bloodborne（Lantern 默认无边框全屏）。先按 Shift+Tab 确认 Steam Overlay 能打开，再关掉 Overlay。切到 Ludonimbus 点「8 秒后测试（切回游戏看）」，马上切回游戏。
   - 期望：游戏画面上出现 Steam 的成就通知（位置按 Steam 设置里的「通知位置」），名称是 Bloodborne。
   - 日志里应是 `shown in Steam (overlay)`。如果是 `(desktop)`，说明 Steam 没认出这个游戏的 Overlay 在运行，告诉我当时游戏里有没有看到通知。
   - 再用时之笛（Longshot，先开启动器再开游戏）和火焰纹章各试一次，记下哪些游戏里能看到。
3. **真实解锁**：先在 Ludonimbus 的 Bloodborne →「成就」里点「关联」（第 3 节第 1 步），然后在游戏里拿一个奖杯。
   - 期望：游戏里出现 Steam 的成就通知「奖杯 N」和对应图标（Lantern 自己的奖杯通知也会出现，可以在 Lantern 设置里关掉）；`achievements.log` 有 `unlocked N`。
   - **请把 `trophies.log` 的位置和前几行内容发给我**（在 `BloodbornePC\user\savedata\` 下面），我按真实格式核对解析。
4. **关掉一个游戏的 Overlay**（Steam 里这个游戏的属性 → 通用 → 取消「在游戏中启用 Steam 界面」），重复第 2 步：记下是 Steam 桌面通知盖在游戏上、还是 Ludonimbus 的小窗口出现。试完改回来。
5. **Steam 没开**：退出 Steam，点「测试弹窗」：右上角出现 Ludonimbus 的小窗口，约 5 秒后消失，鼠标点击能穿过它。

### 2. Steam 游戏页

1. Bloodborne 的游戏页，和 Steam 自己的游戏放在相同位置：
   - 「开始游戏」栏：「LUDONIMBUS 云存档」之后有「成就 0/40」和一条细进度条，样式和 Steam 游戏的成就项一样。窗口较窄时 Steam 会把放不下的统计项藏起来，先藏的是成就，云存档留着；把 Steam 窗口拉宽看一下。右栏只有一个「成就」区块，在最上面。
   - 右栏最上面（「笔记」上方）有「成就」区块：「您已解锁 0/40（0%）」、进度条、灰色的奖杯图标和「+29」、「查看所有成就」。
2. 点任意一处弹出成就列表：40 个奖杯、有图标，未解锁的变灰；Esc 或点空白处关闭。
3. 拿到奖杯后，两处的数字在弹窗之后几秒内更新（最多 20 秒）。
4. 没有关联成就来源的游戏（例如火焰纹章）两处都不显示。
5. 在应用里取消 Bloodborne 的关联，重启 Ludonimbus：不会再自动关联（重新点「关联」才会恢复）。

### 3. 应用里的成就页

**PS4 奖杯密钥**：Ludonimbus 不附带密钥，Bloodborne 要显示官方名称需要你自己的密钥（32 个十六进制字符，来自你自己的主机或模拟器配置）。两种方式任选一种：
- 在 shadPS4 的设置里填 `TrophyKey`（写在 shadPS4 的 `config.toml`），Ludonimbus 自动使用；
- 或在 Ludonimbus「设置 → 成就 → PS4 奖杯密钥」粘贴后点「保存密钥」，状态变成「已设置」。密钥只保存在这台电脑的 `%APPDATA%\Ludonimbus\achievements\settings.json`，不会上传到云盘。

没有密钥时奖杯显示为「奖杯 N」和图标，其他功能照常。

1. Bloodborne →「成就」标签：「找到的奖杯数据」里有 Lantern，点「关联」后列出 40 个奖杯和图标。
   - **有密钥**：名称和说明是官方简体中文（例如「亚南日出」），有等级（白金 / 金 / 银 / 铜）；界面切到 English 后点「刷新」变成英文名称。之前没有密钥、后来才填的，填好后点「刷新」就变成名称。
   - **没有密钥**：显示「奖杯 1」…「奖杯 40」和图标。
   - 在设置里点「清除」后再「刷新」，名称变回编号（用 shadPS4 的 `TrophyKey` 时设置里没有「清除」，要在 shadPS4 里删掉）。下面有「解锁时在 Steam 里弹出成就提醒」开关和关掉 Lantern 自带通知的提示。
2. 「刷新」「取消关联」能用，取消后可以重新关联。
3. 「设置 → 成就」：两个开关的状态重新打开应用后保持；PS4 奖杯密钥的状态（来自 shadPS4 / 已设置 / 未设置）也保持。填一个不是 32 个十六进制字符的值，提示格式不对、不保存。

### 3b. 存档规则和 PS4 移植（2026-11-24 新增）

1. **BotW（Cemu）**：打开 Ludonimbus 后过几秒，BotW 自动关联到「TrophyNX」，「成就」标签列出 33 个成就（英文），已解锁的有 Rise and Shine、Sheikah Slate、Out of the Cave、Marathon Runner（按你现在的存档）。Steam 里 BotW 的游戏页也显示「成就 4/33」。
2. 在 BotW 里拿一个新的（例如再开一座神庙、拿到一个规则里的道具），存档后退出：几秒内成就数增加，游戏运行中存档时会弹出 Steam 的成就提醒。
3. 断网后打开 Ludonimbus：BotW 的成就照常显示（用的是缓存的规则）。
4. 有 Ryujinx 游戏或 Switch 重编译游戏（Super Mario Odyssey、Hollow Knight、Slay the Spire …）时：加进 Ludonimbus 后自动关联「TrophyNX」，成就数和存档进度对得上。
5. 有 PS4 移植版（Diablo III、Dead Nation、Resogun、Uncharted …）时：自动关联「PS4 port」，有 PS4 奖杯密钥时名称是官方中文（没有时是编号）；解锁一个奖杯后几秒内出现。**这条没有真实样本，最需要确认**，把 `achievements.log` 和移植版文件夹里的奖杯记录文件位置发给我。

### 4. 多台电脑（可选，有第二台电脑时）

1. 第 1 节第 3 步拿到奖杯并退出游戏后，云盘里有 `Ludonimbus/games/bloodborne-lantern/achievements/<设备 ID>.json` 和 `schema.json`。用 Google Drive 时特别看一下这两个文件写进去了。
2. 在另一台电脑上关联同一个游戏（或者不关联也行），「成就」标签里能看到这个奖杯，写着「在 <这台电脑的名字> 上解锁」。

### 5. RetroAchievements（可选，你决定以后测）

1. 「设置 → 成就 → RetroAchievements 账号」填用户名和 Web API Key，「连接」后显示已连接；填错的 Key 会提示用户名或 Key 不对。
2. 在 RetroArch 等模拟器里玩一个有 RA 成就的游戏后，Ludonimbus 里这个游戏的「成就」→「列出最近玩过的游戏」能看到它，「关联」后列出全部成就和你的解锁记录（分数、硬核标记）。
3. 游戏中解锁一个成就，约 30 秒内弹出 Steam 的成就通知。

### 6. 原有功能

1. 游戏页的云存档状态、立即同步照常。
2. 从 Steam 启动游戏、退出后上传存档正常；`runner.log` 没有新的报错（退出后多一次读取成就和上传成就记录）。

遇到问题时，把上面三个日志和截图发给我。
