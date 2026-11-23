# WebDAV：坚果云、NAS、Nextcloud、OpenList

WebDAV 是很多网盘和 NAS 都支持的标准协议。坚果云、群晖、威联通、Nextcloud 都自带 WebDAV；百度网盘、阿里云盘、夸克等可以先挂载到 OpenList / AList，再通过它的 WebDAV 接入。

## 通用步骤

1. 打开「设置 → 云盘」，在 WebDAV 那一项点「填写 WebDAV 信息…」。
2. 在「常用」里点你用的服务（坚果云、群晖 NAS、OpenList / AList、Nextcloud），Ludonimbus 会填好地址的格式，并在下面显示这个服务的注意事项。
3. 填写：
   - **地址**：WebDAV 地址。尖括号 `<…>` 里的部分要换成你自己的，例如 `<NAS 地址>` 换成 `192.168.1.10`。
   - **用户名**、**密码**。
   - **文件夹**：存档放在哪个文件夹，默认 `Ludonimbus`。不存在会自动创建。
4. 点「测试连接」，确认能连上、能写入。
5. 点「连接」。

<!-- screenshot: settings-cloud-webdav -->

密码保存在本机的 Windows 凭据管理器里，不会写进配置文件，断开时删除。之后要改地址或密码，在同一个地方改好点「保存修改」；密码栏留空表示不修改。

::: tip 局域网 NAS 和 HTTPS
局域网 NAS 没有受信任的 HTTPS 证书（例如自签名证书）时，「测试连接」会报证书错误，请改用 `http://` 地址。
:::

## 坚果云 {#jianguoyun}

| 项目 | 填写 |
|---|---|
| 地址 | `https://dav.jianguoyun.com/dav/` |
| 用户名 | 坚果云账号邮箱 |
| 密码 | **应用密码**，不是登录密码 |
| 文件夹 | `Ludonimbus`（会在坚果云根目录下创建） |

生成应用密码：登录坚果云网页版，打开「账户信息 → 安全选项 → 第三方应用管理」，添加一个应用（名字随便填，例如 Ludonimbus），复制生成的密码。

坚果云免费版每月有上传和下载流量限制，存档不大的话一般够用。

## 群晖 Synology {#synology}

1. 在 DSM 的「套件中心」安装并启用「WebDAV Server」，打开 HTTPS（默认端口 5006）或 HTTP（默认端口 5005）。
2. 在 Ludonimbus 里点「群晖 NAS」：

| 项目 | 填写 |
|---|---|
| 地址 | `https://<NAS 地址>:5006/`；证书不受信任时用 `http://<NAS 地址>:5005/` |
| 用户名 / 密码 | DSM 账号 |
| 文件夹 | `<共享文件夹>/Ludonimbus`，例如 `home/Ludonimbus` |

文件夹要放在一个你有读写权限的共享文件夹下面。

## 威联通 QNAP {#qnap}

1. 在 QTS 打开「控制台 → 网络和文件服务 → Win/Mac/NFS/WebDAV → WebDAV」，勾选「启用 WebDAV」。权限可以选「共享文件夹权限」。
2. 记下这一页显示的 HTTP / HTTPS 端口（可能使用 Web 服务器的端口，也可能是单独设置的端口）。
3. 在 Ludonimbus 里填写：

| 项目 | 填写 |
|---|---|
| 地址 | `http://<NAS 地址>:<端口>/` |
| 用户名 / 密码 | QTS 账号 |
| 文件夹 | `<共享文件夹>/Ludonimbus`，例如 `Public/Ludonimbus` |

## Nextcloud {#nextcloud}

| 项目 | 填写 |
|---|---|
| 地址 | `https://<服务器>/remote.php/dav/files/<用户名>/` |
| 用户名 | Nextcloud 用户名 |
| 密码 | 登录密码；开启了两步验证时用应用密码 |
| 文件夹 | `Ludonimbus` |

应用密码在 Nextcloud 的「个人设置 → 安全 → 设备与会话」里创建。

## OpenList / AList {#openlist}

[OpenList](https://github.com/OpenListTeam/OpenList)（AList 的社区分支）可以把百度网盘、阿里云盘、夸克等网盘挂载成一个 WebDAV 服务。这些网盘没有面向个人开发者的官方接口（百度网盘的一键登录还在审核中），用 OpenList 是目前接入它们的办法。

1. 安装并运行 OpenList（Windows 程序或 Docker 都可以），打开它的管理页面，按 OpenList 的文档添加你的网盘存储，记下挂载路径（例如 `/夸克`）。
2. 用来连接的 OpenList 用户要有 WebDAV 的读取和管理权限（管理员账号默认都有）。
3. 在 Ludonimbus 里点「OpenList / AList」：

| 项目 | 填写 |
|---|---|
| 地址 | `http://<地址>:5244/dav/`，OpenList 装在本机时 `<地址>` 写 `127.0.0.1` |
| 用户名 / 密码 | OpenList 账号 |
| 文件夹 | `<挂载的网盘>/Ludonimbus`，例如 `夸克/Ludonimbus` |

注意：

- 文件夹一定要写在挂载的网盘下面，否则存档只会存在 OpenList 本机。
- **同步时 OpenList 必须在运行**。装在本机的话，建议设成开机自动启动；没运行时 Ludonimbus 会当作云端暂时无法访问，存档先留在本机，之后再补传（见[离线游玩](../sync/offline.md)）。
- 速度和稳定性取决于对应网盘和 OpenList 的驱动；有的网盘对非会员下载限速。
- 用 OpenList 和夸克时，存档会经过你自己运行的 OpenList，再到夸克网盘，不经过任何第三方服务器。

## 连不上？ {#troubleshooting}

| 现象 | 可能的原因 |
|---|---|
| 401 / 用户名或密码错误 | 坚果云、Nextcloud（两步验证）要用应用密码 |
| 证书错误 | 局域网 NAS 没有受信任的证书，改用 `http://` |
| 404 / 地址不存在 | 地址少了路径，例如坚果云要以 `/dav/` 结尾，OpenList 要以 `/dav/` 结尾 |
| 403 / 没有权限 | 文件夹不在你有写权限的共享文件夹下面；OpenList 用户没有 WebDAV 管理权限 |
| 超时 | 不在同一个局域网，或者 NAS 没有开放到外网 |

更多见[常见问题与故障排查](../reference/faq.md)。
