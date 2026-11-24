# 成就

给模拟器、重编译和移植游戏补上成就：读取模拟器的奖杯记录或 RetroAchievements 的记录，或者根据存档判断；在 Steam 游戏页显示进度，游戏中解锁时弹出 Steam 的成就提醒，记录在多台电脑之间同步。

![游戏详情页的「成就」标签](/screenshots/zh-CN/game-achievements.png)

## 支持的来源

| 来源 | 适用的游戏 | 说明 |
|---|---|---|
| shadPS4、Lantern Engine | PS4 游戏 | 读取模拟器的奖杯记录。Bloodborne（Lantern）打开 Ludonimbus 后自动关联 |
| RPCS3 | PS3 游戏 | 读取 RPCS3 的奖杯记录 |
| PS4 移植版 | 从 PS4 版移植的 PC 游戏 | 读取移植版文件夹里原版游戏的奖杯文件 |
| RetroAchievements | RetroArch、DuckStation、PCSX2、PPSSPP、Dolphin 等支持 RetroAchievements 的模拟器 | 在「设置 → 成就」连接你的 RetroAchievements 账号 |
| 存档规则（TrophyNX） | Ryujinx 上的 Switch 游戏、Switch 重编译游戏、Cemu 上的旷野之息 | 根据存档内容判断。规则来自社区项目 TrophyNX，运行时下载，离线时用缓存 |
| 存档规则（Ludonimbus） | 时之笛（Longshot） | Ludonimbus 自带的规则 |

存档规则不带成就图标，这些成就显示游戏自己的图标：Steam 里的图标、在 Ludonimbus 里选的图标，或者游戏程序里的图标；都没有时显示一个奖杯符号。

## 关联成就来源

打开游戏详情页的「成就」标签：

- **自动关联**：来源很明确的游戏（例如 Lantern 上的 Bloodborne、有存档规则的游戏）打开 Ludonimbus 后自动关联。你取消过关联的不会再自动关联。
- **找到的奖杯数据**：列出 Ludonimbus 在本机找到的来源，点「关联」。
- **RetroAchievements**：「列出最近玩过的游戏」后选一个，或者填游戏 ID（游戏页网址里的数字）。
- **选择奖杯文件…**：RPCS3 的 `TROPUSR.DAT` / `TROPCONF.SFM`、shadPS4 的奖杯 XML、Lantern 的 `trophies.log`。

关联后可以「刷新」或「取消关联」。取消关联不会删掉云端已有的记录。

## 在 Steam 里

装了 [Steam 插件](../steam/status-in-steam.md) 后，Steam 游戏页和 Steam 自己的游戏放在同样的位置：「开始游戏」栏有「成就 x/y」和进度条，右栏最上面有「成就」区块。点任意一处列出全部成就，未解锁的显示为灰色。

### 解锁提醒

游戏中解锁成就时，弹出 Steam 自己的成就提醒（同样的样式和音效，游戏里显示在 Steam 界面上）。Steam 没开或者弹不出来时，可以改用 Ludonimbus 自己的小窗口，它只在窗口化或无边框全屏的游戏上面看得见。

- 「设置 → 成就」里有「测试弹窗」和「8 秒后测试」（切回游戏看）。
- 每个游戏的「成就」标签可以单独关掉提醒。
- 模拟器自己也会弹通知的话，在模拟器里关掉它的，免得弹两次（「成就」标签里有各模拟器的提示）。
- 需要 Ludonimbus 在托盘里运行。

## 多台电脑

每台电脑只写自己的记录，云盘里是 `Ludonimbus/games/<游戏>/achievements/<设备>.json`。在另一台电脑上解锁的成就也会显示，写着「在 … 上解锁」；同一个成就取最早的解锁时间。

## PS4 游戏的奖杯名称 {#ps4-trophy-key}

PS4 奖杯的名称和说明在游戏自带的加密文件里，读取需要 **PS4 奖杯密钥**（和 shadPS4 用的是同一个）。Ludonimbus 不附带这个密钥：

- shadPS4 的设置里已经填了 `TrophyKey` 时，自动使用它；
- 否则在「设置 → 成就 → PS4 奖杯密钥」里填写（32 个十六进制字符）。密钥只保存在这台电脑上，不会上传。

没有密钥时，PS4 奖杯显示为编号和图标，进度、解锁提醒和同步都照常。

![设置 → 成就](/screenshots/zh-CN/settings-achievements.png)
