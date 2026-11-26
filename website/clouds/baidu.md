# 百度网盘

::: info 现状
Ludonimbus 没有百度网盘的一键登录。百度网盘开放平台的应用要通过审核才能给所有人使用，这项审核没有继续做，所以设置页里没有百度网盘这一项。
:::

## 怎么用百度网盘

通过 [OpenList](https://github.com/OpenListTeam/OpenList) / AList 接入：

1. 在 OpenList 里挂载你的百度网盘。
2. 在 Ludonimbus 里用 WebDAV 连接 OpenList，文件夹写在挂载的百度网盘下面，例如 `百度网盘/Ludonimbus`。

具体步骤见 [WebDAV → OpenList / AList](./webdav.md#openlist)。OpenList 是独立的第三方开源工具，速度和稳定性取决于百度网盘和 OpenList 的驱动。

## 速度

百度网盘对非会员的下载有限速，拉取较大的存档（或[游戏本体备份](../features/game-backup.md)）时要多等一会儿。

## 其他选择

[夸克网盘](./quark.md)扫码登录就能用，[坚果云](./webdav.md#jianguoyun)填账号和应用密码就能用。以后想换云盘，存档可以一键复制过去，见[换云盘与复制存档](./switching.md)。
