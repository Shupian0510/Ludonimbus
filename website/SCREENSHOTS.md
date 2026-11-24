# 文档站截图清单 / Planned screenshots

页面里用 `<!-- screenshot: <场景名> -->` 标出了要放截图的位置（中英文页面在同一个位置）。截图生成后放在 `public/screenshots/<语言>/<场景名>.png`（语言为 `zh-CN`、`en`），再把注释换成图片：

```md
![设置 → 云盘](/screenshots/zh-CN/settings-cloud.png)
```

Pages mark screenshot spots with `<!-- screenshot: <scene> -->` (same spot in both languages). Generated images go to `public/screenshots/<lang>/<scene>.png`; then replace the comment with the image.

## 能用示例数据自动生成的 / From the app's mock data

这些截图由主仓库的 `scripts/docs-screenshots.ps1` 生成：它用浏览器预览的示例数据（`app/` 的 `npm run dev`），通过 `?scene=<场景名>&lang=<语言>` 直接打开对应画面（见 `app/src/scene.ts`），再用 Windows 自带的 Edge 无头模式截图。界面改了就重跑一次：

These are made by `scripts/docs-screenshots.ps1` in the main repository: it opens each scene in the browser preview's sample data (`?scene=<scene>&lang=<lang>`, see `app/src/scene.ts`) and captures it with headless Edge. Re-run it after UI changes:

```powershell
./scripts/docs-screenshots.ps1 -Out website/public/screenshots
```

| 场景名 | 页面 |
|---|---|
| `library-grid` | guide/what-is-ludonimbus |
| `settings-cloud`、`add-game-scan`、`game-detail` | guide/quick-start |
| `guide-welcome`、`guide-usage`、`guide-cloud`、`guide-art`、`guide-steam`、`guide-done` | guide/first-run |
| `settings-cloud-google` | clouds/google-drive |
| `settings-cloud-quark` | clouds/quark |
| `settings-cloud-webdav` | clouds/webdav |
| `settings-cloud-s3` | clouds/s3 |
| `settings-cloud-folder` | clouds/folder |
| `settings-cloud-migrate` | clouds/switching |
| `add-game-scan` | games/scan |
| `save-finder` | games/save-finder |
| `add-game-scan-folder` | games/scan-folder |
| `add-game-manual` | games/manual |
| `game-detail-steam` | steam/auto-sync |
| `add-to-steam` | steam/add-to-steam |
| `settings-sgdb`、`art-picker` | steam/artwork |
| `settings-steam-plugin` | steam/status-in-steam |
| `versions` | sync/versions |
| `game-detail-conflict` | sync/conflicts |
| `game-detail-pending` | sync/offline |
| `add-game-cloud` | sync/devices |
| `settings`、`settings-language`、`game-settings` | reference/settings |

## 需要在真实环境里截的 / Real environment only

示例数据做不出来，需要在装了 Ludonimbus 和 Steam 的电脑上截（中文、英文系统各一张；没有英文环境时英文页面先用中文图）。

已有：`quark-login-window`、`steam-play-bar-status`、`steam-library-artwork`（.webp）、`sgdb-api-page`，放在 `public/screenshots/zh-CN/`，英文页面暂时共用。`steam-play-bar-status` 以后可以补一张 Ludonimbus 切到 English 时的英文版。暂缺：`smartscreen-warning`（截图的电脑关闭了 SmartScreen）、`runner-conflict`、`runner-game-moved`。

| 场景名 | 页面 | 截什么 |
|---|---|---|
| `quark-login-window` | clouds/quark | 「登录夸克网盘」窗口：夸克网盘官方网页的扫码登录（可以遮住二维码） |
| `steam-play-bar-status` | steam/status-in-steam | Steam 游戏页「开始游戏」栏里的「Ludonimbus 云存档」状态，最好带悬停提示 |
| `steam-library-artwork` | steam/artwork | 用 SteamGridDB 配好封面的 Steam 游戏库 |
| `sgdb-api-page` | steam/artwork | SteamGridDB「偏好设置 → API」页面（遮住 Key） |
| `smartscreen-warning` | guide/install | Windows「Windows 已保护你的电脑」提示，点开「更多信息」后能看到「仍要运行」 |
| `runner-conflict` | sync/conflicts | 从 Steam 启动时的「Ludonimbus · 存档冲突」弹窗 |
| `runner-game-moved` | games/moved | 从 Steam 启动时的「Ludonimbus · 游戏位置变了」弹窗 |
