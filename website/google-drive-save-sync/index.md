---
title: 用 Google Drive 同步游戏存档
description: 用你自己的 Google Drive 存放游戏存档：非 Steam 游戏和模拟器自动同步，版本历史是普通的 zip 文件，Ludonimbus 只能访问它自己创建的文件。
---

# 用 Google Drive 同步游戏存档

很多人都有一个带 15 GB 免费空间的 Google 账号，而游戏存档都不大，每个游戏通常只有几 KB 到几十 MB。Ludonimbus 把**你自己的 Google Drive** 当作非 Steam 游戏、模拟器和重编译游戏的云存档，从 Steam 启动游戏时自动同步。

[下载 Windows 版](https://github.com/Shupian0510/Ludonimbus/releases/latest) · [Google Drive 连接说明](../clouds/google-drive.md)

::: tip 网络
需要能访问 Google 的网络。连不上 Google 的话，可以选[夸克网盘](../clouds/quark.md)、[坚果云](../clouds/webdav.md#jianguoyun)等其他云盘，用法完全一样。
:::

## 为什么不直接把存档放进 Drive 同步文件夹？

Google Drive 桌面版可以同步一个文件夹，只有一个游戏时也许够用。但用来同步游戏存档，它有几个缺口：

| | Drive 同步文件夹 | Ludonimbus + Google Drive |
|---|---|---|
| 开始游戏前拿到最新存档 | 要看文件夹有没有及时同步完 | 从 Steam 启动前先拉取 |
| 什么时候上传 | 文件一变就传，可能存档写到一半 | 游戏和启动器都退出后 |
| 存档在别的文件夹 | 要自己挪动或链接每个存档文件夹 | 自动找到每个游戏的存档位置 |
| 两台电脑都改了存档 | 单个文件出现冲突副本 | 让你选保留哪一边，另一边留在历史里 |
| 历史版本 | 按单个文件 | 每次游戏一个完整存档版本，记录设备和时间 |
| 模拟器 | 整个模拟器文件夹 | 大部分模拟器按游戏同步，不碰密钥和固件 |

## 一分钟连好

1. 打开「设置 → 云盘」，点**连接 Google Drive**（首次使用引导里也有）。
2. 浏览器会打开 Google 的登录页面，选你的账号并同意授权。
3. 回到 Ludonimbus，云盘一栏会显示「已连接」、你的账号和已用空间。

![设置 → 云盘：已连接 Google Drive](/screenshots/zh-CN/settings-cloud-google.png)

接着添加游戏并开启 Steam 自动同步，见[快速上手](../guide/quick-start.md)。

## Ludonimbus 能看到你 Drive 里的什么

- **只能访问 Ludonimbus 自己创建的文件**（Google 的 `drive.file` 权限），看不到也改不了你 Drive 里的其他任何文件。
- 登录凭据保存在本机的 Windows 凭据管理器里。
- 中间没有 Ludonimbus 的服务器：存档直接在你的电脑和 Google 之间传输。

详见[隐私政策](../privacy.md)。

## 在 Drive 里是什么样

存档在 **Ludonimbus** 文件夹里，网页版 Drive 也能直接查看和下载：

```text
Ludonimbus/devices/<设备>.json            每台电脑一个小文件
Ludonimbus/games/<游戏>/game.json         游戏信息
Ludonimbus/games/<游戏>/snapshots/*.zip   每个版本一个 zip
```

每个版本都是普通的 zip，即使不用 Ludonimbus 也能拿回存档。旧的自动版本会自动清理，手动备份和锁定的版本永久保留。

## 多台电脑

每台电脑都登录**同一个 Google 账号**。在另一台电脑上开始游戏时，会先拉取最新存档；如果两台电脑都玩过、中间没同步，Ludonimbus 会问你保留哪一份。见[多台设备](../sync/devices.md)。

## 断开连接

「断开」只会停止这台电脑的同步并删除它的登录凭据，存档仍留在 Drive 里。要彻底撤销授权，在 [myaccount.google.com/permissions](https://myaccount.google.com/permissions) 里移除 Ludonimbus；要删除所有云端数据，删掉 Drive 里的 Ludonimbus 文件夹即可。

## 不用 Google Drive？

同样的同步也能用在其他存储上，以后换云盘时可以把全部历史一起复制过去（见[换云盘与复制存档](../clouds/switching.md)）：

- **夸克网盘**：手机扫码登录（实验性），见[夸克网盘](../clouds/quark.md)。
- **坚果云、群晖等 NAS、Nextcloud**：通过 [WebDAV](../clouds/webdav.md) 连接。
- **阿里云 OSS、腾讯云 COS、Cloudflare R2**：[S3 兼容存储](../clouds/s3.md)。
- **OneDrive、Dropbox**：选它们的桌面同步文件夹，见[本地 / 同步文件夹](../clouds/folder.md)。

各云盘的比较见[怎么选云盘](../clouds/index.md)。
