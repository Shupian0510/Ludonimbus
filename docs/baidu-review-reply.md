# 百度网盘开放平台复审：补充材料

这份文件分两部分：

- **第一部分**：可以直接粘贴到工单里的回复正文。`【待填写】` 的地方要你自己填。
- **第二部分**：提交前你要做的事（推送文档站、出测试包、录屏等）。不要粘贴到工单里。

---

## 第一部分：回复正文

审核老师您好，感谢详细的反馈。按工单要求一次性补充如下。本次相关改动已经在新版本（0.6.3）中完成，测试安装包见第 9 项。

### 1. 隐私政策 URL

https://shupian0510.github.io/Ludonimbus/privacy.html#baidu

（页面先英文后中文，中文全文从 #zh 开始；「百度网盘」一节在 #baidu。）

百度网盘一节覆盖：

- 收集与处理的数据范围：授权凭据、网盘昵称、容量（只显示不保存），以及用户选择同步的存档数据，逐项列明用途和保存位置；
- 授权方式：OAuth 2.0 设备码模式，在百度官方授权页面确认，权限 basic、netdisk；
- 撤销授权入口：应用内「断开」，以及百度账号「账号设置 → 授权管理」；
- 应用侧数据删除方式：「最近删除」30 天可恢复、自动清理规则、删除全部数据的方法。

### 2. 服务条款 URL

https://shupian0510.github.io/Ludonimbus/terms.html

### 3. 首次授权流程录屏

已使用未授权状态的百度账号重新录制完整授权流程，附件：【待填写：视频文件名】。

视频包含：点击「连接百度网盘」→ 应用显示授权码和二维码 → 百度授权页面展示的权限清单 → 用户点击确认授权 → 应用显示连接成功 → 首次同步后在百度网盘「我的应用数据/Ludonimbus」中看到存档。视频已脱敏（见文末）。

### 4. 撤销授权指引

应用内（附截图：【待填写】）：

- 「设置 → 云盘 → 百度网盘」中新增可展开的「如何解除授权？」说明，连接前和连接后都能看到，内容为：
  1. 在 Ludonimbus 里点「断开」，删除本机保存的百度授权凭据，之后不再访问网盘；
  2. 在百度一侧：登录百度账号，打开「账号设置 → 授权管理」（https://passport.baidu.com/v6/appAuthority），找到 Ludonimbus，点「取消授权」，所有电脑上的授权立即失效；说明里有按钮直接打开该页面；
  3. 解除授权不会删除网盘里的存档，如需删除可直接删除「我的应用数据/Ludonimbus」文件夹。
- 点「断开」时的确认框也会提示到百度账号「授权管理」解除授权。

文档：隐私政策 #baidu-revoke，以及使用文档 https://shupian0510.github.io/Ludonimbus/clouds/baidu.html#revoke 。

### 5. 删除能力边界说明

**书面确认**：Ludonimbus 的所有删除操作都只作用于应用自己写入「我的应用数据/Ludonimbus」（/apps/Ludonimbus/）目录下的文件，不涉及用户网盘内的任何其他文件。

会发生删除的场景（全部在 /apps/Ludonimbus/ 内）：

1. 用户在「版本历史」里删除某个存档版本：**改为软删除**（见下）。
2. 自动保留策略：按用户在设置里定的规则（保留最近 N 个、最近 N 天每天 1 个）删除较旧的**自动**存档版本；锁定的版本、手动备份、最新版本和本机当前版本永远不会被自动删除。
3. 连接测试：连接时写入一个临时文件 .ludonimbus-test-xxx，读回校验后立即删除。
4. 用户主动使用的可选功能：替换自定义封面时删除旧封面文件；游戏本体备份按用户设置清理旧备份及不再被引用的数据块（/apps/Ludonimbus/gamebackup/）；测速功能删除自己写入的测速临时文件（.ludonimbus-speed-xxx）。

技术保证：

- 所有请求路径都由应用拼成「/apps/<应用名>/ + 相对路径」，本地会去掉路径中的 `.` 和 `..`，应用名也不能包含 `/`，任何路径都不可能指向应用目录以外（有单元测试覆盖）；
- 应用不调用任何以用户其他目录为参数的接口，不使用搜索、递归列表、分类列表、复制、移动、重命名等接口；
- 百度网盘开放平台在服务端同样限制 filemanager 只能操作 /apps/{appname}。

**误删恢复方案（新版本已实现）**：

- 原先的「永久删除，无法恢复」改为「最近删除」软删除：用户删除某个版本时，只把这个版本的记录移到 /apps/Ludonimbus/games/<游戏>/trash/，存档包保留不动；该版本从所有设备的版本历史中消失，但 **30 天内可在「最近删除」里一键恢复**。
- 30 天到期后自动彻底删除；用户也可以在「最近删除」里手动「彻底删除」，此时有二次确认，提示「彻底删除后无法恢复」。
- 恢复的自动版本会同时被锁定，避免被保留策略再次清理。
- 其他保护：删除前二次确认；最新版本和锁定版本不能删除；恢复存档前会自动把本机当前存档备份成「恢复前保护」版本；任何版本都可以「导出 zip」另存。
- 游戏本体备份（可选功能）的版本是完整游戏文件，体积通常为数 GB，可随时从本机安装目录重新备份，因此删除时直接删除，确认框会说明删除的是哪一次备份。

### 6. 「使用自己的百度应用（高级）」凭证安全说明

- **SecretKey**：只保存在用户本机的 Windows 凭据管理器中（由 Windows 按当前登录用户加密保存），不写入配置文件。界面上以密码框显示，应用设置接口只返回「是否已保存」的占位符，不把 SecretKey 回传给界面。断开连接或换用其他应用时删除。
- **AppKey 和应用名称**：保存在本机配置文件 %APPDATA%\Ludonimbus\config.json 中，它们不属于机密信息。
- **是否上报服务端**：不上报。开发者不运营任何服务器。这三项只通过 HTTPS 发送给百度：设备码请求（openapi.baidu.com，AppKey 作为 client_id）和获取 / 刷新令牌请求（openapi.baidu.com，AppKey 与 SecretKey）。
- **授权令牌**（access token / refresh token）：同样只保存在 Windows 凭据管理器中，断开时删除。
- 说明：0.6.2 及以前的版本把自填的 SecretKey 写在本机配置文件里；0.6.3 起改为保存在凭据管理器，并会在启动时自动把旧配置里的 SecretKey 迁移到凭据管理器、从配置文件中清除。
- 发布包内置的应用 AppKey / SecretKey 在构建时从 GitHub Actions 加密变量注入，不在源代码仓库中。

### 7. API 与权限必要性说明

授权范围只申请 basic、netdisk。**不申请用户级文件删除权限，不申请全盘访问权限。** 所有文件类接口的目录范围都限定在 /apps/Ludonimbus/ 及其子目录。

| # | 接口 | 功能对应 | 目录范围 |
|---|---|---|---|
| 1 | openapi.baidu.com/oauth/2.0/device/code | 获取设备码、授权码和二维码，发起授权 | 无 |
| 2 | openapi.baidu.com/oauth/2.0/token（grant_type=device_token） | 用户确认后换取令牌（间隔 ≥ 6 秒轮询） | 无 |
| 3 | openapi.baidu.com/oauth/2.0/token（grant_type=refresh_token） | 令牌到期前刷新 | 无 |
| 4 | pan.baidu.com/rest/2.0/xpan/nas?method=uinfo | 读取网盘昵称，在应用里显示当前连接的账号（返回的其他字段不保存、不使用） | 无 |
| 5 | pan.baidu.com/api/quota | 在设置页显示已用 / 总空间 | 无 |
| 6 | xpan/file?method=list | 列出存档版本、设备记录、游戏定义 | /apps/Ludonimbus/ 及子目录 |
| 7 | xpan/file?method=create（isdir=1） | 创建存放存档的子文件夹 | /apps/Ludonimbus/ 下 |
| 8 | xpan/file?method=precreate | 上传存档：预上传 | /apps/Ludonimbus/ 下 |
| 9 | d.pcs.baidu.com/rest/2.0/pcs/file?method=locateupload | 上传存档：获取上传域名 | /apps/Ludonimbus/ 下 |
| 10 | pcs/superfile2?method=upload | 上传存档：4 MB 分片上传 | /apps/Ludonimbus/ 下 |
| 11 | xpan/file?method=create | 上传存档：合并分片、创建文件 | /apps/Ludonimbus/ 下 |
| 12 | xpan/multimedia?method=filemetas（dlink=1） | 恢复存档时获取下载链接（fsid 只来自第 6 项列出的应用目录文件） | /apps/Ludonimbus/ 下 |
| 13 | 下载链接 dlink | 恢复存档、在其他电脑上拉取存档 | /apps/Ludonimbus/ 下 |
| 14 | xpan/file?method=filemanager&opera=delete | 第 5 项列出的删除场景 | 仅 /apps/Ludonimbus/ 下 |

未使用的接口：复制、移动、重命名、搜索、递归列表（listall）、分类文件列表、分享等。

### 8. 应用主体材料

- 开源仓库：https://github.com/Shupian0510/Ludonimbus （公开）
- 开源许可证：MIT（https://github.com/Shupian0510/Ludonimbus/blob/main/LICENSE ）
- ICP 备案号：【待确认：无。本应用是 Windows 桌面软件，开发者不运营服务器；官网和文档托管在 GitHub Pages（shupian0510.github.io），未办理 ICP 备案。】
- 发布包签名信息：
  - 安装包由 GitHub Actions 从公开源码自动构建，发布在 GitHub Releases：https://github.com/Shupian0510/Ludonimbus/releases
  - 自动更新包使用 minisign 签名（每个安装包附带 .sig 文件），公钥内置在应用中，密钥 ID 995F8B999BF6FCB8；签名校验不通过的更新不会安装。
  - Windows Authenticode 代码签名：【待填写：申请中（SignPath Foundation 开源代码签名）/ 暂未签名】
  - 测试安装包 SHA-256：【待填写】
- 安全联系人：Shupian0510，【待填写：安全联系邮箱】；也可以通过 GitHub 私密漏洞报告提交：https://github.com/Shupian0510/Ludonimbus/security/advisories/new

### 9. 测试账号与测试路径

Ludonimbus 没有自己的账号体系，审核人员使用任意百度账号授权即可，无需测试账号。（应用仍在审核状态时，只有前 10 个完成授权的百度账号可以使用；如审核账号遇到错误码 20011，请告知。）

测试安装包（已内置本应用的 AppKey）：【待填写：下载链接】，SHA-256 见第 8 项。系统要求 Windows 10 / 11 64 位。

测试路径：

1. 安装并打开 Ludonimbus，首次引导可以点「跳过引导」。
2. 准备测试存档：新建文件夹 C:\LudonimbusTest，在里面放一个任意文本文件（例如 save.txt）。
3. 连接百度网盘：「设置 → 云盘 → 百度网盘」，可以先展开「如何解除授权？」查看说明，再点「连接百度网盘」。浏览器打开百度授权页面，应用内同时显示授权码和二维码；在百度页面登录、查看权限清单并确认。完成后应用自动显示已连接。
4. 添加游戏：「游戏库 → 添加游戏 → 手动添加」，名称任意，存档位置选 C:\LudonimbusTest，保存。
5. 在游戏页点「立即同步」。在百度网盘网页版或 App 的「我的应用数据/Ludonimbus/games/」下可以看到上传的存档（.zip 和 .json）。
6. 修改 save.txt 后再点一次「立即同步」，「版本历史」里出现第二个版本。
7. 删除与恢复：在「版本历史」里删除较旧的那个版本（确认框提示移到「最近删除」，30 天内可恢复）→ 点列表下方的「最近删除」→ 点「恢复」，版本回到历史中并被锁定。点锁形图标解锁后可以再次删除，然后在「最近删除」里点垃圾桶图标「彻底删除」（有二次确认）。
8. 撤销授权：「设置 → 云盘」点「断开」（确认框会提示到百度账号「授权管理」解除授权）；再打开 https://passport.baidu.com/v6/appAuthority 取消 Ludonimbus 的授权。

### 关于另外两点提示

1. **演示材料脱敏**：新的演示视频在专门用于录屏的 Windows 账户中录制，未登录 Steam，画面中不出现 Steam 账号名和个人用户目录路径。
2. **WebDAV 文案**：已修改应用内的 WebDAV 说明和 OpenList / AList 预设提示、使用文档的 WebDAV / 百度网盘 / 常见问题页面以及项目首页，删除了「通过 OpenList/AList 接入百度网盘」的写法，并明确说明：Ludonimbus 只是标准的 WebDAV 客户端，不提供网盘聚合或挂载能力；连接哪个服务器、是否使用 OpenList 等第三方工具，属于用户自主配置行为；Ludonimbus 只通过百度网盘开放平台的官方接口接入百度网盘。隐私政策和服务条款中也有同样的说明。

如还需要其他材料，请随时告知，谢谢！

---

## 第二部分：提交前你要做的事（不要粘贴）

1. **提交并推送**这次的改动。文档站（隐私政策、服务条款、百度网盘页）只有推送到 main 之后才会由 `docs.yml` 部署上线；推送后打开第 1、2 项的两个链接确认能访问。
2. **安全联系方式**：仓库的 GitHub 私密漏洞报告已于 2026-11-25 开启，隐私政策和条款里链接的就是它。第 8 项里还要填一个邮箱，请你决定用哪个。
3. **ICP 备案**：确认第 8 项的写法是否符合实际。
4. **出测试包**：确认 `secrets.local.env` 里有 `LUDONIMBUS_BAIDU_*` 三项（已有），运行 `./scripts/build-release.ps1`，看到「Baidu Netdisk app will be built in.」。把安装包传到审核人员能下载的地方（例如 GitHub 预发布版本或网盘分享），用 `Get-FileHash <安装包> -Algorithm SHA256` 算出 SHA-256 填进第 8、9 项。
5. **录屏（第 3 项）**：
   - 先让账号回到「未授权」状态：打开 https://passport.baidu.com/v6/appAuthority ，确认列表里能看到 Ludonimbus 并「取消授权」。**如果列表里没有 Ludonimbus**，说明百度网盘的授权不在这里管理，先别提交，把你看到的页面告诉我，第 4 项的位置要改。
   - 脱敏：新建一个 Windows 本地账户（例如叫 Demo）专门录屏，用户目录就是 C:\Users\Demo；这个账户里不要登录 Steam，也不要打开设置页的 Steam 部分。浏览器只开百度授权页这一个标签。
   - 按第 9 项第 3–8 步录一遍，重点停留在百度授权页的权限清单和确认按钮上。
6. **截图（第 4 项）**：设置页百度网盘一项展开「如何解除授权？」的截图，以及点「断开」时的确认框。
7. 填完所有 `【待填写】`，再粘贴第一部分。
