# 怎么选云盘

Ludonimbus 不提供存储空间，存档放在你自己的云盘里。同一时间连接一个云盘，以后可以随时换，换的时候能一键把历史版本复制过去（见[换云盘与复制存档](./switching.md)）。

存档通常很小（几 KB 到几十 MB），旧的自动版本会定期清理，一般用不了多少空间。

## 对比

| 云盘 | 连接方式 | 适合 | 注意 |
|---|---|---|---|
| [Google Drive](./google-drive.md) | 浏览器登录 | 能访问 Google 的用户 | 免费 15 GB；只能访问 Ludonimbus 自己建的文件 |
| [夸克网盘](./quark.md) | 夸克 App 扫码 | 国内用户，已经在用夸克 | 实验性；通过 Ludonimbus 自动运行的 OpenList 连接，上传比其他云盘慢（每个版本约 10 秒） |
| [坚果云](./webdav.md#jianguoyun) | WebDAV，账号 + 应用密码 | 国内用户，最省事 | 免费版每月有上传 / 下载流量限制 |
| [群晖 / 威联通 NAS](./webdav.md#synology) | WebDAV（或 MinIO） | 家里有 NAS | 在外面用需要能从外网访问 NAS |
| [Nextcloud](./webdav.md#nextcloud) | WebDAV | 自建网盘 | 开了两步验证要用应用密码 |
| [OpenList / AList](./webdav.md#openlist) | WebDAV | 自己部署了 OpenList / AList | 第三方工具，由你自主配置；Ludonimbus 不提供网盘聚合能力 |
| [百度网盘](./baidu.md) | 百度账号授权（官方开放平台） | 国内用户 | 一键登录正在等待百度审核，暂未开放；只访问「我的应用数据/Ludonimbus」 |
| [S3 兼容存储](./s3.md) | Access Key | 阿里云 OSS、腾讯云 COS、Cloudflare R2、NAS 上的 MinIO | 按量付费（存档小，费用很低）；需要先建好 Bucket |
| [本地 / 同步文件夹](./folder.md) | 选一个文件夹 | OneDrive / Dropbox / Google Drive 电脑版的同步文件夹、NAS 共享目录、移动硬盘 | 多台电脑同步时，这个文件夹本身要能在电脑之间同步或共享 |

## 怎么挑

- **在国内，想最省事**：[坚果云](./webdav.md#jianguoyun)，填账号和应用密码就能用。
- **能用 Google**：[Google Drive](./google-drive.md)，点一下在浏览器里登录。
- **家里有 NAS**：群晖、威联通都自带 [WebDAV](./webdav.md)，存档留在自己家里。
- **已经在用 OneDrive、Dropbox 的电脑客户端**：直接选它的同步文件夹，见[本地 / 同步文件夹](./folder.md)。
- **在用夸克网盘**：[夸克网盘](./quark.md)，扫码登录就能用（实验性）。
- **想用百度网盘**：[百度网盘](./baidu.md)一键登录通过审核后会在新版本里开放。
- **已经有对象存储账号**：用 [S3 兼容存储](./s3.md)。

::: tip 多台电脑
多台电脑要连接**同一个**云盘（同一个账号、同一个服务器和文件夹），才能看到同一份存档，见[多台设备](../sync/devices.md)。只在一台电脑上用、又不想注册任何服务的话，选一个移动硬盘或 NAS 文件夹也可以。
:::
