生效日期：2026 年 11 月 25 日（2026 年 11 月 22 日版的更新：补充各种云盘的说明、「最近删除」和本机保存的数据）

Ludonimbus 是一款开源的桌面应用（Windows 为主，也支持 Linux / Steam Deck），用来备份和同步游戏存档，完全在你的电脑上运行。开发者不运营任何服务器，也不收集、接收或保存你的任何数据。应用不包含广告、统计分析或遥测。

你选择的云盘只用来存放你自己的存档。存档在你的电脑和云盘服务商的服务器之间通过 HTTPS 直接传输，不经过开发者或任何第三方的服务器。

## Google Drive

如果你选择连接 Google Drive，Ludonimbus 会申请以下权限：

- `drive.file`：只能创建、读取、修改和删除 **Ludonimbus 自己创建的文件**，看不到也改不了你 Drive 里的其他任何文件。
- `openid`、`email`：在应用里显示当前连接的是哪个 Google 账号。

数据怎么使用：

- Ludonimbus 会在你的 Google Drive 里创建名为 `Ludonimbus` 的文件夹，存放：游戏存档的压缩副本、每个版本的信息（时间、文件名、大小、校验值、可选备注），以及每台电脑一个小文件（设备名、操作系统、应用版本）。
- 这些文件只用于实现应用本身的功能：恢复存档、显示版本历史、在你自己的多台电脑之间同步。
- 你的邮箱地址只会显示在应用的设置页里。
- 在 Ludonimbus 里删除的存档版本会移到 Google Drive 的回收站，30 天内可以在 Ludonimbus 的「最近删除」里恢复，之后由 Google 自动清除。

数据怎么保存：数据通过 HTTPS 在你的电脑和 Google 服务器之间直接传输，不经过开发者的任何服务器。Google 登录凭据保存在你电脑的 Windows 凭据管理器里。

Ludonimbus 对通过 Google API 获得的信息的使用和传输，遵守 [Google API 服务用户数据政策](https://developers.google.com/terms/api-services-user-data-policy)，包括其中的"有限使用"要求。

你的选择：

- 随时可以在 Ludonimbus「设置 → 云盘」里断开 Google Drive，这会删除你电脑上保存的登录凭据。
- 在 [myaccount.google.com/permissions](https://myaccount.google.com/permissions) 撤销应用的访问权限。
- 删除 Google Drive 里的 `Ludonimbus` 文件夹，即可删除应用存储的全部数据。

## 百度网盘 {#baidu}

Ludonimbus 没有内置的百度网盘应用。只有当你用自己在百度网盘开放平台创建的应用连接百度网盘时，以下内容才适用；网盘里的文件夹和百度授权管理里显示的名称是你的应用名称，下面以 Ludonimbus 为例。

### 授权方式

- Ludonimbus 通过**百度网盘开放平台**的官方接口接入，使用 OAuth 2.0 设备码授权：Ludonimbus 显示一个授权码和二维码，你在百度的官方授权页面（`openapi.baidu.com`）或百度网盘 App 里登录百度账号、查看权限清单并点击确认。
- 你的百度账号和密码只在百度的页面上输入，Ludonimbus 看不到也不保存。
- 申请的权限范围是 `basic`（基础信息）和 `netdisk`（网盘），以授权页面上实际展示的为准。Ludonimbus 没有申请、也不使用全盘访问或用户级文件删除权限。

### 收集和处理哪些数据

| 数据 | 用途 | 保存在哪里 |
|---|---|---|
| 授权凭据（access token / refresh token） | 代表你访问百度网盘 | 只保存在你电脑的 Windows 凭据管理器里，断开时删除 |
| 网盘昵称 | 在设置页显示当前连接的账号 | 本机配置文件 `%APPDATA%\Ludonimbus\config.json` |
| 网盘已用 / 总容量 | 在设置页显示空间用量 | 不保存，每次打开设置页时读取 |
| 你选择同步的存档等数据（见下） | 备份、恢复、在你自己的多台电脑之间同步 | 你百度网盘的「我的应用数据/Ludonimbus」文件夹 |

读取账号信息的接口还会返回会员类型等字段，Ludonimbus 不保存也不使用这些字段。

「我的应用数据/Ludonimbus」文件夹（网盘里的路径是 `/apps/Ludonimbus`）里存放的是：

- 游戏存档的压缩副本，以及每个版本的信息（时间、设备名、文件名、大小、校验值、可选备注）；
- 每台电脑一个小文件（设备名、操作系统、应用版本）和每个游戏的定义（游戏名、存档位置规则）；
- 你开启了对应功能时：游戏时长、Steam 截图、自定义封面、成就记录、游戏本体备份。

### 访问范围

- Ludonimbus **只读写**「我的应用数据/Ludonimbus」这一个文件夹，不会列出、读取、修改或删除你网盘里的任何其他文件。
- 百度网盘开放平台也在服务端限制应用只能访问自己的目录；Ludonimbus 在本机同样会去掉路径里的 `..`，任何路径都不能指向这个文件夹以外。

### 如何撤销授权 {#baidu-revoke}

1. **在 Ludonimbus 里**：打开「设置 → 云盘」，点「断开」。Ludonimbus 会删除这台电脑上保存的百度授权凭据，之后不再访问你的百度网盘。
2. **在百度一侧**：登录百度账号，打开「账号设置 → 授权管理」（[passport.baidu.com/v6/appAuthority](https://passport.baidu.com/v6/appAuthority)），找到 Ludonimbus，点「取消授权」。取消后，任何电脑上保存的 Ludonimbus 授权都会立即失效。

Ludonimbus 设置页的百度网盘一项里，「如何解除授权？」也写了这两步，并能直接打开百度的授权管理页面。

### 如何删除数据 {#baidu-delete}

- **删除某个存档版本**：在游戏的「版本历史」里点删除，这个版本会移到「最近删除」，**30 天内可以恢复**，到期后自动彻底删除；也可以在「最近删除」里手动「彻底删除」。
- **自动清理**：Ludonimbus 按你在设置里定的保留规则（例如保留最近 20 个、最近 30 天每天 1 个）删除较旧的自动版本。锁定的版本、手动备份的版本和最新版本永远不会被自动清理。
- **删除全部数据**：在百度网盘里删除「我的应用数据/Ludonimbus」文件夹，即可删除 Ludonimbus 在百度网盘里存储的全部数据。撤销授权本身不会删除这些文件。
- 以上删除都只发生在「我的应用数据/Ludonimbus」文件夹内。

### 你的百度应用的密钥 {#baidu-own-app}

连接时要填写你的应用的 AppKey、SecretKey 和应用名称：

- **SecretKey** 只保存在你电脑的 Windows 凭据管理器里（Linux 上是系统密钥环；没有密钥环时是只有你的账户能读的本机文件），不写进配置文件；断开或换用其他应用时删除。
- **AppKey 和应用名称**保存在本机配置文件里，它们不是机密信息。
- 这三项只会发给百度（登录和刷新授权时发给 `openapi.baidu.com`），不会发给 Ludonimbus 开发者或任何第三方服务器。

## 其他云盘和在线服务

- **本地或同步文件夹**：存档只写进你选择的文件夹，不涉及任何在线服务。
- **WebDAV / S3 兼容存储**：存档只会传到你自己填写的服务器；连接哪个服务器、服务器背后是什么，由你自己配置。密码或密钥保存在 Windows 凭据管理器里，断开时删除。
- **夸克网盘（实验性）**：Ludonimbus 会在你的电脑上运行开源工具 OpenList（只监听本机，只挂载夸克网盘），通过它读写你网盘里的「Ludonimbus」文件夹。夸克的登录信息只保存在本机 OpenList 的数据文件夹里，断开时删除。
- **SteamGridDB**（可选）：如果你填写了 SteamGridDB API Key 来挑选封面，Ludonimbus 会用你的 Key 把你搜索的游戏名发给 SteamGridDB，并下载你选中的图片。Key 保存在 Windows 凭据管理器里，可以在设置里删除。不会发送任何存档数据。
- **GitHub**：Ludonimbus 会连接 GitHub 检查自己的新版本，并让 Ludusavi 从 GitHub 下载游戏数据库。这些请求不包含任何存档数据或账号信息。

## 本机保存的数据

应用设置、游戏列表和同步状态保存在本机的 `%APPDATA%\Ludonimbus`（Linux 上是 `~/.config/Ludonimbus`），日志只记录在本机、不会上传。卸载 Ludonimbus 并删除这个文件夹，即可清除本机的全部数据；各类登录凭据可以在 Windows「凭据管理器」里搜索 `Ludonimbus` 删除。

## 共享与转让

Ludonimbus 不会出售、共享或向任何人转交你的数据。开发者没有服务器，也就没有任何可以共享的数据。

## 未成年人

Ludonimbus 不面向特定年龄群体，不收集任何人的个人信息，也不会专门处理未成年人的信息。

## 变更与联系

本政策的变更会发布在本页面和 [GitHub 仓库](https://github.com/Shupian0510/Ludonimbus)中，页面顶部的生效日期会随之更新。一般问题请在 [GitHub Issues](https://github.com/Shupian0510/Ludonimbus/issues) 提出；安全问题请通过 [GitHub 私密漏洞报告](https://github.com/Shupian0510/Ludonimbus/security/advisories/new)提交，不要公开发布。另见[服务条款](./terms.md)。
