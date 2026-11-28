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

### 界面和 Rust 之间的类型

命令的参数和返回值、事件的内容，在 TypeScript 里的类型是用 [ts-rs](https://github.com/Aleph-Alpha/ts-rs) 从 Rust 的类型生成的：`app/src/bindings/`，一个类型一个文件，加一个汇总的 `index.ts`，都提交进仓库。`app/src/api.ts`、`achievementsApi.ts`、`gameBackupApi.ts` 从这里导入并按界面用惯的名字导出，模拟数据（`mock.ts` 等）也按这些类型检查；不要再手写一份。

```powershell
cargo test -p ludonimbus-app export_bindings   # 重新生成；和其他桌面应用测试一样，要先有 sidecar 和 app/dist
```

- 改了这些 Rust 类型（加字段、改名、改 serde 属性）之后运行一次，把 `app/src/bindings/` 的变化一起提交。CI（Windows 的 `test`）会重新生成，有差别或多出文件就失败。
- 新命令用到新的类型时：在类型上加 `#[derive(TS)]`（和 `Serialize` 并列；它里面用到的类型也要加），再在 [`app/src-tauri/src/bindings.rs`](../app/src-tauri/src/bindings.rs) 的 `roots` 里加一行。里面的类型会自动带上。
- 名字：默认和 Rust 一样。重名（两个 `Candidate`）或界面一直用另一个名字时，用 `#[ts(rename = "…")]`；两个类型生成到同一个文件时，这个测试会失败。
- 对应规则跟着 serde：`rename_all`、`tag`、`flatten`、`skip` 照搬；`u64` / `i64` 是 `number`（JSON 里本来就是 JavaScript 的数），`DateTime` 和 `PathBuf` 是 `string`。
- `skip_serializing_if` 的字段只有同时有 `#[serde(default)]` 时才自动成为可选（`name?:`）；没有的话，`Option` 加 `#[ts(optional)]`，`Vec` / `String` 等加 `#[ts(as = "Option<_>", optional)]`。只当参数用、缺了就不改的结构（`GamePatch` 等）在结构上加 `#[ts(optional_fields)]`。
- Rust 里不精确的地方写明：`serde_json::Value` 用 `#[ts(type = "unknown")]` 或 `#[ts(as = "…")]`（存成 JSON 的 `StatusReport`），只有几个固定值的 `&'static str` / `String` 用 `#[ts(type = r#""a" | "b""#)]`。
- 用 `json!` 拼的事件内容（`openlist-progress`、`game-backup-progress`）没有 Rust 类型，类型写在监听它的组件里。
- 构建 core 时 ts-rs 会提示一次 `failed to parse serde attribute … other`：`Trigger` 的 `#[serde(other)]` 只管读取，可以不管。

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

**Decky 插件**（`plugin-decky/`，游戏模式的快捷菜单）：`npm ci && npm run build` 生成 `dist/index.js`；装到 Deck 时把 `dist/`、`main.py`、`plugin.json`、`package.json` 放进 `~/homebrew/plugins/Ludonimbus/`。后端只读 `~/.config/Ludonimbus/agent.json` 转发请求。本机接口只有一份实现，在 core 的 [`localapi`](../crates/core/src/localapi/mod.rs)，桌面应用和后台服务共用，说明见 [steam-plugin.md](steam-plugin.md)；测试是 `crates/core/tests/local_api.rs`（临时端口、假的宿主和临时数据文件夹）。

**后台服务**：`ludonimbus-agent --once` 只补做一次「被强制结束的包装器」然后退出，排查时有用；正常由 `systemd --user` 一直运行，日志和应用写在同一个文件夹。`install-linux.sh` 写的用户服务以较低的 CPU 和磁盘优先级运行（`Nice=10`、`IOSchedulingClass=best-effort`、`IOSchedulingPriority=7`），并尝试为当前用户打开 lingering（`loginctl enable-linger`），让它在切换游戏模式 / 桌面模式（会注销）后继续运行；要密码时脚本会打印那一条 `sudo loginctl enable-linger <用户>`。Decky 面板连不上 Ludonimbus 时，「启动 Ludonimbus」经插件的 Python 后端运行 `systemctl --user start ludonimbus-agent.service`。

**Linux 的其他硬盘**：扫描（游戏文件夹、模拟器程序）和存档查找器（Heroic / Lutris 的前缀）也看 `/run/media/<用户>/*`、`/run/media/mmcblk0p1` 这类直接挂在 `/run/media` 下的盘、`/var/mnt/*`、`/mnt/*`（`core::paths::extra_drives`，最多 32 个）：Steam 库（SteamOS 把 SD 卡的库放在卡的根目录）、GOG / Heroic、`Games` 文件夹，`Applications`、`Emulators`、EmuDeck 的 `Emulation/tools` 里的模拟器。

### Flatpak

桌面应用另有一个 Flatpak 包（[`packaging/flatpak/`](../packaging/flatpak/io.github.shupian0510.ludonimbus.yml)，应用 id `io.github.shupian0510.ludonimbus`，不上 Flathub）。CI 的 `linux-flatpak`（合并进 dev 后跟在 `linux-build` 后面，或者手动运行 CI 时勾上 build）用 Linux 下载包里的 `.deb` 和后台服务打出 `Ludonimbus.flatpak`，放进 workflow artifact，并在沙箱里用 `ldd` 检查每个库都找得到。手动打包时照 [`ci.yml`](../.github/workflows/ci.yml) 里这个任务的步骤：装 `flatpak-builder` 和 `org.gnome.Platform//49` + `org.gnome.Sdk//49`，把 Flathub 的 shared-modules 克隆到 `packaging/flatpak/shared-modules`，把 `.deb` 里的 `usr/bin/{ludonimbus-app,ludonimbus-run,ludusavi}`、`usr/lib/Ludonimbus/`、后台服务和图标放进 `packaging/flatpak/stage/`，再 `flatpak-builder --user --repo=repo build io.github.shupian0510.ludonimbus.yml` 和 `flatpak build-bundle repo Ludonimbus.flatpak io.github.shupian0510.ludonimbus`。安装：`flatpak install --user Ludonimbus.flatpak`。

**权限**（manifest 的 `finish-args`）：网络、Wayland / X11、GPU；家目录（存档到处都是）；`/run/media`、`/mnt`、`/var/mnt`（SD 卡、U 盘、其他硬盘）；Steam 的 Flatpak（`~/.var/app/com.valvesoftware.Steam`）、规则认得的模拟器的 Flatpak、Heroic / Lutris / Bottles 的 Flatpak 文件夹（家目录权限不包括别的应用的 `~/.var/app`）；通知（`org.freedesktop.Notifications`）、密码（Secret Service，`org.freedesktop.secrets`：KWallet、GNOME 钥匙串）、托盘（`org.kde.StatusNotifierWatcher`；托盘要的 libayatana-appindicator 用 shared-modules 编进包里，Tauri 启动时加载它，没有就退出）。

**数据文件夹**：沙箱里的 XDG 文件夹指向 `~/.var/app/<id>`；应用和后台服务一启动就把它们改回用户自己的（`core::paths::use_host_folders`），所以数据仍在 `~/.config/Ludonimbus`，模拟器和 Steam 的文件夹也照常找得到：Steam 启动的包装器、Decky 插件都在沙箱外面，要读同一份数据。沙箱内外的进程号不一样，所以 `agent.json` 里记着 `flatpak: true`，另一边判断写它的程序还在不在时看端口有没有回应（`localapi::live_owner`）。

**在沙箱里做不到的**（应用直接说明原因，不会做一半）：

- **Steam 的文件**：沙箱里看不到 Steam 在不在运行，也不能关闭或启动它，所以不改 Steam 的文件：加到 Steam、修复快捷方式、写文件改启动选项、装 Millennium 都会提示改用 AppImage（`steam::outside_sandbox`）。Steam 插件连着时，启动选项和 Steam 云开关经插件由 Steam 自己改，照常可用。
- **沙箱外的进程**：不经包装器启动的游戏（直接打开、在模拟器里开）看不到，玩完后不会自动同步；经 Steam 启动（包装器）照常。包装器的运行标记一律当作「在运行」，被强制结束的会话由沙箱外的后台服务补做。
- **后台服务**：不能在沙箱里安装或启动 systemd 用户服务。游戏模式要后台服务时，在桌面模式的终端里运行 Linux 下载包的 `./install-linux.sh --flatpak`：写一个用 `flatpak run --command=ludonimbus-agent io.github.shupian0510.ludonimbus` 运行后台服务的用户服务（同样的优先级和 lingering）。它在沙箱里运行，限制同上；也可以直接装 Linux 下载包的后台服务（`./install-linux.sh`），两者用同一个数据文件夹。
- **托盘**：托盘图标要桌面有 StatusNotifier 宿主（Deck 的桌面模式 KDE 有）；做不出托盘图标时，关闭窗口就退出，不再藏到托盘里（再打开 Ludonimbus 时窗口照常出来）。
- **更新**：应用里的「更新」不管 Flatpak；用新的 `Ludonimbus.flatpak` 覆盖安装。

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
crates/core          同步引擎：存档扫描、快照、云盘、Steam 快捷方式、同步判定、Millennium 安装、给 Steam 插件用的本机接口（localapi）
crates/cli           ludonimbus 命令行
crates/runner        ludonimbus-run，Steam 快捷方式实际启动的程序
crates/agent         ludonimbus-agent，不带界面的后台服务（Steam Deck 游戏模式）
app/                 桌面应用（Tauri 2 + React + TypeScript），含手柄界面（app/src/handheld）；托盘运行时提供本机接口
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
- 同步判定规则：[`crates/core/src/cloud/decide.rs`](../crates/core/src/cloud/decide.rs) 顶部的表格；本机按判定做什么（队列、替换存档、不等云盘）在 [`crates/core/src/sync/`](../crates/core/src/sync/mod.rs)，`mod.rs` 顶部写着各部分在哪个文件

云端目录结构：

```text
Ludonimbus/devices/<设备>.json
Ludonimbus/games/<游戏>/game.json
Ludonimbus/games/<游戏>/snapshots/<版本>.zip
Ludonimbus/gamebackup/chunks/<blake3>                  游戏本体备份的数据块，所有游戏共用（见 plan-game-backup.md）
Ludonimbus/gamebackup/games/<游戏>/versions/<版本>.manifest  + <版本>.json
```

每个版本是一个只追加、不可修改的 zip 快照，带父版本指针和设备信息；最新的主线版本就是「云端当前版本」。
