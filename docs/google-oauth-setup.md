# 创建 Google OAuth 客户端（发布者做一次）

Ludonimbus 用 Google 官方的「桌面应用」登录方式连接 Google Drive。**只有发布者需要做下面这些步骤，而且只做一次**。做完后把 Client ID 内置进发布包，用户只需要点「连接 Google Drive」并登录自己的账号。

开发阶段还没内置时，可以把下面得到的 Client ID / Client secret 填到 Ludonimbus「设置 → 云盘 → Google Drive」里使用。

## 1. 创建项目并启用 Drive API

1. 打开 [Google Cloud Console](https://console.cloud.google.com/)，新建一个项目（例如 `Ludonimbus`）。
2. 进入「API 和服务 → 库」，搜索 **Google Drive API**，点「启用」。

## 2. 配置 OAuth 同意屏幕

进入「API 和服务 → OAuth 同意屏幕」（新版界面叫「Google Auth Platform」）：

1. 用户类型选 **外部（External）**。
2. 「Branding（品牌）」页填写：
   - App name：`Ludonimbus`
   - User support email、Developer contact information（页面最底部）：你的邮箱
   - App home page：`https://shupian0510.github.io/Ludonimbus/`
   - Application privacy policy link：`https://shupian0510.github.io/Ludonimbus/privacy.html`
   - Authorized domains：`shupian0510.github.io`
   - Logo 先不要上传（上传 Logo 会触发品牌验证）

   主页和隐私政策由仓库的 `docs/index.html`、`docs/privacy.html` 通过 GitHub Pages 发布。
3. 「数据访问 / 范围」里添加：
   - `https://www.googleapis.com/auth/drive.file`（非敏感范围：只能访问本应用创建的文件）
   - `openid`、`.../auth/userinfo.email`（用来显示已登录的账号）
4. **把发布状态改为「正式版（In production）」。**
   停在「测试」状态时，refresh token 7 天就会过期，用户每周都要重新登录。只申请上面这些非敏感范围时，切到正式版不需要提交受限范围的安全评估。

## 3. 创建 OAuth 客户端

1. 「API 和服务 → 凭据 → 创建凭据 → OAuth 客户端 ID」。
2. 应用类型选 **桌面应用（Desktop app）**。
3. 创建后记下 **Client ID** 和 **Client secret**。

桌面应用的 client secret 在 Google 的设计里本来就不是机密（它会随应用分发），但仍建议不要直接写进仓库，而是在构建时通过环境变量注入。

## 4. 把 Client ID 内置进发布包

构建时设置环境变量，Ludonimbus 会在编译时读取：

```powershell
$env:LUDONIMBUS_GOOGLE_CLIENT_ID = "xxxx.apps.googleusercontent.com"
$env:LUDONIMBUS_GOOGLE_CLIENT_SECRET = "GOCSPX-xxxx"
./scripts/prepare-sidecars.ps1 -Release
npm --prefix app run tauri build
```

在 GitHub Actions 里把它们存成仓库 Secrets（`Settings → Secrets and variables → Actions`），在构建步骤里作为环境变量传入。

## 说明

- 用户的存档会放在**他自己**的 Google Drive「Ludonimbus」文件夹里，网页版 Drive 能直接看到和下载。
- `drive.file` 范围下，不同 Google Cloud 项目之间互相看不到对方创建的文件。所以用户如果在设置里改用自己的 Client ID，就看不到之前用官方版上传的存档（反过来也一样）。
- 登录凭据（refresh token）保存在 Windows 凭据管理器里，不写入明文配置文件。
