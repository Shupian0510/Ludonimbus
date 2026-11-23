# PC 游戏（Ludusavi）

普通 PC 游戏的存档位置来自 [Ludusavi](https://github.com/mtkennerly/ludusavi) 的数据库（数据来自 PCGamingWiki），覆盖上万款游戏。Ludonimbus 的安装包附带了 Ludusavi 的官方程序。

## 怎么添加

- **扫描本机**：会用 Ludusavi 查找这台电脑上所有已经有存档的游戏（来源「找到存档」），大约需要半分钟。找到存档的游戏，Ludonimbus 还会去 Epic、GOG、Xbox、EA、Ubisoft 等平台的安装目录和常用游戏文件夹里找游戏程序。见[扫描本机](../games/scan.md)。
- **Ludusavi 数据库**：「添加游戏 → Ludusavi 数据库」里按游戏英文名搜索，适合还没扫描到、或者本机还没有存档的游戏。第一次搜索会下载数据库，稍等片刻。见[手动添加](../games/manual.md#ludusavi)。
- 数据库里没有的游戏：用[找存档向导](../games/save-finder.md)，从常见位置里挑、「玩一下再找」，或者手动指定存档文件夹。

## 平台游戏

| 平台 | 说明 |
|---|---|
| Epic、GOG | 「扫描本机」会列出它们安装的游戏 |
| Xbox 应用（PC Game Pass） | 能加进 Steam，通过游戏文件夹里的 `gamelaunchhelper.exe` 启动，见[一键加入 Steam](../steam/add-to-steam.md#xbox) |
| Steam 商店游戏 | 装在 Steam 里、Steam 云已经在同步的游戏会单独列出，一般不需要再添加 |

## 不支持的情况

- **存档在注册表里**的游戏，暂不支持同步（扫描时单独列为「存档在注册表里」）。
- Ludonimbus 只同步存档文件，不同步游戏本体。

## 找不到 Ludusavi

「设置 → 关于 → Ludusavi」显示「未找到」时，只影响从 Ludusavi 数据库添加 PC 游戏，不影响模拟器和重编译游戏。重新安装 Ludonimbus 可以修复。
