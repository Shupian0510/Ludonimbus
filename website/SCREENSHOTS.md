# 文档站截图清单 / Planned screenshots

页面里用 `<!-- screenshot: <场景名> -->` 标出了要放截图的位置（中英文页面在同一个位置）。截图生成后放在 `public/screenshots/<语言>/<场景名>.png`（语言为 `zh-CN`、`en`），再把注释换成图片：

```md
![设置 → 云盘](/screenshots/zh-CN/settings-cloud.png)
```

Pages mark screenshot spots with `<!-- screenshot: <scene> -->` (same spot in both languages). Generated images go to `public/screenshots/<lang>/<scene>.png`; then replace the comment with the image.

## 能用示例数据自动生成的 / From the app's mock data

界面在浏览器里用示例数据运行（`app/` 里 `npm run dev`），按「场景 × 语言」截图。

| 场景名 | 页面 | 截什么 |
|---|---|---|
| `library-grid` | guide/what-is-ludonimbus | 游戏库网格视图，有几个不同状态的游戏（已同步、待上传、存档冲突） |
| `settings-cloud` | guide/quick-start | 「设置 → 云盘」，未连接时的几种云盘选项 |
| `add-game-scan` | guide/quick-start, games/scan | 「添加游戏 → 扫描本机」的结果：「可以直接添加」「需要指定存档位置」「Steam 云已经在同步」几组，带标签 |
| `game-detail` | guide/quick-start | 游戏详情页上半部分：立即同步、备份按钮，「Steam 自动同步」和「游戏位置」两栏 |
| `settings-cloud-google` | clouds/google-drive | 已连接 Google Drive 的云盘一栏（账号、已用空间、「断开」） |
| `settings-cloud-webdav` | clouds/webdav | 展开的 WebDAV 表单，点了「坚果云」预设，显示提示文字 |
| `settings-cloud-s3` | clouds/s3 | 展开的 S3 表单，点了「Cloudflare R2」预设 |
| `settings-cloud-folder` | clouds/folder | 已连接「本地 / 同步文件夹」 |
| `settings-cloud-migrate` | clouds/switching | 「要把原来云盘里的存档复制过来吗？」横幅，最好是正在复制、带进度条 |
| `save-finder` | games/save-finder | 找存档向导，四个部分都可见，「常见的存档位置」里有候选 |
| `add-game-scan-folder` | games/scan-folder | 扫描文件夹后列表里来源为「文件夹」的游戏 |
| `add-game-manual` | games/manual | 「手动添加」第一步：选择游戏程序和平台目录快捷入口 |
| `game-detail-steam` | steam/auto-sync | 「Steam 自动同步」一栏，开关打开、状态正常 |
| `add-to-steam` | steam/add-to-steam | 「加入 Steam」窗口，批量加入几个游戏，「自动配封面」打开 |
| `settings-sgdb` | steam/artwork | 「设置 → 封面（SteamGridDB）」填写 Key 的表单 |
| `art-picker` | steam/artwork | 「更换封面」窗口，搜到游戏、显示竖版封面候选 |
| `settings-steam-plugin` | steam/status-in-steam | 「在 Steam 中显示云存档状态」一栏，Millennium 和插件的状态 |
| `versions` | sync/versions | 「版本历史」标签，有几台设备、几种来源，带「最新」「本机当前」「已锁定」标记 |
| `game-detail-conflict` | sync/conflicts | 游戏详情页的冲突提示和两个按钮 |
| `game-detail-pending` | sync/offline | 「有 N 个版本等待上传」提示，状态「云端离线」 |
| `add-game-cloud` | sync/devices | 「添加游戏 → 云端已有」列表 |
| `settings` | reference/settings | 设置页整体（可以是长截图，或者「本机与同步」一栏） |
| `game-settings` | reference/settings | 游戏详情页的「设置」标签 |

## 需要在真实环境里截的 / Real environment only

示例数据做不出来，需要在装了 Ludonimbus 和 Steam 的电脑上截（中文、英文系统各一张；没有英文环境时英文页面先用中文图）。

| 场景名 | 页面 | 截什么 |
|---|---|---|
| `steam-play-bar-status` | steam/status-in-steam | Steam 游戏页「开始游戏」栏里的「Ludonimbus 云存档」状态，最好带悬停提示 |
| `steam-library-artwork` | steam/artwork | 用 SteamGridDB 配好封面的 Steam 游戏库 |
| `sgdb-api-page` | steam/artwork | SteamGridDB「偏好设置 → API」页面（遮住 Key） |
| `smartscreen-warning` | guide/install | Windows「Windows 已保护你的电脑」提示，点开「更多信息」后能看到「仍要运行」 |
| `runner-conflict` | sync/conflicts | 从 Steam 启动时的「Ludonimbus · 存档冲突」弹窗 |
| `runner-game-moved` | games/moved | 从 Steam 启动时的「Ludonimbus · 游戏位置变了」弹窗 |
