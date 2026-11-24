生效日期：2026 年 11 月 22 日

Ludonimbus 是一款开源的 Windows 桌面应用，用来备份和同步游戏存档，完全在你的电脑上运行。开发者不运营任何服务器，也不收集、接收或保存你的任何数据。

## 访问哪些 Google 用户数据

如果你选择连接 Google Drive，Ludonimbus 会申请以下权限：

- `drive.file`：只能创建、读取、修改和删除 **Ludonimbus 自己创建的文件**，看不到也改不了你 Drive 里的其他任何文件。
- `openid`、`email`：在应用里显示当前连接的是哪个 Google 账号。

## 数据怎么使用

- Ludonimbus 会在你的 Google Drive 里创建名为 `Ludonimbus` 的文件夹，存放：游戏存档的压缩副本、每个版本的信息（时间、文件名、大小、校验值、可选备注），以及每台电脑一个小文件（设备名、操作系统、应用版本）。
- 这些文件只用于实现应用本身的功能：恢复存档、显示版本历史、在你自己的多台电脑之间同步。
- 你的邮箱地址只会显示在应用的设置页里。

## 数据怎么保存

- 数据通过 HTTPS 在你的电脑和 Google 服务器之间直接传输，不经过开发者的任何服务器。
- Google 登录凭据保存在你电脑的 Windows 凭据管理器里；应用设置和同步状态保存在本机的 `%APPDATA%\Ludonimbus`。

## 共享

Ludonimbus 不会出售、共享或向任何人转交你的数据，不包含广告、统计分析或遥测。

Ludonimbus 对通过 Google API 获得的信息的使用和传输，遵守 [Google API 服务用户数据政策](https://developers.google.com/terms/api-services-user-data-policy)，包括其中的"有限使用"要求。

## 你的选择

- 随时可以在 Ludonimbus「设置 → 云盘」里断开 Google Drive，这会删除你电脑上保存的登录凭据。
- 在 [myaccount.google.com/permissions](https://myaccount.google.com/permissions) 撤销应用的访问权限。
- 删除 Google Drive 里的 `Ludonimbus` 文件夹，即可删除应用存储的全部数据。
- 如果你使用本地或同步文件夹而不是 Google Drive，则不涉及任何 Google 数据。
- 如果你连接百度网盘，Ludonimbus 使用百度网盘官方开放平台（权限 `basic,netdisk`），只能读写你网盘里的「我的应用数据/Ludonimbus」文件夹。登录凭据保存在 Windows 凭据管理器里，断开时删除。删除这个文件夹即可删除应用在百度网盘里存储的全部数据。
- 如果你连接夸克网盘，Ludonimbus 会在你的电脑上运行开源工具 OpenList（只监听本机），通过它读写你网盘里的「Ludonimbus」文件夹。夸克的登录信息只保存在本机 OpenList 的数据文件夹里，存档只在你的电脑和夸克之间传输；断开时会删除这些登录信息。
- 如果你使用 WebDAV 服务器或 S3 兼容存储，存档只会传到你填写的服务器。它的密码或密钥保存在你电脑的 Windows 凭据管理器里，断开时会删除。
- 如果你填写了 SteamGridDB API Key 来挑选封面，Ludonimbus 会用你的 Key 把你搜索的游戏名发给 SteamGridDB，并下载你选中的图片。Key 保存在 Windows 凭据管理器里，可以在设置里删除。不会向 SteamGridDB 发送任何存档数据。

## 变更与联系

本政策的变更会发布在本页面和 [GitHub 仓库](https://github.com/Shupian0510/Ludonimbus)中。有问题请在 [GitHub Issues](https://github.com/Shupian0510/Ludonimbus/issues) 提出。
