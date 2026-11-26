# Windows 代码签名 / Code signing

安装包用 [SignPath Foundation](https://signpath.org/) 的免费开源代码签名：签名服务由 SignPath.io 提供，证书颁发给 SignPath Foundation，所以文件属性里的发布者显示为「SignPath Foundation」，不是个人名字。

`release.yml` 在配置好 SignPath 之后自动签名；没配置时照旧发布未签名的安装包，发版流程不受影响。

## 签了哪些文件

| 文件 | 签名 |
|---|---|
| `ludonimbus-app.exe`（Ludonimbus 主程序） | 签 |
| `ludonimbus-run.exe`（Steam 快捷方式启动的包装器） | 签 |
| `Ludonimbus_X.Y.Z_x64-setup.exe`（安装包） | 签 |
| `ludusavi.exe` | 不签：它是 Ludusavi 官方发布的程序，SignPath 只签项目自己构建的文件 |

NSIS 安装包里的文件没法事后再签，所以每次发版要发**两个**签名请求：先签两个程序，再用签好的程序打安装包，最后签安装包。更新签名（`.sig`）在 Authenticode 签名之后生成，覆盖的是最终的安装包。

开发版（`ci.yml` 的 `windows-build`）和候选版（手动运行 `release.yml`）不签名。

## 1. 申请（一次性）

准备：

- GitHub 账号和之后的 SignPath 账号都要开两步验证，这是 SignPath Foundation 的要求。
- 首页（README）和下载页（文档站「下载与安装」）要有「Code signing policy」一节，申请表明确要求下载页写明用 SignPath Foundation 签名。这两处已经写好，提交、推送、文档站部署之后再去申请。

在 <https://signpath.org/apply> 填表，建议的填法：

| 字段 | 填什么 |
|---|---|
| Project Name | `Ludonimbus` |
| Repository URL | `https://github.com/Shupian0510/Ludonimbus` |
| Homepage URL | `https://shupian0510.github.io/Ludonimbus/en/` |
| Download URL | `https://shupian0510.github.io/Ludonimbus/en/guide/install.html`（这页有 Code signing policy） |
| Privacy Policy URL | `https://shupian0510.github.io/Ludonimbus/en/privacy.html` |
| Tagline | `Open-source Steam Cloud alternative for non-Steam games, emulators and recompiled games.` |
| Category | 选最接近的一项（例如 Utilities / Games） |
| Description | `Ludonimbus syncs game saves for non-Steam games on Windows (emulators, recompiled games, other launchers) to the user's own cloud storage: Google Drive, WebDAV, S3-compatible storage and more. It backs up saves automatically when a game exits, keeps version history, and integrates with Steam so a game launched from Steam pulls its saves first and uploads them afterwards.` |
| Reputation | 如实写：GitHub stars、Release 下载量、winget 包 `Shupian0510.Ludonimbus`、文档站、在社区里的讨论链接（有的话） |
| Maintainer Type | Individual |
| Build System | GitHub Actions |
| 姓名 / Email | 你自己的，会用来创建 SignPath 账号 |

> [!WARNING]
> SignPath Foundation 会看项目的知名度，条款里说他们没法给「没人知道的源码」签名，也没有义务接受申请。Ludonimbus 刚发布不久，有被拒或被要求过一段时间再申请的可能。被拒的话，备选是 Certum 的开源代码签名证书（约 €49，证书上是「Open Source Developer, 你的名字」）。

## 2. 通过后配置

SignPath 会给你开一个项目（在 SignPath Foundation 的组织下）。在 SignPath 里：

1. 记下 **Organization ID**（组织设置里）、**项目 slug**、**签名策略 slug**（一般是 `release-signing`）。
2. 确认项目已关联 **GitHub.com** 这个 Trusted Build System（一般 SignPath 会帮你设好）。
3. 把 [`.signpath/artifact-configuration.xml`](../.signpath/artifact-configuration.xml) 的内容设为项目的默认 Artifact Configuration。以后改这个文件也要同步到 SignPath。
4. 创建一个 API token：给一个账号（可以单独建一个 CI 用户）在 `release-signing` 策略上的 Submitter 权限，生成它的 API token。

在 GitHub 仓库 Settings → Secrets and variables → Actions 里：

| 类型 | 名称 | 值 |
|---|---|---|
| Secret | `SIGNPATH_API_TOKEN` | 上面生成的 API token |
| Variable | `SIGNPATH_ORGANIZATION_ID` | Organization ID |
| Variable | `SIGNPATH_PROJECT_SLUG` | 项目 slug，是 `Ludonimbus` 时可以不设 |
| Variable | `SIGNPATH_SIGNING_POLICY_SLUG` | 签名策略 slug，是 `release-signing` 时可以不设 |

`SIGNPATH_API_TOKEN` 和 `SIGNPATH_ORGANIZATION_ID` 都有了，下一次发版就会签名。

第一个签名版本发布后，再改用户看得到的说明：README 和文档站「下载与安装」里「安装包暂未签名」的提示，改成「安装包由 SignPath Foundation 签名，新版本刚发布时 SmartScreen 仍可能提示」之类。

## 3. 每次发版

推送标签后，Actions 会在两个地方停下等你批准：

1. **Sign programs**：日志里有签名请求的链接，或者直接在 SignPath 网页上找到它，点 Approve。
2. 大约一两分钟后是 **Sign installer**，再批准一次。

每个请求最多等 2 小时，超时这次运行就失败，没有 Release 产生；在 Actions 里重新运行即可（会发新的签名请求）。发布前流程会检查每个文件的签名都有效。

发布后可以右键安装包 → 属性 → 数字签名，确认签名者是 SignPath Foundation。

签名不能马上消除 SmartScreen 提示：每个新版本的安装包刚发布时，下载量还少，Windows 仍可能提示，随着下载变多会消失。

---

## English summary

- Installers are signed through SignPath Foundation's free open-source program: the certificate is issued to SignPath Foundation, which shows as the publisher.
- `release.yml` signs once the `SIGNPATH_API_TOKEN` secret and the `SIGNPATH_ORGANIZATION_ID` variable exist (optional variables: `SIGNPATH_PROJECT_SLUG`, default `Ludonimbus`; `SIGNPATH_SIGNING_POLICY_SLUG`, default `release-signing`). Without them it publishes unsigned installers as before.
- Signed: `ludonimbus-app.exe`, `ludonimbus-run.exe` and the NSIS installer. Not signed: `ludusavi.exe`, an upstream release. Files inside an NSIS installer can't be signed afterwards, so each release sends two signing requests (programs, then the installer built from them), and each needs a manual approval in SignPath (the action waits up to 2 hours). The updater `.sig` is made after Authenticode signing.
- The SignPath artifact configuration is kept in `.signpath/artifact-configuration.xml`; keep it in sync with the project's default configuration in SignPath. SignPath Foundation requires product name and version restrictions, so every signed program carries Windows version info (`crates/runner/build.rs` adds it to `ludonimbus-run.exe`).
- The home page (README) and the download page (docs site, install guide) must keep their "Code signing policy" sections.
- Application: <https://signpath.org/apply>. SignPath Foundation weighs project reputation and may decline young projects; the fallback is a Certum open-source code-signing certificate.
