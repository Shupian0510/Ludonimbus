# Google Drive

用你自己的 Google 账号，存档放在你的 Google Drive 里。

## 连接

1. 打开「设置 → 云盘」，在 Google Drive 那一项点「连接 Google Drive」。
2. 浏览器会打开 Google 的登录页面，选你的账号并同意授权。
3. 回到 Ludonimbus，云盘一栏会显示「已连接」、你的账号和已用空间。

<!-- screenshot: settings-cloud-google -->

需要能访问 Google 的网络。连不上 Google 的话，可以选[坚果云](./webdav.md#jianguoyun)等其他云盘。

## Ludonimbus 能看到什么

- **只能访问 Ludonimbus 自己创建的文件**（Google 的 `drive.file` 权限），看不到也改不了你 Drive 里的其他任何文件。
- 存档在你 Drive 里的「Ludonimbus」文件夹，网页版 Drive 也能直接查看和下载。每个版本是一个普通的 zip。
- 登录凭据保存在本机的 Windows 凭据管理器里。

详见[隐私政策](../privacy.md)。

## 重新连接、断开

- 登录过期或想换账号：点「重新连接 Google Drive」。
- 不再使用：点云盘一栏右边的「断开」。这只会让本机不再同步并删除本机的登录凭据，云端已有的存档和版本都会保留。
- 想彻底撤销授权：在 [myaccount.google.com/permissions](https://myaccount.google.com/permissions) 里移除 Ludonimbus。
- 想删除所有云端数据：在 Google Drive 里删除「Ludonimbus」文件夹。

## 使用自己的 Client ID（高级）

正式版已经内置了 Google 登录需要的 OAuth 客户端，一般用户不需要管。如果你想用自己在 Google Cloud 创建的「桌面应用」类型客户端，可以点「使用自己的 Client ID（高级）」，填入 Client ID 和 Client secret 后再连接。
