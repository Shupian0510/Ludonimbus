# 发布管线 / Release pipeline

[English summary](#english-summary)

一个改动从分支到用户手里要经过的全部步骤。怎么提改动见 [CONTRIBUTING.md](../CONTRIBUTING.md)。

## 总览

```text
feat/x ──PR──▶ dev ──────── 发版 PR ────────▶ main ──标签 vX.Y.Z──▶ Release
               │ 每次合并                      ▲
               ▼                               └── hotfix/x、docs/x（PR）
          开发版安装包
     （Actions 构建产物，不是发布）
               │ 发版前手动运行 Release 工作流
               ▼
          候选版安装包
  （和正式版同一套流程，不发布）
```

- **`main` 是已发布的版本。** 项目首页（README）、文档站、下载页都跟着它，所以它们描述的永远是用户现在能下载到的东西。
- **`dev` 是下一个版本。** 所有新改动先进 `dev`；`dev` 上的版本号就是下一个要发布的版本号，`CHANGELOG.md` 里那一节随着改动一点点写满。
- 两个分支都只能通过 PR 修改，CI 的 `ok` 检查通过才能合并。

## 三种构建

| | 什么时候 | 版本号 | 在哪里拿 | 自动更新 |
|---|---|---|---|---|
| **开发版** | 每次有改动合并进 `dev`（或 `main`），测试通过后 | `1.0.0-dev.57` | Actions → CI → 那次运行 → Artifacts（`Ludonimbus-dev.57-windows`、`-linux`），保留 30 天 | 不会推送给任何人 |
| **候选版** | 发版前手动运行 Release 工作流 | `1.0.0-rc.3` | Actions → Release → 那次运行 → Artifacts（`release-windows`、`release-linux`），保留 14 天 | 不会推送给任何人 |
| **正式版** | 在 `main` 上推送 `v1.0.0` 标签 | `1.0.0` | [Releases](https://github.com/Shupian0510/Ludonimbus/releases) | 推送给所有用户 |

- 开发版和候选版的版本号由 [`scripts/set-build-version.ps1`](../scripts/set-build-version.ps1) 在构建时加上后缀，不提交进仓库。按语义化版本，`1.0.0-dev.57` 和 `1.0.0-rc.3` 都比 `1.0.0` 旧：装了它们的人会在 1.0.0 发布后收到更新，也不会被「更新」回更旧的正式版。
- 候选版走的是和正式版完全相同的工作流（[`release.yml`](../.github/workflows/release.yml)），只是不请求 SignPath 签名、不创建 Release。发版前要测的就是它。
- 想给某个还没合并的分支打安装包：Actions → CI → Run workflow，选那个分支，勾上 build。

## 发一个版本

```powershell
# 1. dev 上要发的改动都已合并，CI 通过；把 CHANGELOG.md 当前版本一节整理成发布说明（中英文）。

# 2. 打候选版并测试（产物在这次运行的 Artifacts 里）
gh workflow run release.yml --ref dev

# 3. 发版 PR：dev → main。脚本会检查分支、版本号、更新日志和测试
./scripts/release.ps1
#    合并方式选 Create a merge commit（不要 squash，否则 dev 和 main 的历史会分叉）。

# 4. 在 main 上打标签并推送，GitHub Actions 接手编译、签名和发布
git switch main; git pull
./scripts/release.ps1 -Tag

# 5. 发布之后：把 main 合回 dev，并把 dev 的版本号改成下一个版本
git switch -c ci/next-version origin/dev
git merge origin/main
./scripts/bump-version.ps1 -Version 1.1.0     # 同时在 CHANGELOG.md 顶部加一个空的双语小节
#    提 PR 到 dev，合并方式选 Create a merge commit
```

配置了 SignPath 代码签名后，第 4 步的 Actions 中途会停下两次，等你在 SignPath 里批准签名请求（先是程序，再是安装包），见 [code-signing.md](code-signing.md#3-每次发版)。

发布后大约 15–20 分钟（加上等你批准签名的时间），[Releases](https://github.com/Shupian0510/Ludonimbus/releases) 页面会出现新版本，包含：

- `Ludonimbus_X.Y.Z_x64-setup.exe`：Windows 安装包（附带 Steam 插件 `millennium/ludonimbus.star`），配置了 SignPath 时带 Authenticode 签名
- `Ludonimbus_X.Y.Z_x64-setup.exe.sig`：更新签名
- `latest.json`：自动更新读取的版本信息，说明文字来自 CHANGELOG.md
- `Ludonimbus-linux-x86_64.tar.gz`：Linux / Steam Deck 下载包（AppImage、启动包装器、后台服务、安装脚本）
- `Ludonimbus-decky.zip`：Decky 插件

## 修已发布的版本

用户手里的版本有 bug，而 `dev` 上已经是下一个版本的半成品时：

1. 从 `main` 拉 `hotfix/…`，修好；用 `./scripts/bump-version.ps1` 升补丁号并写更新日志。
2. PR 到 `main`（Squash and merge），然后 `./scripts/release.ps1 -Tag`。
3. 把 `main` 合回 `dev`（PR，Create a merge commit），让修复也进下一个版本。合并时 `dev` 的版本号和它自己的更新日志小节保持不变。

## 文档站

`website/` 在推送到 `main` 时由 [`docs.yml`](../.github/workflows/docs.yml) 部署。

- 新功能的文档和功能放在同一个 PR 里进 `dev`，发版时随 `dev → main` 一起上线。
- 已发布版本的文档有错：从 `main` 拉 `docs/…`，PR 到 `main`，合并后几分钟上线；之后和 hotfix 一样合回 `dev`。

## winget

用户可以用 `winget install Shupian0510.Ludonimbus` 安装、`winget upgrade` 更新，清单在 [microsoft/winget-pkgs](https://github.com/microsoft/winget-pkgs/tree/master/manifests/s/Shupian0510/Ludonimbus)。

- Release 发布后，`release.yml` 的 `winget` 任务用 [Komac](https://github.com/russellbanks/Komac) 从 fork `Shupian0510/winget-pkgs` 给 winget-pkgs 提一个更新 PR。微软的自动检查和审核通过后才会合并，通常要一两天，之后 `winget upgrade` 才能看到新版本。
- 需要 `WINGET_TOKEN`（见下面的 Secrets）；没有配置时这个任务直接跳过。
- Komac 只能更新已经在 winget-pkgs 里的包。第一个版本 0.4.0 是手动提交的（[microsoft/winget-pkgs#487408](https://github.com/microsoft/winget-pkgs/pull/487408)），它合并之前发新版，`winget` 任务会失败；Release 本身不受影响，合并后在 Actions 里重新运行这个任务即可。
- 第一次向 winget-pkgs 提 PR 时，微软的 CLA 机器人会要求签贡献者协议，按它的提示在 PR 里回复一次，之后的 PR 不用再签。
- 微软的检查没通过时（比如安装包被误报），PR 会被打上标签并等待处理，需要去 PR 里看一下。
- Komac 的版本固定在 `release.yml` 的 `KOMAC_VERSION` / `KOMAC_SHA256`；升级时两个一起改，哈希取自该版本 Release 里的 `SHA256SUMS`。

## 版本号和更新日志

- 版本号遵循 [语义化版本](https://semver.org/lang/zh-CN/)：修 bug 升补丁号，加功能升次版本号。1.0 之后，不兼容的改动（包括云端文件布局）升主版本号。
- `CHANGELOG.md` 每个版本一节，中英文都要有，CI 每次都检查（`scripts/changelog-section.ps1`）。当前版本那一节在开发期间就存在：每个用户看得见的改动，在自己的 PR 里中英文各加一行。

```markdown
## 1.0.0

### 中文

- 新增：……
- 修复：……

### English

- New: ...
- Fixed: ...
```

发版时这一节会原样作为 GitHub Release 的说明，也会显示在应用内的更新提示里。

## 自动化检查

| 时机 | 检查内容 | 位置 |
|---|---|---|
| 每个 PR、每次推送到 `dev` / `main` | 各处版本号一致、当前版本有双语更新日志、进 `main` 的 PR 来自允许的分支（`meta`）；Windows 和 Linux 上的 Rust 测试、前端构建、桌面应用能编译（`test`、`linux`，只改文档时跳过）；汇总成 `ok` | [`ci.yml`](../.github/workflows/ci.yml) |
| 推送到 `dev` / `main`，`ok` 通过后 | 开发版：Windows 安装包、Linux 下载包、Decky 插件 | `ci.yml` 的 `windows-build`、`linux-build` |
| 手动运行 Release | 候选版：和正式版一样构建，不签名、不发布 | [`release.yml`](../.github/workflows/release.yml) |
| 推送 `v*` 标签 | 标签与版本号一致、更新日志双语齐全，然后编译、代码签名（配置了 SignPath 时）、生成更新签名、发布 Release 和 `latest.json` | `release.yml` |
| Release 发布后 | 给 winget-pkgs 提更新 PR（需要 `WINGET_TOKEN`） | `release.yml` 的 `winget` |
| 推送到 `main` 且改了 `website/` | 部署文档站 | [`docs.yml`](../.github/workflows/docs.yml) |

Linux 下载包只有一份构建定义（[`linux-package.yml`](../.github/workflows/linux-package.yml)），开发版和正式版都调用它。

## 仓库设置

**分支规则**（Settings → Rules → Rulesets，两条：`dev` 和 `main`）：

- 必须通过 PR 修改；必须通过状态检查 `ok`；禁止强制推送；禁止删除分支。
- 需要的审批数是 0。有第二个维护者后改成 1，并加上 `CODEOWNERS`。
- 没有绕过名单，仓库所有者也要走 PR。真有紧急情况，管理员可以在设置里临时停用规则，用完再打开。
- `main` 的规则在 1.0 合并进去之前不要求 `ok`：`main` 上现在还是 0.6 的工作流，没有这个检查。1.0 发布后给它加上。

**谁能做什么**：

| | 现在 | 有更多维护者之后 |
|---|---|---|
| 提 PR | 任何人（fork） | 一样 |
| 合并进 `dev` | 仓库所有者 | 有写权限的维护者，至少一人审过 |
| 发版（合并进 `main`、推标签、批准签名） | 仓库所有者 | 仓库所有者或指定的发布负责人 |

**Secrets**：

| 名称 | 用途 |
|---|---|
| `LUDONIMBUS_GOOGLE_CLIENT_ID` / `LUDONIMBUS_GOOGLE_CLIENT_SECRET` | 内置的 Google OAuth 客户端（见 [google-oauth-setup.md](google-oauth-setup.md)） |
| `LUDONIMBUS_BAIDU_APP_KEY` / `LUDONIMBUS_BAIDU_SECRET_KEY` / `LUDONIMBUS_BAIDU_APP_NAME` | 内置的百度网盘应用。没有设置：官方构建不内置（见 [baidu-netdisk-setup.md](baidu-netdisk-setup.md)） |
| `TAURI_SIGNING_PRIVATE_KEY` / `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` | 更新签名私钥。**私钥丢失后无法再给已安装的用户推送更新**，本地备份在 `%USERPROFILE%\.tauri\ludonimbus.key` |
| `WINGET_TOKEN` | 给 winget-pkgs 提 PR 用的 GitHub token：classic token，只勾 `public_repo`（Komac 不支持 fine-grained token）。它能写你所有的公开仓库，建议设过期时间，到期后换新的 |
| `SIGNPATH_API_TOKEN` | SignPath 代码签名的 API token，和 Variables 里的 `SIGNPATH_ORGANIZATION_ID`（以及可选的 `SIGNPATH_PROJECT_SLUG`、`SIGNPATH_SIGNING_POLICY_SLUG`）一起配置，见 [code-signing.md](code-signing.md#2-通过后配置)；没配置时安装包不签名 |

来自 fork 的 PR 读不到这些 Secrets。测试不需要它们；开发版和正式版只在本仓库的分支和标签上构建。

## 以后：公开的测试版通道

现在的开发版和候选版要登录 GitHub 才能从 Actions 下载，适合维护者和愿意帮忙测试的人。需要让普通用户也能选择接收测试版时再加：

- 用 `vX.Y.Z-beta.N` 标签发 GitHub 预发布版（预发布版不会被正式通道的 `releases/latest` 选中）。
- 应用设置里加「接收测试版」开关，打开后改为检查测试版的 `latest.json`。

---

## English summary

- **Branches.** `main` is the released version: the README, the documentation site and the downloads follow it. `dev` is the next version: all new work lands there by pull request, and its version number is the next release. Both branches require a pull request and the `ok` check; no direct or forced pushes.
- **Three kinds of builds.** A *development build* (`1.0.0-dev.N`) for every merge into `dev`, as a 30-day workflow artifact of the CI run. A *release candidate* (`1.0.0-rc.N`) when the Release workflow is run by hand: the same workflow as a release, without SignPath and without publishing, kept 14 days. A *release* (`1.0.0`) when a `v1.0.0` tag is pushed on `main`. Only releases are on the Releases page and offered by the auto-updater. `scripts/set-build-version.ps1` adds the suffix at build time; it is never committed.
- **Releasing.** Tidy the current section of `CHANGELOG.md` on `dev` → `gh workflow run release.yml --ref dev` and test the candidate → `./scripts/release.ps1` opens the pull request `dev → main` (merge with **Create a merge commit**) → on `main`, `./scripts/release.ps1 -Tag` → afterwards merge `main` back into `dev` and set the next version with `./scripts/bump-version.ps1`.
- **Hotfix.** Branch `hotfix/*` from `main`, bump the patch version, pull request into `main`, tag, then merge `main` back into `dev`.
- **Documentation site.** Deployed from `main`. Documentation of new features goes into `dev` with the feature and goes live with the release; corrections for the released version go through `docs/*` into `main`.
- **Release contents.** Windows installer, its updater signature and `latest.json`; `Ludonimbus-linux-x86_64.tar.gz`; `Ludonimbus-decky.zip`. Code signing through SignPath pauses the release twice for approval (see [code-signing.md](code-signing.md)).
- **winget.** The package is `Shupian0510.Ludonimbus`. After each release, the `winget` job runs Komac (pinned version and SHA-256) to open an update pull request on microsoft/winget-pkgs. It needs the `WINGET_TOKEN` secret and is skipped without it.
- **Repository settings.** A ruleset on each of `main` and `dev` (pull request, `ok` check, no force push, no deletion, no bypass list; 0 approvals until there is a second maintainer). Until 1.0 is merged, `main` still has the 0.6 workflow and its ruleset doesn't require `ok`. Secrets are not available to pull requests from forks; tests don't need them.
