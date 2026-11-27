# 开发说明 / Development

## 从源码构建

需要：Rust（stable，MSVC）、Node.js 20+、Visual Studio C++ 生成工具、WebView2（Windows 10/11 自带）。

```powershell
# 1. 准备随应用分发的文件：启动包装器、Ludusavi 官方发布版、Steam 插件
./scripts/prepare-sidecars.ps1

# 2. 开发模式运行
cd app
npm install
npm run tauri dev

# 3. 打包安装程序（NSIS）。Google OAuth 客户端从 secrets.local.env 或环境变量读取
cd ..
./scripts/build-release.ps1
```

只调界面时可以在 `app/` 里 `npm run dev`，直接在浏览器里打开，会使用模拟数据（`app/src/mock.ts`）。

只改 Steam 插件时运行 `./scripts/build-plugin.ps1`。插件源码在 `plugin-millennium/`，用 Millennium 官方的 starlight 工具打包成 `.star`，说明见 [steam-plugin.md](steam-plugin.md)。

### Linux（WSL2）

Steam Deck 版的基础（[plan-phase5.md](plan-phase5.md)）。在 WSL2 的 Ubuntu 24.04 里直接用 Windows 这边的源码文件夹，编译输出放在 Linux 自己的磁盘上（快得多，也不和 Windows 的 `target/` 混在一起）：

```bash
# 一次性：编译工具和桌面应用要的系统库（sudo 要密码时，可以在 Windows 里用 wsl -u root 运行这一行）
sudo apt-get install -y build-essential pkg-config file rustup libssl-dev libdbus-1-dev \
  libwebkit2gtk-4.1-dev libgtk-3-dev libayatana-appindicator3-dev librsvg2-dev libxdo-dev libsoup-3.0-dev
rustup default stable

cd /mnt/c/Users/<你>/…/Ludonimbus
export CARGO_TARGET_DIR=$HOME/target/ludonimbus
cargo test --workspace --exclude ludonimbus-app
scripts/prepare-sidecars.sh            # 启动包装器 + Ludusavi 官方 Linux 版（校验 SHA-256）；只想编译时加 --stand-ins
cargo build -p ludonimbus-app          # 前端用 Windows 这边 npm run build 生成的 app/dist
```

CI 的 `linux` 任务（ubuntu-24.04）跑同样的测试，并检查桌面应用能编译。`linux-build`（每次合并进 dev）用 [`linux-package.yml`](../.github/workflows/linux-package.yml) 打出 Linux 下载包和 Decky 插件，做法和下面相同：

```bash
scripts/prepare-sidecars.sh --release
cargo build --release -p ludonimbus-agent          # 游戏模式的后台服务
(cd app && npx tauri build --bundles appimage)      # 要 libfuse2；容器里设 APPIMAGE_EXTRACT_AND_RUN=1
# 下载包 = AppImage + target/release/ludonimbus-run + ludonimbus-agent + scripts/install-linux.sh
```

**Decky 插件**（`plugin-decky/`，游戏模式的快捷菜单）：`npm ci && npm run build` 生成 `dist/index.js`；装到 Deck 时把 `dist/`、`main.py`、`plugin.json`、`package.json` 放进 `~/homebrew/plugins/Ludonimbus/`。后端只读 `~/.config/Ludonimbus/agent.json` 转发请求，本地接口的说明在 [agent/src/main.rs](../crates/agent/src/main.rs)。

**后台服务**：`ludonimbus-agent --once` 只补做一次「被强制结束的包装器」然后退出，排查时有用；正常由 `systemd --user` 一直运行，日志和应用写在同一个文件夹。

**Switch 客户端**（单独的仓库 [Ludonimbus-NX](https://github.com/Shupian0510/Ludonimbus-NX)，本地放在本仓库旁边）：用 devkitPro 编译。devkitpro.org 拒绝这台电脑的下载，WSL2 里的 `/opt/devkitpro` 是从 Docker Hub 的官方镜像 `devkitpro/devkita64` 解出来的；编译：

```bash
export DEVKITPRO=/opt/devkitpro PATH=/opt/devkitpro/tools/bin:/opt/devkitpro/devkitA64/bin:$PATH
cd /mnt/c/…/Ludonimbus-NX && make            # ludonimbus.nro
cd tests/host && make                         # 共用部分的 Linux 版（要 libmbedtls-dev libjansson-dev）
```

**Switch 局域网接口**：协议见 [nx-protocol.md](nx-protocol.md)；`crates/core/tests/nx_lan.rs` 按 Switch 的角色走一遍，不用 TLS。要试真正的 HTTPS，在设置里打开「允许 Switch 连接」，然后 `curl -k https://127.0.0.1:47716/nx/v1/hello`。

写测试时注意：Linux 上 `\` 不是路径分隔符，`tmp.join(r"a\b")` 会建出一个名字里带反斜杠的文件。测试里的路径用 `/`（Windows 也认）；需要绝对路径的 Windows 例子用 `util::test_path(r"D:\Emu")`，在 Linux 上会变成 `/d/Emu`。代码里比较、拼接路径用 `util::same_path` / `util::native_sep` / `std::path::MAIN_SEPARATOR`，不要写死 `\`。

## 测试与排查

```powershell
cargo test --workspace --exclude ludonimbus-app
```

命令行工具能完成大部分操作，方便排查问题：

```powershell
cargo run -p ludonimbus-cli -- status
cargo run -p ludonimbus-cli -- steam shortcuts
cargo run -p ludonimbus-cli -- snapshots fe-fortunes-weave
cargo run -p ludonimbus-cli -- discover --ludusavi --folder D:\Games   # 扫描本机能添加的游戏
cargo run -p ludonimbus-cli -- speedtest --mb 200 --threads 4           # 当前云盘的上传 / 下载速度（测完删除）
cargo run -p ludonimbus-cli -- game-backup list zelda64recomp           # 游戏本体备份的版本
cargo run -p ludonimbus-cli -- game-backup backup botw --dry-run        # 会备份哪些部分、多大
```

`./scripts/cloud-speedtest.ps1 -Threads 2,4,8` 用你的真实设置测当前云盘，报告写到 `%LOCALAPPDATA%\Ludonimbus-speedtest\`。`./scripts/cloud-sources-e1.ps1 -Quark|-Baidu …` 检查夸克 / 百度能否使用网盘里已有的游戏文件（plan-game-backup.md 10.9）。

日志在 `%APPDATA%\Ludonimbus\logs\`：`runner.log`（从 Steam 启动时）、`sync.log`、`watcher.log`（托盘后台监视）、`agent.log`（本机接口）、`steam-plugin.log`（安装 / 更新插件）、`gamebackup.log`（游戏本体的备份、恢复、删除）。

手动测试清单：[testing-v0.1.md](testing-v0.1.md)、[testing-v0.2.md](testing-v0.2.md)、[testing-v0.3.md](testing-v0.3.md)、[testing-v0.4.md](testing-v0.4.md)。

## 文档站

用户文档在 `website/`（VitePress，中文在根目录，英文在 `en/`，文件名一一对应），推送到 main 后由 `.github/workflows/docs.yml` 构建并发布到 <https://shupian0510.github.io/Ludonimbus/>。本地预览：

```powershell
cd website
npm ci
npm run dev       # http://localhost:5173/Ludonimbus/，改了页面自动刷新
npm run build     # 和 CI 一样构建，死链会报错
npm run preview   # 预览构建结果
```

- 更新日志页由 `scripts/gen-changelog.mjs` 在构建时从根目录的 `CHANGELOG.md` 生成（`### 中文` / `### English` 两段分到两种语言），不要手改 `website/reference/changelog.md`。
- `privacy.html` 必须保持原地址（Google OAuth 同意屏幕填了这个链接），内容在 `website/parts/`。
- 计划要放截图的位置用 `<!-- screenshot: 场景名 -->` 标出，清单在 `website/SCREENSHOTS.md`。
- 「使用场景」页（`non-steam-cloud-saves/`、`emulator-cloud-saves/`、`recompiled-game-saves/`、`steam-integration/`、`google-drive-save-sync/`、`supported-games/`、`comparison/`，中英各一份）是面向搜索的产品页，每页都写了 `title` 和 `description` frontmatter。改功能时记得一起更新，`comparison/` 里其他工具的信息要注明核对时间。

### 搜索引擎

构建时自动生成，不用手动维护：

- `sitemap.xml`（带 `<lastmod>`，取自每页最后一次提交的时间，所以 `docs.yml` 用完整历史检出）。
- 每页的 canonical、中英文 hreflang（`x-default` 指向英文）、Open Graph；两个首页带 `SoftwareApplication` 结构化数据，版本号读自 `app/src-tauri/tauri.conf.json`。逻辑在 `website/.vitepress/config.mts` 的 `transformPageData`。
- IndexNow：部署后 `docs.yml` 的 `indexnow` 任务运行 `website/scripts/indexnow.mjs`，把这次推送改动的页面提交给 Bing 等搜索引擎（Google 不支持 IndexNow）。密钥文件是 `website/public/682015ff8ef9063735475f3d73379694.txt`，换密钥时脚本里的 `key` 一起改。

需要手动做一次：

1. [Google Search Console](https://search.google.com/search-console)：添加「网址前缀」资源 `https://shupian0510.github.io/Ludonimbus/`，验证方式选「HTML 标记」，把 `content="…"` 里的值填进 `config.mts` 的 `googleSiteVerification`，推送部署后点验证；然后在「站点地图」里提交 `sitemap.xml`。
2. [Bing Webmaster Tools](https://www.bing.com/webmasters)：可以直接从 Google Search Console 导入；单独验证的话，把值填进 `bingSiteVerification`。

`website/public/robots.txt` 在 `github.io/Ludonimbus/` 下不起作用：搜索引擎只读域名根目录的 robots.txt，而 `shupian0510.github.io` 根目录没有，等于全部允许抓取。站点换到独立域名后它才生效。

用户能看到的改动（界面文字、新功能、行为变化），在同一个 PR 里更新文档站的中英文页面。文档站从 `main` 部署，所以这些页面会在发版时和功能一起上线。

## 协作和发布

- 改动怎么进来（分支、PR、检查）：[CONTRIBUTING.md](../CONTRIBUTING.md)。
- 从 PR 到用户手里的整条管线（开发版、候选版、正式版，发版步骤）：[release-process.md](release-process.md)。

简单说：从 `dev` 拉分支，向 `dev` 提 PR，CI 的 `ok` 通过后合并；每次合并 CI 都会打一个开发版安装包（Actions 构建产物，保留 30 天）。`main` 只在发版时更新。

## 内置存档规则

`adapters/` 里的三个文件编译进程序，按顺序读，同一个程序匹配到多条规则时选最具体的（路径层数多、带 `sibling` / `args` 条件的优先），一样具体时先读到的优先：

| 文件 | 内容 | 维护方式 |
|---|---|---|
| `builtin.yaml` | 重编译 / 移植游戏 | 手写，存档位置要按源码或 PCGamingWiki 核对 |
| `emulators.yaml` | 模拟器（`kind: emulator`） | 手写；按游戏同步的写 `gameId` + `gameSaves`，识别游戏 ID 的代码在 [`emulators.rs`](../crates/core/src/emulators.rs) |
| `recomplauncher.yaml` | RecompLauncher 目录里的重编译游戏 | 生成，不要手改；要修正某个游戏就在 `builtin.yaml` 写同 key 的规则 |

更新 `recomplauncher.yaml`（需要 Python 3.9+，只用标准库）：

```powershell
python scripts/import-recomplauncher.py                    # 下载 https://recomplauncher.com/pc-catalog.json
python scripts/import-recomplauncher.py pc-catalog.json    # 或用已下载的文件
cargo test -p ludonimbus-core adapters                     # 检查规则能解析、根目录都认识
```

脚本只取目录里开发者在 recomplauncher.json 声明的存档位置，以及 N64Recomp 游戏（`<应用文件夹>/saves`），丢掉 macOS / Linux 路径、整个游戏目录和带 `..` 的路径；生成的规则带 `skipJunk`，扫描时会排除程序、光盘镜像、日志和缓存。RecompLauncher 的 `{install}` 是发布包根目录，exe 在子文件夹里时 `match` 写相对路径，Ludonimbus 据此把 `{install}` 设成发布包根目录。

`roots` 可以用 `|` 列几个候选位置（第一个存在的；都不存在时用最后一个），用来同时支持便携版和安装版。模拟器自己的设置和数据决定的位置在 `emulators::config_roots` 里每台电脑单独读取：Cemu 的 `<mlc_path>`，以及 Ryujinx 每个游戏的存档。

**跨平台的游戏规则**（`rules/games/<id>.yaml`，一个游戏一个文件，[`crosssaves.rs`](../crates/core/src/crosssaves.rs)）：游戏在几个平台上存的是同样的文件时写在这里，`crates/core/build.rs` 在构建时把文件夹里的 `*.yaml` 编译进去。每个文件旁边放各平台的样例存档（`rules/games/<id>/<平台>/`），`ludonimbus rules check <文件>` 检查字段和样例，CI 里由 `crates/core/tests/game_rules.rs` 对每个文件跑同样的检查。写法见 [rules/README.md](../rules/README.md)。

**Ryujinx 按游戏**（[`switch.rs`](../crates/core/src/switch.rs)）：存档在 `bis/user/save/<编号>`，编号按创建顺序在每台电脑上分配，记在存档索引 `bis/system/save/8000000000000000/0/imkvdb.arc`（`IMKV` 头 + `IMEN` 条目，键是 0x40 字节的存档属性：程序 ID、用户 ID 高位 / 低位、类型，值的前 8 字节是编号）。规则里用两个动态根目录：`{nxsave-<标题 ID>}` 是当前用户档案（`system/Profiles.json` 的 `last_opened`）的这个游戏的存档，`{nxdevice-<标题 ID>}` 是设备存档，都指向编号文件夹里的 `0`（已提交的那份）。这台电脑没有时先指向一个不存在的 `ludonimbus-new-…` 文件夹（扫描什么也找不到）；恢复前 `emulators::prepare_restore` 按 Ryujinx 的方式新建：索引加一条、`lastPublishedId` 加一、写 `ExtraData0/1`（属性、所有者、大小照这台电脑上同一游戏的其他存档，没有就 1 GB），索引的 `0` 和 `1` 两份都改，旧索引先复制到 `local-backups/ryujinx-save-index`。Ryujinx 运行时会把索引留在内存里，所以要新建存档时如果它开着就报错。恢复后 `finish_restore` 把 `0` 复制到 `1`（工作副本），不管 Ryujinx 打开存档时从哪份同步都能看到恢复的文件。

标题 ID：文件名里的 16 位十六进制；NSP 里的 `.cnmt.xml` 或票据（`.tik`）文件名；最后用 `prod.keys` 里的 `header_key` 解密 NSP / XCI 里 NCA 头（AES-128-XTS，扇区号大端作 tweak）读程序 ID。更新和 DLC 的 ID 归到本体（低 13 位清零）。

**模拟器数据目录扫描**（[`emugames.rs`](../crates/core/src/emugames.rs)，`discover::emulator_games`）：在 Steam 快捷方式和库里的模拟器、扫描到的文件夹里的模拟器，以及每个模拟器的默认数据目录里，逐个列出有存档的游戏，`saves` 用规则的 `gameSaves` 填好，和带游戏文件的快捷方式得到的完全一样（所以能按规则 + 存档模板判断是否已添加）。名称来自存档或模拟器（GCI 注释、Wii `banner.bin`、Cemu 游戏列表 / `meta.xml`、PARAM.SFO、Ryujinx `games/<ID>/gui/metadata.json`）；Ryujinx `Config.json` 的 `game_dirs`、Dolphin `Dolphin.ini` 的 `ISOPath*`、Cemu 游戏列表能找到游戏文件时填上启动参数。同一个模拟器的库条目不能重复同步同一批文件：整体同步（`saves` 为空）和逐个游戏二选一，同一个游戏只能一个条目（`games::check_emulator_overlap`）。

## Google Drive / 百度网盘

发布者需要在 Google Cloud 创建一次「桌面应用」类型的 OAuth 客户端，并在构建时注入 Client ID，见 [google-oauth-setup.md](google-oauth-setup.md)。普通用户只需要登录。

百度网盘没有内置的应用（没有做百度开放平台的上线审核），发布版的设置页不显示百度网盘这一项；代码还在，开发版里可以填自己在百度网盘开放平台创建的应用来连接，见 [baidu-netdisk-setup.md](baidu-netdisk-setup.md)。

## 项目结构

```text
crates/core          同步引擎：存档扫描、快照、云盘、Steam 快捷方式、同步判定、Millennium 安装
crates/cli           ludonimbus 命令行
crates/runner        ludonimbus-run，Steam 快捷方式实际启动的程序
crates/agent         ludonimbus-agent，不带界面的后台服务（Steam Deck 游戏模式）
app/                 桌面应用（Tauri 2 + React + TypeScript），含给 Steam 插件用的本机接口和手柄界面（app/src/handheld）
plugin-millennium/   Steam 插件（Millennium：Lua 后端 + TypeScript 前端）
plugin-decky/        游戏模式插件（Decky Loader：Python 后端 + TypeScript 前端）
adapters/            内置存档规则（游戏、模拟器、从 RecompLauncher 目录生成的规则）
rules/               跨平台的游戏规则，一个游戏一个文件，带样例存档
website/             文档站（VitePress），发布到 GitHub Pages
docs/                设计文档、测试清单（不发布到文档站）
scripts/             构建与发布脚本
```

## 设计文档

- [plan-phase1.md](plan-phase1.md)：整体设计、阶段划分、路线图
- [plan-phase2.md](plan-phase2.md)：下一阶段（更多云盘、添加游戏、截图与游戏时间、一键加入 Steam）
- [steam-plugin.md](steam-plugin.md)：Steam 插件、本机接口和一键安装
- [plan-game-backup.md](plan-game-backup.md)：游戏本体备份（切块、去重、云端布局、恢复）和云盘测速
- [plan-v2.md](plan-v2.md)：存储 v2（游戏、存档线、快照；绑定和编解码；版本向量；接管 Steam 云存档）和到 1.0 的路线图
- 同步判定规则：[`crates/core/src/sync.rs`](../crates/core/src/sync.rs) 顶部的表格

云端目录结构：

```text
Ludonimbus/devices/<设备>.json
Ludonimbus/games/<游戏>/game.json
Ludonimbus/games/<游戏>/snapshots/<版本>.zip
Ludonimbus/gamebackup/chunks/<blake3>                  游戏本体备份的数据块，所有游戏共用（见 plan-game-backup.md）
Ludonimbus/gamebackup/games/<游戏>/versions/<版本>.manifest  + <版本>.json
```

每个版本是一个只追加、不可修改的 zip 快照，带父版本指针和设备信息；最新的主线版本就是「云端当前版本」。
