# 更新日志 / Changelog

每个版本的说明会原样显示在 GitHub Release 页面和应用内的更新提示里。每一节都必须同时包含 `### 中文` 和 `### English`，CI 每次都会检查。最上面一节是还没发布的下一个版本，随改动一点点写满。发版流程见 [docs/release-process.md](docs/release-process.md)。

Each section is shown as-is on the GitHub Release page and in the in-app update prompt. Every section must contain both `### 中文` and `### English`; CI checks this every time. The top section is the next, unreleased version and fills up as changes land. See [docs/release-process.md](docs/release-process.md).

## 1.0.0

### 中文

存档和同步

- 新增：同一个游戏的几个版本（Steam、Xbox 应用、GOG、模拟器）显示成一张卡。存档格式相同的版本自动共用同一个进度；存档放在不同位置的版本会问一次。
- 新增：Steam 商店装的游戏也能加进来。「只备份」给 Steam 云存档再留一份带历史的备份；「接管」以后由 Ludonimbus 同步，可以和其他平台、其他设备共用进度。
- 新增：独立存档。一台设备可以存自己的进度，不和其他设备共用；别的设备可以跟随它。
- 新增：Xbox 应用（Game Pass）的版本可以和 Steam 版共用进度。写入 Xbox 版是实验性的，只在你自己选择时才写。
- 新增：Switch 游戏的存档可以在 Ryujinx 和 Eden 之间通用。规则列表里的游戏（比如星露谷物语），Switch 版和 PC 版也能共用进度：第一次问一次；另一个平台用更新版本的游戏存的档不会自动换过来。
- 改进：同步更快，在慢的云盘上尤其明显：启动前检查通常只要一次请求。启动前最多等云端 10 秒；退出后上传超过 10 秒就转到后台继续，不用等着。
- 改进：判断哪一份更新不再看电脑的时钟。两台设备各有进度时一定会问你，不会悄悄覆盖。
- 改进：大存档只上传变化了的文件；上传中断后从断的地方接着传。
- 改进：写入存档是整体完成的，中途出错或断电不会留下半份存档。

「存档」页签

- 改进：「版本历史」改成「存档」。上面是当前的、每个平台最新的和保留的，自动存档折叠在下面。
- 改进：「恢复」改成「设为当前」，可以撤销。「锁定」和「备注」合成「保留」：点星标保留，可以起名字。
- 新增：把一份存档导出成 `.lnsave` 文件发给朋友；朋友导入后多出一份保留的存档，不改他本机的存档。
- 新增：删除的存档先进「最近删除」，30 天内可以放回。

游戏库和界面

- 新增：游戏库把本机装着的游戏和只有存档的游戏分开显示；存档只在云端的游戏也会列出来，带封面。
- 改进：游戏卡的封面保持干净，下面一行显示平台的标志、同步状态和游戏时间。
- 改进：界面上的文字少了，说明收在「?」里。
- 新增：在几个平台上玩过的游戏，游戏时间按平台分别列出。
- 新增：手柄界面，适合 Windows 掌机。

其他平台

- 新增：Linux 和 Steam Deck 版（预览）：桌面应用、游戏模式下的后台同步和 Decky 插件。
- 新增：Switch 上的自制软件客户端通过局域网和电脑上的 Ludonimbus 同步存档（预览）。

说明

- 从 0.6 升级不需要做什么：云端原来的文件一个都不动，游戏、存档和游戏时间都会带过来。请把所有设备都更新到 1.0：还在 0.6 的设备看不到 1.0 存的存档。
- 新增服务条款页面；隐私政策写得更完整。
- 百度网盘一键登录不再计划（没有继续做百度开放平台的审核）。百度网盘可以通过 OpenList / AList 用 WebDAV 接入。

### English

Saves and sync

- New: several versions of one game (Steam, Xbox app, GOG, emulators) show as one card. Versions whose saves have the same format share one progress automatically; a version that keeps its saves in a different place is asked about once.
- New: games installed from the Steam store can be added too. "Backup only" keeps a second copy of what Steam Cloud syncs, with history; after "Take over", Ludonimbus syncs the game, so it can share progress with other platforms and other devices.
- New: a device's own saves. A device can keep its own progress instead of sharing with the others, and another device can follow it.
- New: the Xbox app (Game Pass) version can share progress with the Steam version. Writing into the Xbox version is experimental and happens only when you choose it.
- New: saves of Switch games work in both Ryujinx and Eden. For games in the rule list (such as Stardew Valley), the Switch and PC versions can share progress too: you're asked once, and a save made by a newer version of the game on the other platform isn't brought over by itself.
- Improved: sync is faster, most of all on slow clouds: the check before launch usually takes one request. A launch waits for the cloud for at most 10 seconds; an upload that takes longer than 10 seconds after exit carries on in the background.
- Improved: which save is newer is no longer decided by the PC's clock. When two devices both have progress you are always asked; nothing is overwritten silently.
- Improved: large saves upload only the files that changed, and an interrupted upload continues where it stopped.
- Improved: saves are written as a whole; a failure or a power cut halfway never leaves half a save.

The Saves tab

- Improved: "Version history" is now "Saves". The current save, the newest of each platform and the kept ones are at the top; automatic saves are folded away below.
- Improved: "Restore" is now "Make current" and can be undone. "Lock" and "Note" became "Keep": star a save to keep it, and give it a name.
- New: export a save as a `.lnsave` file for a friend; importing it adds one kept save and changes nothing on their PC.
- New: deleted saves go to "Recently deleted" first and can be put back for 30 days.

Library and interface

- New: the library tells games installed on this PC from games that only have saves here; games whose saves are only in the cloud are listed too, with their covers.
- Improved: covers stay clean; the row under a cover shows the platforms' logos, the sync status and the play time.
- Improved: less text on screen; explanations are behind the "?" marks.
- New: for a game played on several platforms, play time is listed per platform.
- New: a gamepad interface for Windows handhelds.

Other platforms

- New: Linux and Steam Deck (preview): the desktop app, background sync in Gaming Mode and a Decky plugin.
- New: a homebrew client on the Switch syncs saves with Ludonimbus on your PC over the local network (preview).

Notes

- Upgrading from 0.6 needs nothing from you: none of the old files in the cloud are touched, and games, saves and play time carry over. Update all your devices to 1.0: a device still on 0.6 can't see saves made by 1.0.
- A terms of service page was added; the privacy policy is more complete.
- One-click Baidu Netdisk sign-in is no longer planned (Baidu's open-platform review is not being pursued). Baidu Netdisk can be connected over WebDAV through OpenList / AList.

## 0.6.2

### 中文

- 改进：没有自己图标的成就（例如根据存档判断的时之笛、旷野之息的成就）不再显示空白方块，改为显示游戏的图标：Steam 里的图标、在 Ludonimbus 里选的图标、Steam 快捷方式的图标，或者游戏程序里的图标；都没有时显示一个奖杯符号。Ludonimbus 里的成就列表、Steam 游戏页和解锁提醒都会用上。

### English

- Improved: achievements without an icon of their own (such as those Ludonimbus tells from saves for Ocarina of Time and Breath of the Wild) no longer show an empty square. They show the game's icon instead: its icon in Steam, the one chosen in Ludonimbus, the Steam shortcut's icon, or the icon built into the game's program; a trophy glyph when there is none. This applies to the achievement list in Ludonimbus, the game's Steam page and unlock popups.

## 0.6.1

### 中文

- 改进：游戏库的封面上不再叠加来源图标，封面完整可见；「显示选项」里的「显示来源标识」开关也随之去掉。列表视图仍在游戏名下方写着来源。
- 改进：分类、分组标题和列表里的标志换成模拟器、重编译和 Xbox App 自己的图标，直接从这台电脑上装好的程序里读取，不再用 Ludonimbus 自己画的机型图标；程序不在这台电脑上时只显示名字。
- 新增：「显示选项」里可以关掉「显示分类」，隐藏游戏库上方的分类。

### English

- Improved: library covers no longer have a source icon on top, so the art is fully visible; the "Show source badges" switch under View options is gone with it. The list view still shows each game's source under its name.
- Improved: categories, group headings and the list now show the emulators', recomps' and the Xbox app's own icons, read from the programs installed on this PC, instead of device icons drawn by Ludonimbus; when a program isn't on this PC, only its name is shown.
- New: turn off "Show categories" under View options to hide the categories at the top of the library.

## 0.6.0

### 中文

- 新增：游戏时间。每台电脑记录自己玩了多久（从 Steam 启动的和直接打开的都算），同步到云盘后所有设备相加；第一次使用时导入 Steam 已经记录的时间。游戏库和详情页显示总时间，鼠标悬停看各设备明细；Steam 里显示的游戏时间也换成所有设备的总时间。
- 新增：Steam 截图同步（默认关闭，在「设置 → Steam → 截图同步」打开）。从 Steam 截的图会上传到云盘；选「上传并下载」时，其他电脑截的图会放进这台电脑同一个游戏的 Steam 截图文件夹。详情页新增「截图」标签，本机和云端的截图都能看。
- 新增：在 Steam 游戏页点「Ludonimbus 云存档」，直接列出云存档的版本历史，可以恢复到任意版本，不用打开 Ludonimbus。
- 新增：游戏来源。游戏库的封面角落用图标标出每个游戏来自哪里（Dolphin、Ryujinx 等模拟器各用所模拟机型的图标，重编译 / 移植，PC 游戏，Xbox；鼠标悬停显示名字），可以按来源筛选，或在「显示选项」里按来源分组；每个分组可以一键「全部同步」「全部备份」，同一个模拟器的游戏一起管理。标识可以关掉；在游戏的「设置」里可以把游戏放进自己起名的分组（例如手动添加的模拟器游戏）。
- 新增：首次使用引导多了一步「在 Steam 中显示云存档状态」，可以在引导里直接安装 Steam 插件，也可以跳过。
- 新增：游戏本体备份。在游戏详情页的「游戏本体」卡片把游戏备份到云盘（不含存档、日志和缓存），以后只上传改动的部分。模拟器游戏分成程序、游戏文件、设置和 mods 几部分（可选再加密钥和固件），几个游戏共用的模拟器只传一次。游戏删了或换了电脑，可以在 Ludonimbus 或 Steam 游戏页「从云端安装」，装好后存档自动拉取。设置里新增「云盘测速」；百度网盘非会员下载很慢，备份前会提示。
- 新增：成就。读取 shadPS4、Lantern、RPCS3 的奖杯和 RetroAchievements 的记录，也能根据存档判断成就（Switch 游戏用社区的 TrophyNX 规则，时之笛 Longshot 用 Ludonimbus 自带的规则）。Steam 游戏页显示进度和成就列表，游戏中解锁时弹出 Steam 的成就提醒，记录在多台电脑之间同步。PS4 游戏的官方奖杯名称需要在 shadPS4 或「设置 → 成就」里填写 PS4 奖杯密钥，没有时显示编号和图标。

### English

- New: playtime. Each PC records how long you play (started from Steam or directly), and the cloud adds it up over all your devices; the minutes Steam already counted are imported the first time. The library and the game page show the total, with a per-device breakdown on hover, and the playtime Steam shows for the game becomes the total too.
- New: Steam screenshot sync (off by default; turn it on in Settings → Steam → Screenshot sync). Screenshots taken in Steam are uploaded to your cloud; with "upload and download", screenshots from your other PCs go into this PC's Steam screenshot folder of the same game. The game page has a new Screenshots tab showing screenshots on this PC and in the cloud.
- New: click "Ludonimbus Cloud Saves" on a game's page in Steam to see the version history of its cloud saves and restore any version, without opening Ludonimbus.
- New: game sources. An icon in the corner of each cover shows where the game comes from (an emulator such as Dolphin or Ryujinx, drawn as the device it emulates; a recomp or port; a PC game; Xbox), with the name on hover. Filter the library by source, or group it by source under View options; each group has "Sync all" and "Back up all", so all games of one emulator are managed together. The badges can be turned off, and a game's Settings tab can put it in a group you name (for example an emulator game you added by hand).
- New: the first-run guide has an optional step that installs the Steam plugin, so cloud save status shows in Steam.
- New: game backup. Back up a game to your cloud from the "Game files" card on its page (saves, logs and caches left out); later backups upload only what changed. Emulator games are split into the program, the game files, and settings and mods (optionally keys and firmware too), so an emulator shared by several games is uploaded once. After deleting a game or on a new PC, "Install from the cloud" in Ludonimbus or on the game's Steam page puts it back, then pulls the saves. Settings has a new cloud speed test; Baidu Netdisk without a membership downloads slowly, and you're told before backing up.
- New: achievements. Ludonimbus reads trophies from shadPS4, Lantern and RPCS3 and your RetroAchievements records, and can tell achievements from saves (community TrophyNX rules for Switch games, Ludonimbus's own rules for Ocarina of Time on Longshot). The game's Steam page shows progress and the list, Steam's achievement popup appears when one unlocks in game, and records sync between your PCs. Official PS4 trophy names need a PS4 trophy key, set in shadPS4 or in Settings → Achievements; without one you see numbers and icons.

## 0.5.0

### 中文

- 新增：夸克网盘一键连接（实验性）。在「设置 → 云盘」用夸克 App 扫码登录，Ludonimbus 会下载开源工具 OpenList（只在第一次，约 57 MB，校验 SHA-256），在本机后台运行它，自动挂载夸克并连接。存档放在网盘的「Ludonimbus」文件夹里；登录失效时扫码重新登录即可。夸克的登录信息只保存在本机，断开时删除。
- 新增：英文界面。默认跟随 Windows 的显示语言，可以在「设置 → 语言 / Language」切换，立即生效；托盘菜单、从 Steam 启动时的弹窗和 Steam 里的云存档状态也跟着切换。
- 新增：首次使用引导。第一次打开时介绍功能、问你的用途，带你连接云盘、填写 SteamGridDB API Key，最后可以直接扫描本机游戏。每一步都能跳过，在「设置 → 关于」里可以重新打开。
- 新增：使用文档网站 <https://shupian0510.github.io/Ludonimbus/>，中英双语，每个功能都有图文说明。「设置 → 关于」里加了文档、GitHub 和反馈问题的链接。
- 改进：WebDAV 服务器返回 5xx 错误时，提示里会带上服务器给出的原因。

### English

- New: one-click Quark (experimental). Scan a QR code with the Quark app in Settings → Cloud; Ludonimbus downloads the open-source OpenList (only the first time, about 57 MB, SHA-256 checked), runs it in the background on this PC, mounts Quark and connects to it. Saves go in the "Ludonimbus" folder of your drive; when the sign-in expires, scan again. The Quark sign-in stays on this PC and is deleted when you disconnect.
- New: English interface. It follows the Windows display language and can be changed in Settings → Language / 语言, right away; the tray menu, the dialogs shown when a game starts from Steam and the cloud status inside Steam follow it too.
- New: a first-run guide that introduces Ludonimbus, asks what you use it for, helps you connect a cloud and enter a SteamGridDB API key, and ends with a scan of this PC. Every step can be skipped; reopen it from Settings → About.
- New: documentation site at <https://shupian0510.github.io/Ludonimbus/> in Chinese and English, with step-by-step screenshots. Settings → About links to it, to GitHub and to issue reporting.
- Improved: WebDAV 5xx errors now include the server's message.

## 0.4.0

### 中文

- 新增：「添加游戏 → 扫描本机」一次列出 Steam 里的非 Steam 游戏、Epic / GOG / Xbox 应用安装的游戏、RecompLauncher 安装的游戏、模拟器里有存档的游戏，以及 Ludusavi 数据库里本机已经有存档的游戏，勾选后批量添加。Steam 云已经在同步的游戏单独列出。放在别处的游戏可以「扫描文件夹…」。
- 新增：找存档向导。认不出的游戏可以搜索 Ludusavi 数据库、从常见位置里挑、「玩一下再找」（启动游戏存一次档，列出刚才被写入的文件夹），或者手动选文件夹；存档很大、可能含有缓存时会提醒。
- 新增：一键加入 Steam。不在 Steam 里的游戏可以单个或批量加入 Steam，并自动开启云存档同步。Ludonimbus 建的快捷方式可以在详情页移除。
- 新增：从 SteamGridDB 自动配好或手动挑选封面、背景、Logo 和图标（需要你自己的 API Key，在设置里填写），也可以用本地图片；详情页可以随时更换封面。
- 新增：只找到存档的游戏，会自动去 Epic、GOG、Xbox、已安装的程序和常用游戏文件夹里找游戏位置；详情页的「自动查找…」也能用。选游戏程序时可以直接打开 Steam、Xbox、Epic、GOG、EA、Ubisoft 的安装目录。
- 新增：Xbox 应用（PC Game Pass）的游戏通过 `gamelaunchhelper.exe` 启动，Ludonimbus 等游戏本身退出后再上传存档。
- 新增：模拟器按游戏同步。Dolphin、Cemu、Azahar / Citra / Lime3DS、RPCS3、shadPS4、PPSSPP、Vita3K、RetroArch、Ryujinx 可以只同步一个游戏的存档：扫描会列出模拟器里有存档的游戏，Steam 快捷方式带游戏文件时自动读出游戏 ID。Ryujinx 换电脑后会自动在当前用户档案下新建存档。
- 新增：内置规则扩充到 19 个模拟器和 140 多个重编译 / 移植游戏（大部分来自 RecompLauncher 的游戏目录）；RecompLauncher 建的 Steam 快捷方式也能认出。

### English

- New: "Add game → Scan this PC" lists non-Steam shortcuts, games installed by Epic, GOG, the Xbox app and RecompLauncher, games with saves inside emulators, and games from Ludusavi's database that have saves on this PC, ready to add in bulk. Games Steam Cloud already syncs are listed apart. Games kept elsewhere can be found with "Scan folder…".
- New: a save finder for unknown games: search Ludusavi's database, pick from common locations, "play and find" (save once in the game and see the folders it wrote to), or choose a folder by hand. Very large saves that may include caches are flagged.
- New: one-click "Add to Steam" for games that are not in Steam, one by one or in bulk, with cloud saves turned on. Shortcuts made by Ludonimbus can be removed from the game page.
- New: artwork (cover, background, logo, icon) picked automatically or by hand from SteamGridDB with your own API key (set in Settings), or from local images; the cover can be changed on the game page at any time.
- New: games found by their saves are looked up in Epic, GOG, Xbox, installed programs and common game folders to find where they are installed; "Find automatically…" on the game page does the same. The program picker opens Steam, Xbox, Epic, GOG, EA and Ubisoft install folders in one click.
- New: Xbox app (PC Game Pass) games start through `gamelaunchhelper.exe`, and saves are uploaded after the game itself exits.
- New: per-game saves for emulators. Dolphin, Cemu, Azahar / Citra / Lime3DS, RPCS3, shadPS4, PPSSPP, Vita3K, RetroArch and Ryujinx can sync a single game: the scan lists games with saves inside the emulator, and shortcuts that start a game file are recognised by its game ID. On another PC, Ryujinx gets a new save for the current user profile automatically.
- New: built-in rules for 19 emulators and 140+ recomps and ports (mostly from RecompLauncher's catalog); Steam shortcuts made by RecompLauncher are recognised.

## 0.3.0

### 中文

- 新增：WebDAV 云盘。支持群晖、威联通等 NAS，坚果云、Nextcloud，也可以通过 OpenList / AList 接入百度网盘、阿里云盘、夸克等。设置页提供常用服务的填写示例，保存前可以先「测试连接」。
- 新增：S3 兼容存储，例如 NAS 上的 MinIO、阿里云 OSS、腾讯云 COS、Cloudflare R2。
- 新增：换云盘时，可以一键把原来云盘里的全部存档和历史版本复制到新云盘。中途断开也没关系，再点一次会从没复制的部分继续；原来的云盘不会被修改。
- WebDAV / S3 的密码保存在 Windows 凭据管理器里，不写入配置文件，断开时删除。
- 百度网盘一键登录已经开发完成，正在等待百度开放平台审核，通过后会在后续版本中开放。

### English

- New: WebDAV clouds: NAS (Synology, QNAP…), Jianguoyun, Nextcloud, and Chinese netdisks such as Baidu, Aliyun and Quark through OpenList / AList. The settings page has presets for common services and a "Test connection" button.
- New: S3-compatible storage such as MinIO on a NAS, Aliyun OSS, Tencent COS and Cloudflare R2.
- New: when you switch clouds, Ludonimbus offers to copy every save and its full history to the new cloud. An interrupted copy picks up where it stopped, and the old cloud is never changed.
- WebDAV / S3 passwords are kept in the Windows Credential Manager, never in the config file, and removed when you disconnect.
- One-click Baidu Netdisk sign-in is ready and waiting for Baidu's open-platform review; it will be enabled in a later update.

## 0.2.0

### 中文

- 新增：在 Steam 里显示云存档状态。在「设置 → 在 Steam 中显示云存档状态」点「一键安装」，Ludonimbus 会从 GitHub 下载并校验开源插件框架 Millennium，安装到 Steam，再启用 Ludonimbus 插件。之后 Steam 游戏页的「开始游戏」栏会显示「Ludonimbus 云存档：已是最新」这类状态，从 Steam 启动、游戏中、上传中都会实时显示，也可以直接点同步按钮，或点它打开 Ludonimbus。
- 插件只负责显示，不接触网盘账号；需要 Ludonimbus 在托盘中运行。可以随时在设置里移除插件，Ludonimbus 更新时插件也会自动更新。
- 改进：同一个游戏不会再同时运行两次同步（界面、后台监视和 Steam 里的同步按钮共用一把锁）。

### English

- New: cloud save status inside Steam. In Settings → "Show cloud save status in Steam", click "Install". Ludonimbus downloads and verifies the open-source Millennium plugin framework from GitHub, installs it into Steam and enables the Ludonimbus plugin. The play bar of a game's library page then shows the status ("Ludonimbus cloud saves: up to date"), live while launching, playing and uploading, with a sync button; clicking it opens the game in Ludonimbus.
- The plugin only displays information and never touches your cloud account; Ludonimbus must be running in the tray. The plugin can be removed in Settings at any time and is updated together with Ludonimbus.
- Improved: the same game is never synced twice at once (the app, the background watcher and the sync button in Steam share one lock).

## 0.1.2

### 中文

- 新增：「游戏进程退出就上传，不等启动器关闭」，可以在每个游戏的「设置」里开关。适合启动器会一直开着的游戏：游戏一关就上传，关闭启动器时再检查一次。塞尔达传说：时之笛（Longshot）默认开启。
- 不经过 Steam、直接打开游戏时，托盘里的后台监视也遵循这个设置。
- 游戏设置页的「游戏进程」会显示内置规则里的进程名。

### English

- New: "Upload when the game process exits, without waiting for the launcher to close", configurable per game in its settings. Useful for launchers that stay open: saves are uploaded as soon as the game closes and checked again when the launcher closes. On by default for The Legend of Zelda: Ocarina of Time (Longshot).
- The tray watcher follows the same setting for games started outside Steam.
- The "Game process" field shows the process names from the built-in rule.

## 0.1.1

### 中文

- 修复：从 Steam 启动塞尔达传说：时之笛后，退出游戏要等最长 60 秒才上传，期间 Steam 一直显示「运行中」。原因是规则把 Longshot 一直留在磁盘上的锁文件当成「游戏还在写存档」；现在只有锁文件正被占用时才等待，最多 20 秒。
- 修复：在 Steam 里点「停止」强制结束后，存档没有上传。现在托盘里的 Ludonimbus 会发现并立即补传。
- 改进：从 Steam 启动游戏前的同步更快：Google Drive 文件夹 id 缓存到本地，设备信息等记录改到游戏退出后再更新。
- 改进：启动器运行超过 20 秒时，退出后不再额外等待 8 秒。
- 改进：游戏结束后界面自动刷新同步状态；每 10 分钟重新检查一次云端。

### English

- Fixed: after playing The Legend of Zelda: Ocarina of Time from Steam, the upload waited up to 60 s after exit while Steam kept showing the game as running. The rule treated Longshot's leftover lock file as "still writing saves"; Ludonimbus now only waits while a lock file is actually in use, for at most 20 s.
- Fixed: saves were not uploaded after pressing "Stop" in Steam. The Ludonimbus tray app now notices and uploads right away.
- Improved: faster sync before launching from Steam. Google Drive folder ids are cached locally, and bookkeeping happens after the game exits.
- Improved: no extra 8 s wait after exit when the launcher ran for more than 20 s.
- Improved: statuses refresh automatically after a game exits, and every game is re-checked against the cloud every 10 minutes.

## 0.1.0

### 中文

第一个版本。

- 像 Steam 云存档一样同步非 Steam 游戏的存档：从 Steam 启动前拉取，退出后上传。
- 云盘：一键连接 Google Drive（存档在你自己的 Drive 里），或者任意文件夹（Google Drive / OneDrive 电脑版同步的文件夹、NAS、移动硬盘）。
- 版本历史：每个版本记录时间、设备名和触发方式，可以恢复、导出 zip、锁定、写备注；自动清理旧版本，手动备份永久保留。
- 冲突处理：两台设备都改过存档时让你选择，没选的那份也保留在版本历史里。
- 离线可用：连不上云端时直接启动游戏，退出后的存档先存在本机，联网后自动补传。
- 模拟器更新不会断：快捷方式只记录游戏 id；解压新版本到新文件夹时自动找到；快捷方式被改动时提醒并一键修复。
- 内置规则：塞尔达传说：时之笛（Longshot）、火焰纹章：万缕千丝（fefw recomp）、Bloodborne（Lantern Engine）；PC 游戏可以从 Ludusavi 数据库搜索添加。
- Steam 风格的游戏库（网格 / 列表），使用你在 Steam 里设置的封面；托盘后台监视不经过 Steam 启动的游戏。
- 应用内自动更新。

注意：安装包暂未进行代码签名，Windows 可能提示「未知发布者」，点「更多信息 → 仍要运行」即可。修改 Steam 快捷方式时需要暂时关闭 Steam（Ludonimbus 会自动关闭并重新打开）。

### English

First release.

- Steam-Cloud-like save sync for non-Steam games: saves are pulled before launching from Steam and uploaded after the game exits.
- Storage: one-click Google Drive (saves live in your own Drive), or any folder (a Google Drive / OneDrive desktop sync folder, a NAS share, a USB drive).
- Version history: every version records the time, device and trigger; restore it, export it as a zip, lock it or add a note. Old automatic versions are cleaned up; manual backups are kept forever.
- Conflicts: when two devices both changed the saves, you choose which one to use; the other one stays in the history.
- Works offline: if the cloud is unreachable the game starts anyway; new saves are kept locally and uploaded later.
- Emulator updates don't break sync: Steam shortcuts only store a game id, a new version extracted into a new folder is found automatically, and edited shortcuts are detected and repaired with one click.
- Built-in rules: The Legend of Zelda: Ocarina of Time (Longshot), Fire Emblem: Fortune's Weave (fefw recomp), Bloodborne (Lantern Engine). PC games can be added from the Ludusavi database.
- Steam-style library (grid / list) using your Steam custom artwork; a tray watcher handles games started outside Steam.
- In-app auto-update.

Note: the installer is not code-signed yet, so Windows may show "Unknown publisher"; click "More info → Run anyway". Changing Steam shortcuts requires closing Steam briefly (Ludonimbus closes and reopens it).
