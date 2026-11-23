# 百度网盘（审核中）

::: info 现状
百度网盘一键登录已经开发完成，**正在等待百度网盘开放平台审核**，通过后会在新版本中开放。在此之前，正式版的设置页里不会出现百度网盘这一项。
:::

## 现在怎么用百度网盘

可以通过 [OpenList](https://github.com/OpenListTeam/OpenList) / AList 接入：

1. 在 OpenList 里挂载你的百度网盘。
2. 在 Ludonimbus 里用 WebDAV 连接 OpenList，文件夹写在挂载的百度网盘下面，例如 `百度网盘/Ludonimbus`。

具体步骤见 [WebDAV → OpenList / AList](./webdav.md#openlist)。速度和稳定性取决于百度网盘和 OpenList 的驱动；百度对非会员的下载有限速，拉取较大的存档时要多等一会儿。

## 开放之后

一键登录开放后，用百度账号授权就能连接，不需要 OpenList。存档会放在你网盘的「我的应用数据/Ludonimbus」文件夹里，Ludonimbus 只能访问这个文件夹。已经通过 OpenList 存了存档的话，换过去时可以一键复制，见[换云盘与复制存档](./switching.md)。
