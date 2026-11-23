# 模拟器

Ludonimbus 内置了 19 个模拟器的存档规则，便携版和安装版都能找到。**只同步存档**：不碰密钥、固件、着色器缓存和即时存档（state）。

## 按游戏还是整个模拟器

| 方式 | 意思 | 什么时候用 |
|---|---|---|
| **按游戏** | 每个游戏单独一份存档和版本历史 | 模拟器支持时推荐。恢复一个游戏的旧版本不会影响其他游戏 |
| **整个模拟器** | 模拟器里所有游戏的存档一起同步 | 记忆卡是所有游戏共用的（PCSX2）、存档和本机用户档案绑定的（yuzu 系 Switch 模拟器），或者你只想简单地整体备份 |

同一个模拟器只能二选一：整个同步，或者逐个游戏同步。

### 怎么按游戏添加

- **扫描本机**：「添加游戏 → 扫描本机」会把模拟器里有存档的游戏逐个列出来（来源「模拟器里的存档」），不需要先建 Steam 快捷方式。能找到游戏文件时（Ryujinx、Dolphin 的游戏文件夹，Cemu 的游戏列表），加入 Steam 后会直接启动这个游戏。
- **带游戏文件的 Steam 快捷方式**：例如 Steam ROM Manager 生成的快捷方式。Ludonimbus 从光盘镜像、游戏文件夹或 NSP / XCI 读出游戏 ID，只同步这个游戏的存档。
- 只打开模拟器本身的快捷方式（或者手动添加时选模拟器程序），会把整个模拟器的存档一起同步。

## 一览

| 模拟器 | 平台 | 同步方式 |
|---|---|---|
| [Dolphin](#dolphin) | GameCube / Wii | 按游戏 |
| [Cemu](#cemu) | Wii U | 按游戏 |
| [Azahar、Citra、Lime3DS](#azahar) | 3DS | 按游戏 |
| [Ryujinx](#ryujinx) | Switch | 按游戏 |
| [Eden、Citron、Sudachi、Suyu、yuzu](#yuzu) | Switch | 整个模拟器 |
| [RPCS3](#rpcs3) | PS3 | 按游戏 |
| [shadPS4](#shadps4) | PS4 | 按游戏 |
| [PPSSPP](#ppsspp) | PSP | 按游戏 |
| [Vita3K](#vita3k) | PS Vita | 按游戏 |
| [PCSX2](#pcsx2) | PS2 | 整个模拟器 |
| [DuckStation](#duckstation) | PS1 | 整个模拟器 |
| [Xenia](#xenia) | Xbox 360 | 整个模拟器 |
| [RetroArch](#retroarch) | 多平台 | 按游戏（按游戏文件名） |

下面的「存档在哪」里，先列便携版的位置（模拟器程序旁边），再列安装版的默认位置；Ludonimbus 用第一个存在的。

## Dolphin {#dolphin}

- **平台**：GameCube / Wii　**方式**：按游戏
- **存档在哪**：`User\`（便携版），或 `文档\Dolphin Emulator`、`%APPDATA%\Dolphin Emulator`
  - GameCube：`GC\<地区>\Card A\*.gci`（和 `Card B`）
  - Wii：`Wii\title\…\<游戏 ID>\data`
- **注意**：GameCube 按游戏同步需要使用「GCI 文件夹」记忆卡，这是 Dolphin 的默认设置。`.raw` 记忆卡是所有游戏共用的，只能整个模拟器同步。整个同步时还包括 Mii（`Wii\shared2\menu\FaceLib`）。扫描时会读取 `Dolphin.ini` 里的游戏文件夹来找游戏文件。

## Cemu {#cemu}

- **平台**：Wii U　**方式**：按游戏
- **存档在哪**：`mlc01\usr\save\<标题 ID>`；`mlc01` 在 `portable\mlc01`、程序旁的 `mlc01` 或 `%APPDATA%\Cemu\mlc01`
- **注意**：会读取你在 Cemu 里设置的 MLC 文件夹。游戏名称和游戏文件来自 Cemu 的游戏列表。

## Azahar、Citra、Lime3DS {#azahar}

- **平台**：3DS　**方式**：按游戏
- **存档在哪**：`user\`（便携版），或 `%APPDATA%\Azahar`（`Citra`、`Lime3DS`），下面的 `sdmc\Nintendo 3DS\…\title\<标题 ID>\data`
- **注意**：按游戏同步只包含游戏自己的存档；扩展数据（extdata）是几个游戏可能共用的，只在整个模拟器同步时包含。

## Ryujinx {#ryujinx}

- **平台**：Switch　**方式**：按游戏
- **存档在哪**：`portable\`（便携版）或 `%APPDATA%\Ryujinx`，下面的 `bis\user\save\<编号>`
- **注意**：
  - 存档编号是每台电脑按创建顺序分配的，Ludonimbus 在每台电脑上查当前用户档案（Ryujinx 里最后打开的那个）的这个游戏的存档。
  - 换到另一台电脑、那里还没有这个游戏的存档时，会按 Ryujinx 的方式自动新建；**新建时需要先关闭 Ryujinx**。
  - 游戏 ID 从文件名、NSP 里的信息读出，必要时用你的 `prod.keys` 解密 NSP / XCI 读取。更新和 DLC 归到游戏本体。
  - 整个模拟器同步时包括所有存档、存档索引和用户档案。

## Eden、Citron、Sudachi、Suyu、yuzu {#yuzu}

- **平台**：Switch（yuzu 包括 torzu 等分支）　**方式**：整个模拟器
- **存档在哪**：`user\`（便携版）或 `%APPDATA%\<模拟器名>`，下面的 `nand\user\save` 和用户头像
- **注意**：这些模拟器的存档文件夹和本机的用户档案绑定，分开同步换电脑后会对不上，所以连同用户档案整个一起同步。

## RPCS3 {#rpcs3}

- **平台**：PS3　**方式**：按游戏
- **存档在哪**：RPCS3 文件夹里的 `dev_hdd0\home\00000001\savedata\<游戏 ID>…`
- **注意**：只同步第一个用户（00000001）的存档。

## shadPS4 {#shadps4}

- **平台**：PS4　**方式**：按游戏
- **存档在哪**：`user\`（便携版）或 `%APPDATA%\shadPS4`，下面的 `savedata\1\<游戏 ID>`

## PPSSPP {#ppsspp}

- **平台**：PSP　**方式**：按游戏
- **存档在哪**：`memstick\PSP\SAVEDATA\<游戏 ID>…`（便携版），或 `文档\PPSSPP\PSP\SAVEDATA`

## Vita3K {#vita3k}

- **平台**：PS Vita　**方式**：按游戏
- **存档在哪**：`ux0\`（便携版）或 `%APPDATA%\Vita3K\Vita3K\ux0`，下面的 `user\00\savedata\<游戏 ID>`

## PCSX2 {#pcsx2}

- **平台**：PS2　**方式**：整个模拟器
- **存档在哪**：`memcards\`（便携版）或 `文档\PCSX2\memcards`
- **注意**：记忆卡是所有游戏共用的，只能整体同步。

## DuckStation {#duckstation}

- **平台**：PS1　**方式**：整个模拟器
- **存档在哪**：`memcards\`（便携版），或 `文档\DuckStation\memcards`、`%LOCALAPPDATA%\DuckStation\memcards`
- **注意**：同步整个记忆卡文件夹。

## Xenia {#xenia}

- **平台**：Xbox 360　**方式**：整个模拟器
- **存档在哪**：`content\`（便携版）或 `文档\Xenia\content`
- **注意**：同步存档和用户档案；DLC 和游戏更新不同步。

## RetroArch {#retroarch}

- **平台**：多平台　**方式**：按游戏（按游戏文件名）
- **存档在哪**：RetroArch 文件夹里的 `saves\`，按游戏文件名匹配，例如 `saves\…\Super Mario World.srm`
- **注意**：只认 RetroArch 文件夹里的 `saves`；存档文件名要和游戏文件名一致（RetroArch 的默认设置）。即时存档（state）不同步。

## 想支持新的模拟器？

欢迎为 [`adapters/emulators.yaml`](https://github.com/Shupian0510/Ludonimbus/blob/main/adapters/emulators.yaml) 补充规则，见[参与贡献](../reference/contributing.md)。
