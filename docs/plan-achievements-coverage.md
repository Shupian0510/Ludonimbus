# 成就覆盖面评估：Ludonimbus 自己维护成就库可行吗（草案 v0.3 · 2026-11-24）

> **背景**：第一批来源（[plan-achievements.md](plan-achievements.md)）覆盖 RetroAchievements 支持的老主机游戏，以及模拟器自己记录奖杯的 PS3 / PS4。但 Bloodborne（Lantern）只有编号和图标没有名称，重编译和移植游戏（时之笛 Longshot、火焰纹章 recomp 等）、Switch 游戏根本没有成就。这里评估「Ludonimbus 自己维护一套开源成就库」能做到什么、该怎么做。
>
> **结论先说**：要分成两种库，做法完全不同。
>
> | | 名单库：给**本来就有官方成就**的游戏补名称和图标 | 规则库：给**没有成就**的游戏定义成就 |
> |---|---|---|
> | 例子 | Bloodborne 的 40 个奖杯 | 时之笛：集齐 36 个心之碎片、通关每个迷宫 |
> | 内容是谁的 | 名称、说明是发行商写的短句；图标是游戏美术 | 社区自己写的文字、自己画的图标 |
> | 能否放进仓库 | **文字可以由社区录入**（短句，风险低）；**图标不放**（用户本机游戏文件里有）；**不从奖杯网站整包复制**（见 2.3） | **可以**，这是 Ludonimbus 能自己做、别人没做的部分 |
> | 判定解锁 | 模拟器自己判定（已实现） | Ludonimbus 读存档判定 |
> | 工作量在哪 | 每个游戏录入一次名单 | 每个游戏都要写规则，要逆向存档格式 |
>
> **2026-11-24 更新**：你决定不做 PSN 登录。调查结果是**没有开放许可的官方成就数据集**，奖杯网站都没有公开接口（见 2.1）。所以名单也走「社区维护、放进同一个开源库」的路线，和规则库合成一个库。
>
> **2026-11-24 第二轮（核实了你转来的调查报告，见第 0 节）**：有三件事改变了结论：
> 1. **PS4 的官方名单可以从用户自己的游戏文件解出来**：游戏的 `trophy00.trp` 里有全部语言的名称和说明，用 PS4 奖杯密钥解密（shadPS4 和 PSerban93/Achievements 都是这么做的）。在你的 Bloodborne 上验证过：22 个语言文件全部解开，40 个奖杯，有官方简体中文名称（例如「亚南日出」）。**Ludonimbus 不附带密钥**（见 0.1）：用 shadPS4 里设置的 `TrophyKey`，或者用户在 Ludonimbus 设置里填的自己的密钥；没有密钥时只有编号和图标。
> 2. **Switch 已经有社区规则库**：TrophyNX（GPL-2.0）有 45 个 Switch 游戏的成就和读存档的规则，格式和第 3 节设想的几乎一样。Ludonimbus 能找到 Ryujinx 每个游戏的存档，可以直接用这些规则。
> 3. **PS4 重编译游戏的解锁文件格式**已经被 PSerban93/Achievements 整理好了（MIT，可以照着实现）。

---

## 0. 第二轮调查：核实过的项目（2026-11-24）

| 项目 | 存在 / 许可 | 是什么 | Ludonimbus 怎么用 |
|---|---|---|---|
| [PSerban93/Achievements](https://github.com/PSerban93/Achievements) | 存在，**MIT**，270 星，2026-11 还在更新 | Electron 成就工具，支持 Steam、模拟器、PS3 / PS4 / Xbox 360 重编译 | ① 用 PS4 奖杯密钥以 AES-128-CBC 解密 `.ESFM`，和 shadPS4 的做法相同（Ludonimbus 照这个做法实现，但不附带密钥）；② `utils/ps4-recomp*.js` 读 PS4 移植版的解锁记录：`trophies.log`（每行「编号 Unix 秒」，**确认了 Lantern 的格式**）、`unlocked.txt`、`progress_user*.txt`（`DCP-TROPHIES 1` 开头）、`trophies.dat`（16 字节位图）、`CUSAxxxxx-userN.bin`、运行日志里的 `Runtime: trophy N unlocked` / `Trophies: trophy N (platinum) unlocked`。可以照着实现（MIT，注明出处）。它也抓取 Exophase 的网页 |
| [Imp3r1us/TrophyNX](https://github.com/Imp3r1us/TrophyNX) | 存在，**GPL-2.0**，2026-11 还在更新 | Switch 自制软件，给 Switch 游戏加成就 | `romfs/games/<标题 ID>.json`：45 个游戏，社区写的成就 + 规则。读取器：`json`、`byml`（任天堂的二进制格式）、`binary_le`（固定偏移）；规则：`flag`、`count`、`sum`、`launches`、`playtime_hours`；有「Save Lab」找字段（就是 3.3 设想的工具）。**GPL 不能直接复制进 MIT 的仓库**：可以运行时从它的仓库下载规则文件来读，或者请作者同意以宽松许可提供数据；Ludonimbus 的规则格式可以兼容它 |
| [RecompAchievements](https://github.com/RecompAchievements-org/recompachievements-web) | 存在，**AGPL-3.0**，alpha | 给重编译游戏的在线成就服务 | `Catalog/Games/` 只有 9 个游戏（含 Bloodborne 移植、Zelda 64: Recompiled），成就定义在，但解锁要游戏接入它的 SDK。现在用不上 |
| [RecompLauncher 的 Recomp Achievements](https://github.com/elripalda/RecompAchievements) | 存在，**自定义许可（保留权利）** | RecompLauncher 的成就系统：游戏通过 SDK（含 rcheevos 内存规则、N64 Modern Runtime 和 ReXGlue 的桥接）经命名管道把解锁告诉 RecompLauncher | 解锁记录在 RecompLauncher 和它的在线服务里，没有给第三方读的接口。以后可以问作者能否开放 |
| [Achievement Watcher Next](https://github.com/Shirowwww/Achievement-Watcher-Next) | 存在，LGPL-3.0 | 多平台成就追踪 | 参考它的来源设计 |
| [recompiled-games-list](https://github.com/wangxiaoxiang5599/recompiled-games-list) | 存在，**CC BY 4.0** | 重编译游戏目录（JSON / CSV） | 可以用来补游戏识别（注明出处） |
| PSN API、PSNProfiles、Exophase | — | — | 不用（你决定不做 PSN；网站没有许可） |
| Steam Web API / RAWG | — | 同款游戏的 Steam 成就定义 | Steam 定义只能当显示用，和 Switch / 重编译版的解锁对不上；以后再看 |

### 0.1 PS4 奖杯密钥怎么来（2026-11-24 决定：不附带，用用户自己的）

| 做法 | 体验 | 风险 |
|---|---|---|
| a. 在仓库和安装包里**内置**密钥 | 什么都不用做 | 密钥是索尼的，开源仓库分发它可能被要求删除，连带整个仓库的历史（shadPS4 就刻意不内置） |
| b. **用户填自己的**（Ludonimbus 设置 →「成就」→「PS4 奖杯密钥」） | 有密钥的人粘贴一次 32 个十六进制字符 | 无：密钥只存在这台电脑的 `achievements\settings.json` 里，不上传、不同步 |
| c. **自动读已有的**：shadPS4 的 `config.toml`（`TrophyKey`） | 在 shadPS4 里设置过的人不用填 | 无。Lantern 没有类似的设置（它的名称写在启动器里） |

**决定：c 优先，其次 b，不做 a**（一度选了 a，合并前改掉了，分支历史里也不留密钥）。没有密钥时 PS4 奖杯显示编号和图标（图标在 `trophy00.trp` 里不加密）。设置里的提示只说密钥来自用户自己的主机或模拟器配置，不给出密钥、也不链接到发布密钥的页面。解出来的名单只缓存在本机，换了密钥或清除密钥后自动重新解出。

### 0.3 火焰纹章 万缕千丝 recomp 的存档（2026-11-24 调查）

- **文件**：`save/account/<用户>/000/` 下的 `Auto`、`Chapter_<篇>_<部>_<章>`（例如 `Chapter_Ism_0_2`）和 64 字节的 `Global`。
- **格式**：文件头 0x124 字节（魔数 `DD BC EF CD`），之后是一整段 zstd，不加密。解开后 `Auto` 约 140 KB。
- **内容**：按名字组织的块，块表是「FNV-1 32 位哈希（名字的 UTF-8）→ 偏移」。能对上的块名有「実績」「環境設定」「限定」「中継」「共有」「イベント」「状況」「日時」和九个「…篇」。块里是变量表：每条是「哈希 + 值」，值是 1 个字节，或者 `FF` 加 4 个字节（整数或浮点）。
- **卡在哪里**：变量名在游戏能读到的数据表（`GameData/*.bin`）、脚本（`Script/**`）、对话脚本（`Msg/TalkMsg.elf`）和程序的字符串里都找不到，用 FNV-1 / FNV-1a / CRC32 / Murmur3 和 UTF-8 / UTF-16 / Shift-JIS 都试过。名字多半在编译时就只剩哈希了。
- **还能怎么做**：对比同一个存档在不同进度的版本，认出「每过一章就变」的变量。你现在的两个存档都在 0 部 2 章，不够。等 Ludonimbus 的版本历史里有了后面几章的存档再做；或者请 recomp 作者在日志里写出章节完成（它已经有 `fefw.log`）。

### 0.2 修订后的顺序

| 步骤 | 内容 | 覆盖 |
|---|---|---|
| 1 | ✅ **已完成（2026-11-24）** PS4 奖杯解密（`achievements/trp.rs`）：有密钥时（shadPS4 的 `TrophyKey` 或设置里填的）名称、说明、等级、隐藏都来自游戏文件，按界面语言选 `TROP_XX`；没有密钥时只取图标。在你的 Bloodborne 上验证：40 个奖杯，官方简体中文名称和图标 | Bloodborne；所有能找到游戏文件夹的 shadPS4 / Lantern 游戏（PS4 重编译游戏的识别在第 2 步） |
| 2 | ✅ **已完成（2026-11-24）** 新来源 `ps4port`（`trophy.rs`）：在移植版文件夹里找原版游戏的 `npbind.dat` 和 `trophy00.trp` 解出名单；解锁读 `trophies.log`、`unlocked.txt`、`progress_user*.txt`、`trophies.dat` / `CUSA…-userN.bin`（128 位位图）、shadPS4 式 `<NPWR>.xml`、运行日志。Ludonimbus 规则里平台是「（PS4）」的游戏（Diablo III、Dead Nation、Resogun …）自动关联。格式用构造的文件测过，还没有真实移植版的样本 | Bloodborne 以外的 PS4 移植 |
| 3 | ✅ **已完成大部分（2026-11-24）** 新来源 `rules`（`rules.rs`）：TrophyNX 格式，运行时从它的仓库下载并缓存（不放进仓库），本机 `achievements\rules\<标题 ID>.json` 优先。读取器：`json`（含 XOR）、`byml`、`gamedata_hash`（自动判断字节序，Switch 小端、Wii U 大端）、`totk_progress`、`binary_le`；规则 `flag` / `count` / `sum` / `launches` / `playtime_hours`。按标题 ID 关联：Ryujinx 等模拟器的每个游戏、Switch 重编译（Super Mario Odyssey、BotW、Kirby、Pokémon Legends），**以及 Cemu 上的 BotW（Wii U 存档结构相同）**。在你电脑上的 BotW（Cemu）实测：下载规则、找到 5 个存档槽、判断出大端，解锁 4 个。还没做的读取器：`gvas`、`nrbf`、`sgb`、`deltarune`、`theatrhythm`；名单只有英文 | TrophyNX 的 17 个有存档规则的游戏 + 28 个只有游玩统计的游戏 |
| 4 | ✅ **开始了（2026-11-24）** 自己的规则库：`adapters/achievements/<适配器>.json`，格式兼容 TrophyNX，加了中文字段，随应用发布，按适配器自动关联（写法见 [achievement-rules.md](achievement-rules.md)）。第一份是 **Longshot 的时之笛**（45 个成就：精灵石、勋章、乐曲、主要道具、黄金骷髅蜘蛛 10 / 50 / 100、生命上限、加侬城结界、小游戏），名字从游戏自己的数据列表核对过，你的 6 个存档槽解锁 13 个。**火焰纹章 万缕千丝 recomp** 暂时做不了，见 0.3。Ship of Harkinian、2 Ship 2 Harkinian、Zelda 64 Recompiled、sm64 移植等要先有存档样本（你这台电脑上没有装）。「记录存档变化」工具还没做 | 没有成就的重编译游戏 |
| 5 | Xenia；其他商店 PC 游戏 | Xbox 360、GOG / Epic |

第 2 节的「社区名单」对 PS4 不再需要，只留给以后「模拟器只给编号、又解不开游戏文件」的情况。

---

## 1. 现在覆盖到哪里

| 平台 / 运行方式 | 有官方成就吗 | 现在能拿到的 | 缺什么 |
|---|---|---|---|
| NES～PS2、PSP、NDS、GC、Wii 等（RetroArch、DuckStation、PCSX2、PPSSPP、Dolphin） | RetroAchievements 社区成就 | 全部（RA） | — |
| PS3（RPCS3） | 有 | 全部：PS3 的奖杯文件不加密，RPCS3 解出的 `TROPCONF.SFM` 有名称 | — |
| PS4（shadPS4、Lantern） | 有 | 解锁记录、图标（`trophy00.trp` 里的 PNG 不加密） | **名称和说明**：在 `.ESFM` 里，加密，要奖杯密钥 |
| PS Vita（Vita3K） | 有 | 第二批，未做 | 待核实 |
| Xbox 360（Xenia） | 有 | 第二批，未做；名称、说明、图标都在游戏自己的 XEX（SPA 资源）和 Xenia 写的 GPD 里 | — |
| Switch（Ryujinx、Eden 等） | **没有**（Switch 没有系统级成就） | — | 全部 |
| 重编译 / 移植（Ship of Harkinian、Zelda 64 Recompiled、Longshot、火焰纹章 recomp…） | **没有** | — | 全部 |
| 其他商店的 PC 游戏（GOG、Epic…） | 有（各家商店） | 不读（plan-achievements 1.6） | 解锁记录要登录各家商店 |

你这台电脑上：shadPS4 的 `TrophyKey` 是空的，所以 Bloodborne 的名称现在没有办法从本机文件解出来；Steam 的 `appcache/stats/` 里缓存了 733 个游戏的成就定义（含各语言名称、说明、图标），但只对在 Steam 上有的游戏有用。

---

## 2. 名单：给有官方成就的游戏补名称和说明

### 2.1 能找到的开源数据和成就网站（2026-11-24 调查）

| 来源 | 有什么 | 能不能用 |
|---|---|---|
| **RetroAchievements** | 社区写的老主机成就，条款明确允许第三方重用标题和说明 | 已经在用（需要用户自己的 Web API Key）。只覆盖老主机，没有 PS4 / Switch |
| **Steam Web API**（`GetSchemaForGame`）和本机缓存 `appcache/stats/` | 官方接口，名称、说明、图标都有 | 只对在 Steam 上发行的游戏有用；接口需要一个免费的 Key，本机缓存只有玩过或看过的游戏 |
| Kaggle 上的 PS4 游戏 / 奖杯数据集（例如「All Playstation 4 Games」） | 从 TrueTrophies 抓来的 | **不能用**：没有写许可证，来源网站声明版权所有 |
| 学术数据集（论文「You Have Earned a Trophy」，WebSci'22） | 从 PSNProfiles 抓的全量 PlayStation 奖杯 | **不能用**：没有公开发布，也没有数据许可 |
| GitHub 上的相关项目（psn-api、PSNAWP、PSN100 等） | 都是调用 PSN 的工具，要用户自己的 PSN 登录 | 你决定不做 PSN 登录 |
| **Exophase** | 公开网页，覆盖 PlayStation / Xbox / Steam 等，**有简体中文页面**（`/trophies/zh-CN/`） | 没有公开接口文档；robots.txt 只禁止 `/search`；服务条款页面拒绝自动访问，没读到内容。Playnite 的成就插件就是抓它的网页。**未经许可不建议用**，可以写信问他们是否允许开源应用按需读取 |
| **PSNProfiles** | 公开网页 | 没有接口；robots.txt 声明 `use=reference`、禁止 AI 训练；条款没读到。**不建议** |
| **TrueTrophies / TrueAchievements** | 公开网页 | robots.txt 开头就是「go away」，并禁止访问它的移动端接口。**不能用** |
| **PSNINE（P9，中文奖杯社区）** | 中文名称 | 没有公开接口。**不建议** |

结论：**没有能直接拿来用的开放成就数据**。能用的只有用户自己的游戏文件、模拟器，以及社区自己录入的名单。

### 2.2 可行的路线

| 路线 | 覆盖 | 做法 | 评价 |
|---|---|---|---|
| **A. 社区名单**（放进开源库） | 任何「模拟器只给编号」的游戏，先是 Bloodborne | 每个奖杯组一个文件：编号 → 各语言的名称和说明。由玩家对照自己游戏里的奖杯页面录入（Lantern 的「奖杯」页、主机上的奖杯列表），**不从奖杯网站复制**；不放图标（本机 `trophy00.trp` 里有） | **推荐**。和规则库同一个仓库、同一种提交流程，见第 3 节 |
| B. 用户自己的奖杯密钥 | PS4 | 用户在设置里填密钥（Ludonimbus 不提供），解开自己游戏里的官方名单 | ✅ 已实现（2026-11-24），见 0.1 |
| C. 游戏自带的定义 | Xbox 360（XEX 里的 SPA）、PS3（TROPHY.TRP 不加密） | 直接读 | 做 Xenia 来源时用 |
| D. Steam 的定义 | 也在 Steam 上发行的 PC 游戏 | Web API 或本机缓存 | 以后做其他商店的游戏时用 |
| E. 用户导入 | 任何游戏 | 在应用里粘贴一份名单 | 兜底；导入的内容只存本机，用户也可以顺手提交到社区名单 |
| F. 问上游要 | Lantern | 请 Lantern 作者把它内置的名称和编号写进 `trophies.log` 或单独的文件 | 成本最低，值得先问 |
| ✗ PSN 登录 | — | — | 你决定不做 |
| ✗ 未经许可抓取奖杯网站 | — | — | 不做 |

**Lantern 本身的情况**（只读检查你这台电脑上的 `Lantern Engine.exe`）：启动器里有界面翻译表，包含 Bloodborne 奖杯的名称和说明，共 13 种语言（简体中文例如「神职人员怪兽 / 击败神职人员怪兽。」，这是 Lantern 自己的译法，不一定是官方中文版的叫法）。但翻译表里**没有奖杯编号**，界面上其他地方用过的词条被合并掉了，顺序也就不能当编号用；编号和名称的对应写在程序代码里。所以 Ludonimbus 不能可靠地从 Lantern 的文件里取名单，只能走 A 或 F。

### 2.3 放进仓库的边界

- **可以放**：社区成员自己录入的名称和说明。奖杯名称和说明都是很短的句子，单独的短语一般不受著作权保护，风险低；在文档里写明「由社区录入」，收到权利人要求时可以单独删除。
- **不放**：图标（游戏美术，版权明确，而且用户本机的游戏文件里就有）；从奖杯网站整页、整库复制的内容（网站条款和数据库权利）；PS4 奖杯密钥。
- **对照表**也放进来：`CUSA03173 → NPWR05818_00` 这类游戏 ID 和奖杯组的对应，帮助自动关联。

### 2.4 Bloodborne 怎么办

1. 先问 Lantern 作者能否把名称和编号写出来（F），成本最低。
2. 同时在社区库里加 Bloodborne 的名单（A）：40 条，编号对照 Lantern 的「奖杯」页面或主机上的奖杯列表，图标用本机已有的。录入一次，所有用 Lantern 或 shadPS4 玩 Bloodborne 的人都能用。
3. 有奖杯密钥的人可以用 B 直接得到官方中文名单。

---

## 3. 规则库：给没有成就的游戏定义成就

这是 Ludonimbus 能自己维护、而且没有版权问题的部分（文字和图标都是社区自己写、自己画的）。第 2 节的社区名单放在同一个库里：`adapters/achievements/` 下，名单文件只有编号和文字（判定交给模拟器），规则文件有文字和判定条件，提交、审核、联网更新都一样。

名单文件示意：

```yaml
# adapters/achievements/trophies/NPWR05818_00.yaml（Bloodborne）
trophySet: NPWR05818_00
games: [CUSA03173, CUSA00207, CUSA00900]   # 对照表：哪些游戏 ID 用这个奖杯组
trophies:
  - id: 0
    grade: platinum
    name: { zh-CN: …, en: … }
    description: { zh-CN: …, en: … }
  # …共 40 条；图标用本机 trophy00.trp 里的
```

### 3.1 类似的项目

| 项目 | 做法 | 能借鉴的 |
|---|---|---|
| RetroAchievements | 社区写成就，模拟器读**内存**判定，审核后发布 | 成就设计规范、审核流程、开发工具（内存查看器）是社区能做起来的关键；它的触发逻辑不能拿来用（条款保留），也不适用于存档 |
| Ludusavi 清单 | 社区维护的存档位置清单（来自 PCGamingWiki），应用下载更新 | **发布和更新方式**照搬：规则是 YAML，放 GitHub，应用随版本内置，也可以联网更新，不用等发版 |
| Gamerzilla | 开源成就库，游戏自己调用接口上报 | 需要改游戏，不适用；可以参考它的成就描述格式 |
| Playnite 的 SuccessStory / Playnite Achievements | 汇总 Steam、GOG、Epic、Xbox、PSN、RA、RPCS3、shadPS4、Xenia 等；没有成就的游戏可以手动加目标 | 「各平台各一种读取方式」和我们的来源设计一致；它们读 PSN 也走用户登录 |

第一轮没找到「基于存档、社区维护」的现成项目；第二轮找到了 TrophyNX（Switch，见第 0 节），重编译游戏这一块仍然是空白。

### 3.2 规则长什么样

每个游戏一个文件 `adapters/achievements/<游戏>.yaml`，用现有的游戏规则 key 匹配（`soh`、`zelda64recomp`、`fefw-recomp`、Longshot…）。下面是示意，字段路径要以实际存档为准：

```yaml
game: soh
version: 1
achievements:
  - id: hearts-all
    name: { zh-CN: 心之容器, en: Heart of Hyrule }
    description: { zh-CN: 收集全部 36 个心之碎片, en: Collect all 36 Pieces of Heart }
    icon: icons/heart.svg          # 社区自己画的，或用通用奖杯图标加颜色
    progress: { max: 36 }
    when: { json: "$.sections.base.data.pieceOfHeart", ">=": 36 }
  - id: forest-temple
    name: { zh-CN: 森林神殿, en: Forest Temple }
    when: { json: "$.sections.base.data.dungeonsCleared", has: forestTemple }
```

判定方式（只用声明式条件，规则里不能写代码，联网更新也安全）：

| 存档类型 | 例子 | 条件写法 |
|---|---|---|
| JSON | Ship of Harkinian、2Ship2Harkinian 等新移植 | JSON 路径 + 比较 |
| 有公开结构的二进制 | N64 / GC 游戏（反编译项目把存档结构写得很清楚），Zelda 64 Recompiled | 偏移 + 类型（u8 / u16 / 位标志）+ 字节序 |
| 键值表 | Longshot：文件头后面是约 2 万条「32 位名称哈希 + 值」 | 哈希 + 比较（需要先认出哪个哈希代表什么，见 3.3） |
| 文本（INI / XML） | 很多 PC 游戏 | 键 / 路径 + 比较 |
| 组合 | — | 计数、求和、全部 / 任一 |

进度型成就（「心之碎片 23/36」）直接用 `progress`，Steam 游戏页和弹窗都能显示进度。

### 3.3 关键在工具：怎么找到存档里的含义

规则库能不能做起来，取决于写一条规则要多久。像 RetroAchievements 靠内存查看器一样，Ludonimbus 需要一个「**记录存档变化**」工具（和现有的「玩一下再找存档」是同一个思路）：

1. 在应用里点「开始记录」，Ludonimbus 记下当前存档。
2. 去游戏里做一件事（拿一个心之碎片）并存档。
3. 点「这里发生了：____」，Ludonimbus 列出变化的键 / 偏移 / JSON 字段（例如 `0x62596bab: 0 → 1`）。
4. 重复几次确认，点「生成规则」，得到一段可以直接提交的 YAML。

再加一个规则测试命令（`ludonimbus achievements test <游戏> --save <路径>`，列出这份存档满足哪些成就），提交前验证。

### 3.4 怎么判定、和现有功能怎么接

- 新来源类型 `rules`，后面和现有来源完全一样：同一个本机数据库、云端每台设备的记录、Steam 游戏页显示、Steam 弹窗。
- 判定时机：游戏运行时存档文件变化（和奖杯文件一样每 2 秒看一次）；游戏退出后再判定一次。
- **从云端恢复存档、从别的设备同步来的存档不算新解锁**：恢复后的第一次判定只记录、不弹窗，避免一恢复 100% 存档就连弹几十个。个人使用，不做防作弊。

### 3.5 工作量

| 部分 | 估计 |
|---|---|
| 判定引擎（JSON、二进制偏移、键值表、文本，进度型） | 约 2 天 |
| 「记录存档变化」工具和规则测试命令 | 约 2 天 |
| 规则联网更新（照 Ludusavi 清单：GitHub 上的 YAML，校验格式后使用） | 约 0.5 天 |
| 每个游戏的规则 | JSON 存档：几小时；有反编译资料的二进制：半天到 1 天；完全没资料的（Longshot、火焰纹章 recomp）：1～3 天，主要花在用工具认字段 |
| 贡献文档（规则格式、成就设计建议、图标规范） | 0.5 天 |

覆盖面不是代码能解决的：要靠持续加规则。建议先做你常玩的几个，形成样板，再在文档站和 README 里邀请贡献。

---

## 4. 建议的顺序

| 步骤 | 内容 | 覆盖 |
|---|---|---|
| 1 | 社区库的格式和读取（名单 + 对照表），应用里的导入（E）；写信问 Lantern 作者（F） | — |
| 2 | 第一份名单：Bloodborne（40 条，对照 Lantern 的奖杯页录入） | Bloodborne 有中文名称和说明，图标用本机的 |
| 3 | 奖杯密钥（B，可选） | 有密钥的 shadPS4 用户的所有 PS4 游戏 |
| 4 | 规则库：判定引擎 + 「记录存档变化」工具 + 测试命令 | — |
| 5 | 第一批规则：Ship of Harkinian（JSON，最容易）、Zelda 64 Recompiled（有反编译资料），然后用工具做 Longshot | 时之笛、姆吉拉的假面 |
| 6 | Xenia 来源（定义来自游戏自己的 XEX，解锁来自 GPD） | Xbox 360 |
| 7 | 联网更新、贡献文档；之后按需求加名单和规则（火焰纹章 recomp、Switch 游戏…） | 逐步扩大 |
| 以后 | 其他商店的 PC 游戏：名称用 Steam 的定义（D），解锁读 GOG / Epic（要登录） | GOG / Epic |

**不做**：PSN 登录；未经许可抓取奖杯网站；把图标放进仓库；读 PC 游戏 / 重编译游戏的内存（容易被杀毒软件和反作弊当成恶意程序，每个版本偏移都会变）。

---

## 5. 需要你决定的事

1. ~~PSN 路线~~：2026-11-24 决定不做。
2. ~~PS4 奖杯密钥~~：2026-11-24 决定不附带，用 shadPS4 的 `TrophyKey` 或用户在设置里填的（见 0.1）。
3. **TrophyNX 的规则**：运行时从它的仓库下载来用（不放进我们的仓库），同时由你决定要不要写信请作者以宽松许可提供数据（发出去要你同意，我可以起草）。
4. **下一步**：0.2 的第 2 步（其他 PS4 重编译游戏的解锁文件）或第 3 步（TrophyNX 规则用于 Ryujinx 存档）。

## 参考

- 开放数据调查：[Kaggle「All Playstation 4 Games」（Baselight 镜像，来源 TrueTrophies）](https://baselight.app/u/kaggle/dataset/shivamb_all_playstation_4_games)、[论文「You Have Earned a Trophy」](https://arxiv.org/abs/2205.15163)、[Exophase 中文奖杯页示例](https://www.exophase.com/game/astro-bot-psn/trophies/zh-CN/)、robots.txt：[Exophase](https://www.exophase.com/robots.txt)、[TrueTrophies](https://www.truetrophies.com/robots.txt)、[PSNProfiles](https://psnprofiles.com/robots.txt)
- PSN 奖杯接口（非官方，已决定不用）：[psn-api](https://npmjs.com/package/psn-api)
- shadPS4 解密奖杯文件：[trp.cpp](https://github.com/shadps4-emu/shadPS4/blob/main/src/core/file_format/trp.cpp)
- 汇总类工具：[Playnite SuccessStory](https://github.com/Lacro59/playnite-successstory-plugin)、[Playnite Achievements 支持的平台](https://pserban93.github.io/Achievements-Docs/platforms.html)
- 开源成就系统：[Gamerzilla](https://fedoramagazine.org/open-source-game-achievements/)
- 本机：`Steam\appcache\stats\UserGameStatsSchema_<appid>.bin`（Steam 缓存的成就定义）、`%APPDATA%\Longshot\save\…\slot_00\progress.sav`（键值表格式）
