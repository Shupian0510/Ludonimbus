# 参与开发 / Contributing

[English](#english)

怎么搭环境、跑测试见 [docs/development.md](docs/development.md)，怎么发版见 [docs/release-process.md](docs/release-process.md)。这一页讲一个改动怎么进到项目里。不管是人还是 AI agent，规则相同。

## 分支

| 分支 | 是什么 | 怎么改 |
|---|---|---|
| `main` | 已经发布的版本。项目首页、文档站、下载页都以它为准 | 只接受三种 PR：发版（来自 `dev`）、修已发布版本的 bug（`hotfix/*`）、改已发布版本的文档（`docs/*`） |
| `dev` | 下一个版本。所有新改动都先进这里 | PR，检查通过后合并 |
| `feat/*` `fix/*` `ci/*` `docs/*` `hotfix/*` | 一件事一个分支 | 从 `dev` 拉；`hotfix/*` 和改已发布文档的 `docs/*` 从 `main` 拉 |

两个长期分支都不能直接推送，也不能强制推送。

## 一个改动怎么进来

1. 从 `dev` 拉分支：`git switch -c feat/save-shelf origin/dev`。
2. 改代码。提交前在本地跑一遍：
   ```powershell
   cargo fmt --all
   cargo test --workspace --exclude ludonimbus-app
   npm --prefix app run build
   cargo clippy --workspace --all-targets -- -D warnings
   ```
   格式按根目录的 `rustfmt.toml`（一行最多 160 个字符），CI 会检查；clippy 的警告在 CI 里算错误。clippy 要先有桌面应用的 sidecar（`./scripts/prepare-sidecars.ps1`）。

   改了界面和 Rust 之间传递的类型（命令的参数和返回值、事件的内容）时，再运行 `cargo test -p ludonimbus-app export_bindings`，把 `app/src/bindings/` 的变化一起提交。这些 TypeScript 类型是从 Rust 生成的，CI 会重新生成，和提交的不一样就失败（见 [docs/development.md](docs/development.md)）。
3. 向 `dev` 提 PR。标题用一句英文说清做了什么（合并后它就是 `dev` 上的提交信息）。
4. CI 的 `ok` 通过后，用 **Squash and merge** 合并，删掉分支。
5. 合并后 CI 会用 `dev` 打一个开发版安装包（Actions 的构建产物，不是正式发布），要试的人从那里下载。

## 对 PR 的要求

- **一个 PR 只做一件事。** 大的改动拆成几个能单独合并的 PR，有先后的在描述里写「依赖 #N」。
- **带测试。** 改了同步、存储、路径这类核心逻辑，要有覆盖它的测试。测试只用临时目录，不碰真实的存档文件夹、真实的 Steam 配置、真实的云盘。
- **用户看得见的改动**（界面文字、新功能、行为变化）在同一个 PR 里：
  - `CHANGELOG.md` 当前版本一节，中文和英文各加一行；
  - 界面文字两种语言都写（`app/src/i18n/`，规则见 [docs/i18n.md](docs/i18n.md)）；
  - 文档站 `website/` 的中英文页面。
- **界面**按 [docs/plan-v2.md](docs/plan-v2.md) 10.1 的规则：文字少，说明放进「?」，只有覆盖和删除弹确认框，不加动画。
- **云端文件的格式**是所有功能的地基，要改先看 [docs/plan-v2.md](docs/plan-v2.md) 3.9 的扩展规则。
- **不要改版本号。** `dev` 上的版本号是下一个要发布的版本，只在发版后改一次。
- 不要提交密钥、令牌、个人路径和账号名。

## 很多人同时做的时候

- 开始前先开一个 issue 或 draft PR 说在做什么，避免两个人改同一块。
- 分支尽量短，几天内合并。时间长的分支经常 `git merge origin/dev`。
- 最容易冲突的是往同一个列表里加行的文件：`CHANGELOG.md`、`app/src/i18n/*.ts`、`crates/core/src/lib.rs` 的模块列表。冲突时两边的行都保留。
- 有依赖关系的改动，先合下层（数据结构、存储），再合上层（界面）。

## 提交信息

英文，第一行一句话说明做了什么（`Add …`、`Fix …`），不超过 72 个字符。需要解释原因时空一行再写。

## 安全问题

不要开公开的 issue，用 GitHub 的[私密漏洞报告](https://github.com/Shupian0510/Ludonimbus/security/advisories/new)。

---

## English

How to build and test: [docs/development.md](docs/development.md). How releases are made: [docs/release-process.md](docs/release-process.md). This page is about how a change gets in. The rules are the same for people and for AI agents.

**Branches**

- `main` is the released version. The project page, the documentation site and the downloads follow it. It only takes three kinds of pull requests: a release (from `dev`), a fix of the released version (`hotfix/*`), a correction of its documentation (`docs/*`).
- `dev` is the next version. All new work goes here first.
- One branch per piece of work: `feat/*`, `fix/*`, `ci/*`, `docs/*` from `dev`; `hotfix/*` and documentation corrections from `main`.

Neither long-lived branch takes direct or forced pushes.

**A change, step by step**

1. Branch from `dev`.
2. Before you push, run `cargo fmt --all`, `cargo test --workspace --exclude ludonimbus-app`, `npm --prefix app run build` and `cargo clippy --workspace --all-targets -- -D warnings`. CI checks the formatting (`rustfmt.toml`, lines up to 160 characters) and treats clippy warnings as errors; clippy needs the desktop app's sidecars (`./scripts/prepare-sidecars.ps1`). When you change a type the web UI and Rust send each other (a command's arguments or result, an event's payload), also run `cargo test -p ludonimbus-app export_bindings` and commit what it changes in `app/src/bindings/`: those TypeScript types are generated from Rust, and CI fails when generating them again gives something else (see [docs/development.md](docs/development.md)).
3. Open a pull request into `dev`. The title says in one sentence what it does; it becomes the commit message on `dev`.
4. When the `ok` check passes, merge with **Squash and merge** and delete the branch.
5. CI then builds a development installer from `dev` (a workflow artifact, not a release).

**What a pull request needs**

- One thing per pull request. Split large work into parts that can be merged on their own, and write "depends on #N" where the order matters.
- Tests for changes to core logic (sync, storage, paths). Tests use temporary folders only: never real save folders, the real Steam configuration or a real cloud drive.
- For anything users can see, in the same pull request: a line in both languages under the current version in `CHANGELOG.md`, interface texts in both languages (`app/src/i18n/`, see [docs/i18n.md](docs/i18n.md)), and the pages of the documentation site in `website/`.
- The interface follows the rules in [docs/plan-v2.md](docs/plan-v2.md), section 10.1: few words, explanations behind a "?", confirmation dialogs only for overwriting and deleting, no added animation.
- The format of the files in the cloud is what everything else stands on. Read the rules in [docs/plan-v2.md](docs/plan-v2.md), section 3.9, before changing it.
- Don't change the version number. The version on `dev` is the next release and changes once, right after a release.
- No keys, tokens, personal paths or account names.

**When many people work at once**

- Open an issue or a draft pull request first to say what you are doing.
- Keep branches short-lived; merge `origin/dev` into long ones often.
- The files that conflict most are lists everyone adds to: `CHANGELOG.md`, `app/src/i18n/*.ts`, the module list in `crates/core/src/lib.rs`. Keep both sides' lines.
- Land the lower layer (data structures, storage) before what is built on it (interface).

**Commit messages**: English, one sentence in the first line saying what was done (`Add …`, `Fix …`), at most 72 characters; the reason after an empty line when it needs one.

**Security problems**: not in a public issue. Use GitHub's [private vulnerability reporting](https://github.com/Shupian0510/Ludonimbus/security/advisories/new).
