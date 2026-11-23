# 封面与 SteamGridDB

Ludonimbus 的游戏库直接使用你在 Steam 里设置的封面、横幅和 Logo。不在 Steam 里的游戏、Ludonimbus 帮你加进 Steam 的游戏，可以从 [SteamGridDB](https://www.steamgriddb.com/) 社区挑选封面，也可以用本地图片。

## 填写 SteamGridDB API Key

SteamGridDB 需要一个免费的 API Key：

1. 打开 [SteamGridDB 的 API 页面](https://www.steamgriddb.com/profile/preferences/api)，用 Steam 账号登录。
2. 在「偏好设置 → API」里生成一个 Key，复制。
3. 在 Ludonimbus「设置 → 封面（SteamGridDB）」粘贴，点「保存」。

![设置 → 封面（SteamGridDB）](/screenshots/zh-CN/settings-sgdb.png)
![SteamGridDB 的 API Key 页面](/screenshots/zh-CN/sgdb-api-page.png)

Key 保存在本机的 Windows 凭据管理器里，不用时可以点「删除 Key」。不填 Key 也可以用本地图片。

## 自动配封面

[一键加入 Steam](./add-to-steam.md) 时打开「自动配封面」，Ludonimbus 会从 SteamGridDB 选评分最高的图片，已经选过的不会被替换。

## 自己挑

在游戏详情页点「更换封面」：

1. 在上面搜索游戏（找不到时换成英文名再试），选中正确的游戏。
2. 每种图片下面会列出社区上传的图片，点一下就用它。
3. 想用自己的图片，点「使用本地图片…」。

![更换封面](/screenshots/zh-CN/art-picker.png)

| 图片 | 用在哪里 |
|---|---|
| 竖版封面 | 游戏库网格里的封面 |
| 横版封面 | 最近游玩等横向位置 |
| 背景大图 | 游戏页顶部的横幅 |
| Logo | 叠在背景大图上的标题 |
| 图标 | 列表和任务栏里的小图标 |

Steam 里的封面在 Steam 重新打开后更新。

![配好封面的 Steam 游戏库](/screenshots/zh-CN/steam-library-artwork.webp)

## 隐私

只在你搜索封面时，用你的 Key 把游戏名发给 SteamGridDB，并下载你选中的图片；不会发送任何存档数据。见[隐私政策](../privacy.md)。
