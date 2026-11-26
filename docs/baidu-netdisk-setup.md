# 用自己的百度网盘应用连接（开发版）

Ludonimbus 能通过百度网盘官方的开放平台连接百度网盘，登录用的是「设备码模式」：点「连接百度网盘」，在浏览器里登录百度账号并授权，或者用百度网盘 App 扫码，不需要回调地址。

**官方构建没有内置百度应用**：给所有用户用的应用要通过百度的上线审核，这项审核在 2026 年 11 月决定不做。所以发布版的设置页不显示百度网盘这一项，普通用户通过 OpenList / AList 的 WebDAV 使用百度网盘（见网站的「云盘 → 百度网盘」）。这份文档写给开发者：在开发版里用自己创建的应用连接，或者给自己的构建内置一个应用。

官方文档：[创建应用](https://pan.baidu.com/union/doc/使用入门/创建应用/)、[设备码模式授权](https://pan.baidu.com/union/doc/使用入门/接入授权/设备码模式授权/)。

## 「应用用途」选哪个

| | 个人使用 | 公开发布 |
|---|---|---|
| 审核 | 不需要，创建后立即可用 | 需要提交上线审核 |
| 用户数 | 最多 10 个 | 审核通过后不限；审核期间只有前 10 个完成授权的用户能用（错误码 20011） |

自己用选「个人使用」就够了。**个人开发者只能创建 1 个应用**，「应用类别」创建后不能修改。

## 1. 开发者认证

用你的百度账号登录 [百度网盘开放平台](https://pan.baidu.com/union)，点「申请接入」：

- **接入类型选「个人」**（适用于个人开发者，最多可创建 1 个应用，Ludonimbus 只需要 1 个）。「企业」需要企业、学校等组织的资质，最多 10 个应用，个人项目用不上。
- 然后按页面提示完成实名认证。

## 2. 创建应用

打开 [控制台 → 应用列表](https://pan.baidu.com/union/console/applist)，点「创建应用」：

- **应用类别**：软件
- **应用用途**：个人使用（见上一节）
- **应用名称**：`Ludonimbus`（最多 12 个字符，只能用中英文、数字、空格、`-`、`_`）。
  **这个名称就是用户网盘里「我的应用数据/Ludonimbus」文件夹的名字**，Ludonimbus 只能读写这个文件夹。如果你用了别的名称，构建时要把 `LUDONIMBUS_BAIDU_APP_NAME` 设成完全一样的名称（第 5 步）。
- **应用描述**：至少 30 个字符，同样只能用中英文、数字、空格、`-`、`_`（**不能有逗号、句号等标点**）。可以直接用：

  ```
  Ludonimbus 是开源的游戏存档云同步工具 为 Steam 里的模拟器和第三方游戏提供类似 Steam 云存档的自动备份和多设备同步 存档只保存在用户自己网盘的应用目录里
  ```

## 3. 记下密钥

进入应用详情，记下：

- **AppKey**（OAuth 里的 client_id）
- **SecretKey**（OAuth 里的 client_secret）

AppId 和 SignKey 用不到。「安全设置」里的授权回调地址也不用填，设备码模式不需要回调。

## 4. 连接

任选一种：

- **桌面应用**：在没有内置百度应用的版本里（例如 `npm run tauri dev`），打开「设置 → 云盘 → 百度网盘」，填 AppKey、SecretKey、应用名称，点「连接百度网盘」。
- **命令行**：

  ```powershell
  cargo run -p ludonimbus-cli -- cloud baidu --app-key <AppKey> --secret-key <SecretKey> --app-name Ludonimbus
  ```

浏览器会打开百度的授权页面，界面上同时显示授权码和二维码。授权后 Ludonimbus 会在网盘里写入、列出、读回再删除一个测试文件，都成功才算连接完成。之后网盘的「我的应用数据」里会出现 `Ludonimbus` 文件夹。

常见问题：

| 提示 | 原因 |
|---|---|
| 百度网盘不允许访问这个位置（20020 / 31064 / -7） | 填的应用名称和开放平台里登记的不一样 |
| 应用还在审核中，只有前 10 个授权的用户可以使用（20011） | 公开发布的应用还没通过审核 |
| 应用没有这个接口的权限（20013） | 公开发布的应用审核时没有申请这个接口 |
| 百度网盘授权失败：unauthorized_client 等 | 应用不能使用设备码模式，或 AppKey / SecretKey 填错 |
| 等待授权超时 | 5 分钟内没有完成授权，重新点连接即可 |

授权过程中收到的非预期回复会记在 `%APPDATA%\Ludonimbus\logs\baidu.log`。

## 5. 内置进自己的构建（可选）

在仓库根目录的 `secrets.local.env`（已被 git 忽略，不会提交）里加上：

```
LUDONIMBUS_BAIDU_APP_KEY=你的 AppKey
LUDONIMBUS_BAIDU_SECRET_KEY=你的 SecretKey
LUDONIMBUS_BAIDU_APP_NAME=Ludonimbus
```

然后运行 `./scripts/build-release.ps1`，脚本会提示「Baidu Netdisk app will be built in.」。

GitHub Actions 里把这三个值存成同名的仓库 Secrets（`Settings → Secrets and variables → Actions`）。`ci.yml` 和 `release.yml` 已经会把它们传给构建步骤。

和 Google 的 client secret 一样，桌面应用里的 SecretKey 会随安装包分发，本来就不算机密，但仍然不要写进仓库。

## 说明

- 用户的存档只放在他自己网盘的「我的应用数据/<应用名称>」里，网页版和 App 都能看到和下载。
- 登录凭据保存在 Windows 凭据管理器里。access token 有效期 30 天，refresh token 10 年，每次刷新后旧的 refresh token 立即失效；Ludonimbus 会在多个进程之间加锁刷新并马上保存新的。
- 普通用户上传分片固定 4 MB、单个文件最大 4 GB，存档远远够用。
- 非会员的下载会被百度限速（实测约 80 KB/s），上传不受影响。
