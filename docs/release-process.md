# 发版流程 / Release process

## 分支与版本

- **只有一个主分支 `main`**：日常开发直接提交，或者从功能分支发 PR 合并进来。
- **正式版靠标签发布**：推送 `vX.Y.Z` 标签后，GitHub Actions 编译、签名并创建 Release，已安装的 Ludonimbus 会提示更新。
- **开发版**：每次推送到 `main`，CI 都会打一个安装包，作为构建产物保留 14 天（Actions 页 → CI → 对应的运行 → Artifacts → `Ludonimbus-dev-<提交号>`）。想在发版前先试，可以下载安装。它不会出现在 Release 里，也不会被自动更新推送。
- 版本号遵循 [语义化版本](https://semver.org/lang/zh-CN/)：修 bug 升补丁号（0.1.2 → 0.1.3），加功能升次版本号（0.1.x → 0.2.0）。1.0 之前允许不兼容的改动，但要在更新日志里写清楚。

## 发一个新版本

```powershell
# 1. 改版本号：同时更新 Cargo.toml、app/package.json、app/src-tauri/tauri.conf.json、Cargo.lock，
#    并在 CHANGELOG.md 顶部加一个空的双语小节
./scripts/bump-version.ps1 -Version 0.1.3

# 2. 在 CHANGELOG.md 里把「### 中文」和「### English」都写好，然后提交
git commit -am "Release 0.1.3"

# 3. 检查并发布：在 main 上、工作区干净、版本号一致、更新日志双语齐全、测试通过，
#    然后打标签并推送，GitHub Actions 接手编译和发布
./scripts/release.ps1
```

发布后大约 10–15 分钟，[Releases](https://github.com/Shupian0510/Ludonimbus/releases) 页面会出现新版本，包含：
- `Ludonimbus_X.Y.Z_x64-setup.exe`：安装包
- `Ludonimbus_X.Y.Z_x64-setup.exe.sig`：更新签名
- `latest.json`：自动更新读取的版本信息，说明文字来自 CHANGELOG.md

## 更新日志格式

每个版本一节，中英文都要有，发版时会检查（`scripts/changelog-section.ps1`）：

```markdown
## 0.1.3

### 中文

- 修复：……
- 新增：……

### English

- Fixed: ...
- New: ...
```

这一节会原样作为 GitHub Release 的说明，也会显示在应用内的更新提示里。

## 自动化检查

| 时机 | 检查内容 | 位置 |
|---|---|---|
| 每次推送 / PR | 三处版本号一致、当前版本有双语更新日志、Rust 测试、前端构建、桌面应用能编译 | `.github/workflows/ci.yml` |
| 推送到 main | 额外打一个开发版安装包（构建产物，保留 14 天） | `ci.yml` 的 `dev-build` |
| 推送 `v*` 标签 | 标签与版本号一致、更新日志双语齐全，然后编译、签名、发布 Release 和 `latest.json` | `.github/workflows/release.yml` |

## 需要的仓库 Secrets

| 名称 | 用途 |
|---|---|
| `LUDONIMBUS_GOOGLE_CLIENT_ID` / `LUDONIMBUS_GOOGLE_CLIENT_SECRET` | 内置的 Google OAuth 客户端（见 [google-oauth-setup.md](google-oauth-setup.md)） |
| `TAURI_SIGNING_PRIVATE_KEY` / `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` | 更新签名私钥。**私钥丢失后无法再给已安装的用户推送更新**，本地备份在 `%USERPROFILE%\.tauri\ludonimbus.key` |

## 以后：测试版通道

等有了其他用户、需要先给一部分人试用时再加：
- 用 `vX.Y.Z-beta.N` 标签发 GitHub 预发布版（预发布版不会被正式通道的 `releases/latest` 选中）。
- 应用设置里加「接收测试版」开关，打开后改为检查测试版的 `latest.json`。
- 同时给 `main` 开启分支保护，改动都走 PR。

---

## English summary

- Single branch `main`. Stable releases are published by pushing a `vX.Y.Z` tag; every push to `main` also produces a development installer as a 14-day workflow artifact (never offered by the auto-updater).
- Release: `./scripts/bump-version.ps1 -Version X.Y.Z` → write both `### 中文` and `### English` in `CHANGELOG.md` and commit → `./scripts/release.ps1` (checks branch, clean tree, versions, bilingual notes and tests, then tags and pushes). `release.yml` builds, signs and publishes the installer, its signature and `latest.json`.
- CI checks on every push: matching versions, a bilingual changelog entry for the current version, Rust tests, frontend build, desktop app build.
- A beta channel (`-beta.N` prerelease tags plus an opt-in setting) will be added once there are more users.
