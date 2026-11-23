# 0.4 测试清单：添加游戏和一键加入 Steam

0.4 新增：「添加游戏 → 扫描本机」、找存档向导（含「玩一下再找」）、一键加入 Steam、SteamGridDB 选封面。设计见 [plan-phase2.md](plan-phase2.md) 第 2、3 节。

日志在 `%APPDATA%\Ludonimbus\logs\`。每次改 `shortcuts.vdf` 之前都会把原文件备份到 `%APPDATA%\Ludonimbus\steam-backups\`，出问题时可以拿回来。Ludonimbus 里选的封面在 `%APPDATA%\Ludonimbus\art\<游戏>\`。SteamGridDB 的 Key 在凭据管理器里，名字是 `steamgriddb-api-key`。

## 已经自动验证过的

- 模拟器和重编译游戏的规则：`cargo test -p ludonimbus-core` 里的 adapters / emulators / recomplauncher / spec 单元测试和 `--test emulator_rules`。规则文件都能解析、根目录都认识；GameCube / Wii（ISO、RVZ、WBFS）、Wii U meta.xml、3DS 头部、PARAM.SFO、PSP ISO / EBOOT、序列号、Vita3K、RetroArch 文件名都能读出游戏 ID；Dolphin 快捷方式只同步那个游戏、只开模拟器时同步全部；Ryujinx 第二个快捷方式被拒绝并在扫描里标为「已经添加」，不同步 `.lock` 和密钥；exe 在子文件夹里的重编译游戏 `{install}` 是发布包根目录，程序、日志、崩溃转储、着色器缓存不进快照；RecompLauncher 便携版的 library.json 和 `--play` 快捷方式；Cemu 设置里的 MLC 路径（含非 UTF-8 内容）。
- 本机真实数据（只读）：`discover --folder Desktop\Switch` 认出 `publish` 里的 Ryujinx（整个模拟器，`%APPDATA%\Ryujinx` 里 19 个文件约 20 MB，不含 prod.keys）和 ZeldaBW 整合包里的 Cemu（读到 settings.xml 里的 `ZeldaBW\MLC`；带 `-g U-King.rpx` 时只取旷野之息的 20 个存档文件）。
- 模拟器数据目录扫描和 Ryujinx 按游戏：`switch` / `emugames` 单元测试和 `--test ryujinx_per_game`、`--test emulator_rules`。存档索引按真实文件的字节读写；Ryujinx 快捷方式带 NSP 时只同步这个游戏已提交的存档（`0`），名称取 Ryujinx 显示的；换到另一台电脑（另一个用户档案、编号不同、没有这个存档）恢复时新建 8 号存档，索引两份都更新、旧索引留在本地备份，`0` 和 `1` 都是恢复的内容，再恢复一次不会再建；整体同步和逐个游戏不能同时添加。NSP 的 `.cnmt.xml`、票据文件名、NCA 头（NSP / XCI，含 4 KB 密钥区的 XCI）都能读出标题 ID。数据目录扫描：Dolphin GCI / Wii 存档、Cemu（游戏列表名称和启动参数）、PPSSPP、RPCS3、shadPS4、Vita3K、RetroArch、3DS。
- 本机真实数据（只读）：用你的 MHGU NSP 和 prod.keys 解密 NCA 头，读出 `0100770008DD8000`，和文件名一致。`discover --folder Desktop\Switch` 列出「MONSTER HUNTER GENERATIONS ULTIMATE · Ryujinx（0100770008DD8000）」（当前用户档案的 3 号存档，2 个文件 9.8 MB，启动参数是 `Games\ROMS` 里的 NSP）和「The Legend of Zelda - Breath of the Wild · Cemu（00050000101C9300）」（20 个文件，启动参数 `-g …\U-King.rpx`）。

- `cargo test -p ludonimbus-core --test steam_shortcuts`（临时 Steam 文件夹）：新建 `shortcuts.vdf`、快捷方式 id 和 Steam 的算法一致、同名同程序时换一个 id、封面换格式时替换旧文件、删除后重新编号并删掉封面、每次写入都有备份；读取 `libraryfolders.vdf` 和已安装的商店游戏。
- `cargo test -p ludonimbus-core --test add_to_steam`（临时 Steam 文件夹和配置）：扫描结果不知道存档时拒绝添加；添加后选本地封面；加入 Steam 后快捷方式指向 `ludonimbus-run.exe --game <游戏>`、图标和封面写进 `grid`、健康检查为正常、不能重复加入；关闭自动同步后快捷方式直接启动游戏；从 Steam 移除后快捷方式和封面都删掉；移除游戏时删掉 Ludonimbus 里的封面；没有游戏程序时给出提示。
- 单元测试：Epic 清单（跳过没装完的和非游戏）、Xbox 应用的 `.GamingRoot` 和 `MicrosoftGame.config`（跳过没有自己程序的附加内容包；程序用 `gamelaunchhelper.exe`，等待的游戏进程取配置里的程序和 Content 里的 exe；选到游戏自己的程序时自动换成启动程序）、商店名称去掉「- PC」等后缀、按商店名称和 Ludusavi 清单里的安装文件夹名找到游戏位置（Program Files 里只认清单里的文件夹名）、文件夹里挑游戏 exe（排除卸载程序、运行库、崩溃上报，优先和文件夹同名、Unreal 的 Shipping）、Ludusavi 扫描结果和清单解析（含其他版本的 Steam id、安装文件夹名）、Steam 云 / 注册表存档的分类（Steam 已卸载、只剩云存档标记或 userdata 副本的游戏不再算作 Steam 云在同步）、找存档的噪音过滤（缓存、日志、着色器、`%TEMP%`、Steam 客户端自己的文件夹）、「玩一下再找」能记录到真实写入的文件夹。
- 本机真实数据（只读）：`ludonimbus discover --ludusavi --folder <Switch 文件夹>` 找到 3 个 Steam 快捷方式（都标为已添加）、文件夹里 6 个游戏、Ludusavi 72 个有存档的游戏，绝区零标成「存档在注册表里」。改进分类后，碧蓝幻想 Relink、Florence、Phantom Blade Zero（Steam 里已卸载，只剩标记）移到「可以直接添加」；空之轨迹 1st 仍算 Steam 云（以另一个 app id「Sora no Kiseki the 1st」安装）。Xbox 应用装的 DOOM Eternal 出现在扫描结果里，名称去掉了「- PC」，并对上了 Ludusavi 的名字；它自己的程序（idTechLauncher.exe、DOOMEternalx64vk.exe）读取被拒绝，只有 `gamelaunchhelper.exe` 能读，扫描结果现在用 `gamelaunchhelper.exe`，等待的进程是 idTechLauncher.exe 和 DOOMEternalx64vk.exe。
- 界面（模拟数据）：扫描列表的分组和合并（GOG 的巫师 3 带上了 Ludusavi 找到的存档）、批量添加、添加后提示加入 Steam、找存档向导四种方式、扫描文件夹、详情页「加入 Steam」「更换封面」、设置里的 SteamGridDB。

## 需要在真实环境确认的

开始前把 Steam 安装目录下的 `userdata\<账号>\config\shortcuts.vdf` 自己再备份一份。

### 1. 扫描本机

1. 「添加游戏」默认打开「扫描本机」。几秒内出现 Steam 快捷方式（所有账号）、Epic / GOG 游戏；同时显示「正在用 Ludusavi 查找…」，半分钟左右后列表补全。记一下 Ludusavi 扫描的耗时。
2. 已经添加的三个游戏在「已经添加」里，点「查看」能打开详情页。
3. 「Steam 云已经在同步」里只应该有 Steam 现在装着的游戏（每项下面写了原因），或者存档全在 Steam 云文件夹里的游戏；Steam 里已经卸载、用别的平台在玩的游戏应该在「可以直接添加」。看到分错的记下游戏名。
4. Xbox 应用装的游戏出现在列表里（来源「Xbox」），程序是 Content 文件夹里的 `gamelaunchhelper.exe`。Ludusavi 找到存档、而游戏装在 Epic / Xbox / EA 等平台的，应显示「找到游戏位置（…）」和程序路径；没装的显示「没有找到游戏程序（可能已卸载）」。
5. 勾选两三个「可以直接添加」的游戏，点「添加到 Ludonimbus」：列表不关闭，它们移到「已经添加」，游戏库里出现并开始检查状态。
6. 关掉对话框再打开：Ludusavi 结果不用重新扫描（本次运行内缓存）。
7. 「扫描文件夹…」选放游戏的文件夹（例如 `D:\Games` 或 Switch 文件夹）：识别出的游戏程序是不是对的（不是卸载程序、启动器、崩溃上报）；模拟器显示「已识别：…」。

### 2. 找存档向导

挑一个 Ludusavi 数据库里没有的游戏（或者在列表里点「指定存档…」）。

1. 「在 Ludusavi 数据库里找」：搜索英文名，选中后直接「添加」能用。
2. 「常见的存档位置」：真正的存档文件夹是否在列表前几项；有没有明显不相关的文件夹。
3. 「玩一下再找」：点「开始记录并启动游戏」，游戏应被启动；进游戏存一次档、退出后点「完成」，列表里应该有存档文件夹，没有日志、缓存、着色器缓存之类的。记下列出了哪些文件夹。
4. 选的文件夹超过 200 MB 时出现提醒。
5. 添加后同步一次，「存档文件」里的文件就是游戏的存档。

### 3. 一键加入 Steam

用一个不在 Steam 里的游戏（例如刚从文件夹扫描添加的）。

1. 先在「设置 → 封面（SteamGridDB）」填 Key（链接能打开 SteamGridDB 的 API 页面）；故意填错应提示 Key 无效。
2. 详情页「更换封面」：搜索结果正确，五种图片都能选，选中后详情页和游戏库立刻换图；「使用本地图片…」也能用。记一下 SteamGridDB 在国内打开图片的速度。
3. 详情页「加入 Steam…」：Steam 开着时会提示需要暂时关闭 Steam，确认后 Steam 关闭、写入、重新打开。
4. Steam 里出现这个游戏：名称、图标、竖版封面、横版、背景大图和 Logo 都是刚才选的；属性里的目标是 `ludonimbus-run.exe`，启动选项是 `--game <游戏>`。
5. 从 Steam 启动：先同步再启动游戏，退出后上传（看 `runner.log`）；Steam 记录游戏时间、Shift+Tab 覆盖层可用。
6. 关闭「Steam 自动同步」：快捷方式改成直接启动游戏 exe，封面不变；再打开又恢复。
7. 「从 Steam 移除快捷方式」：Steam 里这个游戏和它的封面都消失，Ludonimbus 里的游戏和封面还在，可以再次加入。
8. 批量：扫描添加三个以上游戏后点提示条里的「加入 Steam…」，开着「自动配封面」，只关闭 / 重启一次 Steam，每个游戏都有封面。
9. 移除游戏：确认框写明会一起删掉 Steam 快捷方式；移除后 Steam 里也没有了。不是 Ludonimbus 建的快捷方式（原来的三个游戏）不能「从 Steam 移除」，移除游戏时也不会被删掉。

### 4. 其他

- 已经在 Steam 里的游戏「更换封面」：Steam 重新打开后显示新封面。如果 Steam 开着时换的封面不用重启就显示了，记下来。
- 「手动添加」：选模拟器程序时显示「识别为 …」，直接添加；选普通游戏程序进入找存档向导。下面的「从游戏平台的安装目录选」应列出本机存在的 Steam 库、Xbox、Epic 等文件夹，点了直接在那里打开选择框。
- 没有游戏程序的游戏（从存档添加的）：详情页「游戏位置」点「自动查找…」，装着的游戏应能找到并确认；找不到时用下面的平台目录按钮手动选。
- Xbox 应用装的游戏（例如 DOOM Eternal）：
  1. 扫描添加后，详情页「游戏位置」是 `…\Content\gamelaunchhelper.exe`，下面写着会等游戏本身退出再上传。以前加的、程序是游戏自己 exe 的，详情页有「改用 gamelaunchhelper.exe」按钮。
  2. 加入 Steam 后从 Steam 启动：游戏能打开；玩一会儿存档后退出，Ludonimbus 在游戏退出后才上传（`%APPDATA%\Ludonimbus\logs\runner.log` 里有 `game process is running` 或 `waiting for ["idTechLauncher.exe", …] to exit`，上传时间在游戏退出之后）。
  3. 游戏开着时 Ludonimbus 主界面显示它在运行；同时开着另一个 Xbox 游戏不会让这个也显示在运行。

### 5. 模拟器和重编译游戏

1. 「扫描本机」：列表里有「MONSTER HUNTER GENERATIONS ULTIMATE」（来源「模拟器里的存档」，已识别：Ryujinx（0100770008DD8000），本机 2 个存档文件）。再「扫描文件夹…」选 `Desktop\Switch`：它不会多出一条，而是带上了 `publish\Ryujinx.exe`；还多出「The Legend of Zelda - Breath of the Wild」（Cemu（00050000101C9300））。`publish` 和 `ZeldaBW` 本身仍显示「整个模拟器的存档」。
2. 添加 MHGU（模拟器里的存档那条）后同步一次：「存档文件」只有 `nxsave-0100770008dd8000/…` 两个文件（当前用户档案的 3 号存档），没有别的游戏、`prod.keys` 或索引。「加入 Steam」后从 Steam 启动，Ryujinx 直接打开 MHGU，退出后上传。
3. 添加旷野之息（Cemu 那条）后同步一次，存档文件来自 `ZeldaBW\MLC\usr\save\00050000\101c9300`；加入 Steam 后直接启动游戏。
4. 再添加 `publish`（Ryujinx 整个模拟器）：提示「MONSTER HUNTER …」已经单独同步了。
5. 换电脑或换用户档案的恢复（有第二台电脑或愿意临时切换 Ryujinx 用户档案时再测，先关掉 Ryujinx）：在另一台电脑添加同一个游戏并同步，Ludonimbus 新建存档后打开 Ryujinx，MHGU 读到的是第一台电脑的进度；`%APPDATA%\Ludonimbus\local-backups\ryujinx-save-index` 里有旧索引的备份。Ryujinx 开着时恢复会提示先关闭它。
6. 有 Dolphin / PPSSPP / RPCS3 等模拟器的话：在 Steam 里建一个带游戏文件的快捷方式（例如 `Dolphin.exe -b -e "D:\ROMs\游戏.rvz"`，或用 Steam ROM Manager），扫描时应显示「已识别：Dolphin（游戏 ID）」，添加后只同步这个游戏的存档。只打开模拟器的快捷方式显示「整个模拟器的存档」。
7. 同一个 Eden / yuzu 等 Switch 模拟器的第二个快捷方式：扫描里显示为已经添加（指向第一个），手动添加时提示所有存档已经由它同步。
8. 「手动添加」选模拟器程序：显示「识别为 Xxx（整个模拟器的存档）」和按游戏分开同步的说明。
9. 有 RecompLauncher 的话：它装的游戏出现在扫描列表（来源「RecompLauncher」）；它建的 Steam 快捷方式（`recomplauncher.exe --play …`）显示为对应的游戏，添加后从 Steam 启动，游戏退出就上传（`runner.log` 里有 “game exited while the launcher is still open”）。
