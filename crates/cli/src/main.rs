//! `ludonimbus` command line: scripting and troubleshooting.

use anyhow::{anyhow, bail, Context, Result};
use clap::{Parser, Subcommand};
use ludonimbus_core::cloud::{self, oauth};
use ludonimbus_core::config::{self, CloudConfig};
use ludonimbus_core::snapshot::{self, Trigger};
use ludonimbus_core::sync::{self, Ctx, Resolution};
use ludonimbus_core::{discover, games, join, ludusavi, paths, steam, steamcloud};
use std::path::PathBuf;

mod gamebackup;

#[derive(Parser)]
#[command(name = "ludonimbus", version, about = "Cloud saves for Steam non-Steam games")]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// List configured games and their sync status
    Status { key: Option<String> },
    /// Show the save files found for a game
    Scan { key: String },
    /// Sync a game now
    Sync {
        key: String,
        #[arg(long, conflicts_with = "use_cloud")]
        use_local: bool,
        #[arg(long)]
        use_cloud: bool,
    },
    /// Create a manual snapshot (kept forever)
    Backup {
        key: String,
        #[arg(short, long, default_value = "")]
        message: String,
    },
    /// List cloud snapshots of a game
    Snapshots { key: String },
    /// Restore a snapshot (current saves are protected first)
    Restore { key: String, id: String },
    /// Show the save line a game could share with a version that keeps the
    /// same saves in other folders, or answer
    Join {
        key: String,
        /// Share that line from here on (then sync with it)
        #[arg(long, conflicts_with_all = ["decline", "leave"])]
        accept: bool,
        /// Don't suggest that line again
        #[arg(long, conflicts_with = "leave")]
        decline: bool,
        /// Stop sharing: back to this version's own line
        #[arg(long)]
        leave: bool,
    },
    /// Show the save lines of a game, or make it follow another one on this
    /// device
    Lines {
        key: String,
        /// Share saves with the game's other devices
        #[arg(long, conflicts_with_all = ["own", "follow"])]
        shared: bool,
        /// Keep this device's saves to itself
        #[arg(long, conflicts_with = "follow")]
        own: bool,
        /// Follow the line another device keeps to itself (its id)
        #[arg(long)]
        follow: Option<String>,
        /// A name for this device's own line
        #[arg(long, requires = "own")]
        name: Option<String>,
    },
    /// Steam integration
    #[command(subcommand)]
    Steam(SteamCmd),
    /// Games installed through Steam: backup only, taking over from Steam
    /// Cloud, handing back
    #[command(subcommand)]
    SteamCloud(SteamCloudCmd),
    /// Cloud provider
    #[command(subcommand)]
    Cloud(CloudCmd),
    /// Rename this device
    DeviceName { name: String },
    /// Find games on this PC (Steam shortcuts, Epic, GOG; optionally Ludusavi
    /// and a folder)
    Discover {
        /// Also run Ludusavi's full scan (about a minute)
        #[arg(long)]
        ludusavi: bool,
        /// Also look for games in this folder
        #[arg(long)]
        folder: Option<PathBuf>,
    },
    /// Search Ludusavi's save database
    Ludusavi {
        query: String,
        /// Show the save files found for this exact title instead of searching
        #[arg(long)]
        files: bool,
    },
    /// Print the data folder
    Paths,
    /// Measure upload / download speed of the current cloud (test files are deleted)
    Speedtest(gamebackup::SpeedArgs),
    /// Game backups: the game's own files in the cloud
    #[command(subcommand)]
    GameBackup(gamebackup::Cmd),
    /// Games' rules (rules/games/)
    #[command(subcommand)]
    Rules(RulesCmd),
}

#[derive(Subcommand)]
enum RulesCmd {
    /// Look over game rule files before they go in: their fields and their
    /// sample saves (rules/README.md)
    Check {
        #[arg(required = true)]
        files: Vec<PathBuf>,
    },
}

#[derive(Subcommand)]
enum SteamCmd {
    /// Steam accounts on this PC
    Accounts,
    /// Non-Steam shortcuts of an account
    Shortcuts {
        #[arg(long)]
        account: Option<String>,
    },
    /// Add a game from a shortcut
    Add {
        appid: u32,
        #[arg(long)]
        account: Option<String>,
        #[arg(long)]
        name: Option<String>,
        #[arg(long)]
        adapter: Option<String>,
        /// Save pattern(s), e.g. "{install}/save/**"
        #[arg(long = "save")]
        saves: Vec<String>,
    },
    /// Launch the game through ludonimbus-run from Steam
    Enable { key: String },
    /// Restore the shortcut to launch the game directly
    Disable { key: String },
    /// Fix a shortcut that was edited or recreated
    Repair { key: String },
    /// Check every linked shortcut
    Health,
}

#[derive(Subcommand)]
enum SteamCloudCmd {
    /// Games Steam installed, and how each stands
    List,
    /// Add games Steam installed (backup only)
    Add {
        #[arg(required = true)]
        appids: Vec<u32>,
    },
    /// How a game stands with Steam Cloud
    Status { key: String },
    /// Let Ludonimbus keep a game's saves (Steam Cloud must be off for it)
    TakeOver {
        key: String,
        /// Leave the launch options alone: the game is synced in the
        /// background while it isn't running
        #[arg(long)]
        no_wrapper: bool,
    },
    /// Give a game back to Steam Cloud
    HandBack { key: String },
    /// Add every game Steam installed whose save location is known (backup only)
    BackupAll,
    /// Take over every game that is backup only (Steam Cloud must be off for all games)
    TakeOverAll {
        #[arg(long)]
        no_wrapper: bool,
    },
    /// Give every game back to Steam Cloud
    HandBackAll,
}

fn print_done(list: &[steamcloud::Done]) {
    for d in list {
        let key = d.key.as_deref().unwrap_or("-");
        match &d.error {
            None => println!("{:<10} {key}  完成", d.appid),
            Some(e) => println!("{:<10} {key}  失败：{e}", d.appid),
        }
    }
}

fn steam_cloud(cmd: SteamCloudCmd) -> Result<()> {
    match cmd {
        SteamCloudCmd::List => {
            for a in steamcloud::apps(&config::load()?) {
                println!(
                    "{:<10} {:<40} {:<8} {:<16} {}{}",
                    a.appid,
                    a.name,
                    format!("{:?}", a.state),
                    a.sync_state.unwrap_or_else(|| "-".into()),
                    if !a.cloud_used {
                        "[没有 Steam 云] "
                    } else if a.cloud_enabled {
                        ""
                    } else {
                        "[Steam 云已关] "
                    },
                    a.key.map(|k| format!("[已添加: {k}]")).unwrap_or_default()
                );
            }
        }
        SteamCloudCmd::Add { appids } => print_done(&steamcloud::add_many(&config::load()?, &appids)),
        SteamCloudCmd::Status { key } => {
            let cfg = config::load()?;
            println!("{}", serde_json::to_string_pretty(&steamcloud::status(&cfg, cfg.require_game(&key)?)?)?);
        }
        SteamCloudCmd::TakeOver { key, no_wrapper } => {
            let g = steamcloud::take_over(&Ctx::load()?, &key, !no_wrapper)?;
            println!("已接管「{}」：存档由 Ludonimbus 同步", g.name);
        }
        SteamCloudCmd::HandBack { key } => {
            let g = steamcloud::hand_back(&Ctx::load()?, &key)?;
            println!("已把「{}」交还给 Steam 云：请在 Steam 里重新打开它的云存档", g.name);
        }
        SteamCloudCmd::BackupAll => print_done(&steamcloud::backup_all(&config::load()?)),
        SteamCloudCmd::TakeOverAll { no_wrapper } => print_done(&steamcloud::take_over_all(&Ctx::load()?, !no_wrapper)?),
        SteamCloudCmd::HandBackAll => print_done(&steamcloud::hand_back_all(&Ctx::load()?)),
    }
    Ok(())
}

#[derive(Subcommand)]
enum CloudCmd {
    /// Use a folder as the cloud (NAS, USB, Google Drive for desktop folder…)
    Folder { path: PathBuf },
    /// Sign in to Google Drive in the browser
    Google {
        #[arg(long, default_value = "")]
        client_id: String,
        #[arg(long, default_value = "")]
        client_secret: String,
    },
    /// Use a WebDAV server (NAS, Nextcloud, Jianguoyun, OpenList…). The
    /// password is read from LUDONIMBUS_SECRET or asked for.
    Webdav {
        #[arg(long)]
        url: String,
        #[arg(long, default_value = "")]
        user: String,
        #[arg(long, default_value = "Ludonimbus")]
        folder: String,
    },
    /// Use S3-compatible storage. The secret key is read from
    /// LUDONIMBUS_SECRET or asked for.
    S3 {
        #[arg(long)]
        endpoint: String,
        #[arg(long, default_value = "")]
        region: String,
        #[arg(long)]
        bucket: String,
        #[arg(long, default_value = "Ludonimbus")]
        prefix: String,
        #[arg(long)]
        access_key: String,
        #[arg(long)]
        path_style: bool,
    },
    /// Sign in to Baidu Netdisk (open the shown page or scan the QR code)
    Baidu {
        #[arg(long, default_value = "")]
        app_key: String,
        #[arg(long, default_value = "")]
        secret_key: String,
        #[arg(long, default_value = "")]
        app_name: String,
    },
    /// Copy all saves from the previous cloud into the current one
    Migrate,
    /// Show the connected account
    Info,
    /// List devices known to the cloud
    Devices,
    /// Forget the cloud connection (cloud data is kept)
    Disconnect,
}

fn main() {
    if let Err(e) = run(Cli::parse()) {
        eprintln!("错误：{e:#}");
        std::process::exit(1);
    }
}

pub(crate) fn human(bytes: u64) -> String {
    const U: [&str; 4] = ["B", "KB", "MB", "GB"];
    let mut v = bytes as f64;
    let mut i = 0;
    while v >= 1024.0 && i < U.len() - 1 {
        v /= 1024.0;
        i += 1;
    }
    if i == 0 {
        format!("{bytes} B")
    } else {
        format!("{v:.1} {}", U[i])
    }
}

fn read_secret(prompt: &str) -> Result<String> {
    if let Ok(s) = std::env::var("LUDONIMBUS_SECRET") {
        if !s.is_empty() {
            return Ok(s);
        }
    }
    eprint!("{prompt}：");
    let mut line = String::new();
    std::io::stdin().read_line(&mut line)?;
    Ok(line.trim_end_matches(['\r', '\n']).to_string())
}

/// Test WebDAV / S3 settings, then switch to them.
fn connect_store(c: CloudConfig) -> Result<()> {
    let prompt = if matches!(c, CloudConfig::S3 { .. }) { "Secret Key" } else { "密码" };
    let secret = read_secret(prompt)?;
    let timeout = cloud::timeout(&config::load()?).max(std::time::Duration::from_secs(15));
    let account = cloud::probe(&c, Some(&secret), timeout)?;
    cloud::switch_to(c, Some(&secret))?;
    println!("已连接：{}", account.label);
    hint_migrate()
}

fn hint_migrate() -> Result<()> {
    if let Some(p) = config::load()?.previous_cloud {
        println!("要把原来 {} 里的存档复制过来，请运行：ludonimbus cloud migrate", cloud::kind_name(&p));
    }
    Ok(())
}

fn account_or_default(account: Option<String>) -> Result<String> {
    if let Some(a) = account {
        return Ok(a);
    }
    let cfg = config::load()?;
    let root = steam::root(&cfg).ok_or_else(|| anyhow!("没有找到 Steam"))?;
    steam::default_account(&cfg, &root).ok_or_else(|| anyhow!("没有找到 Steam 账号"))
}

fn run(cli: Cli) -> Result<()> {
    match cli.cmd {
        Cmd::Speedtest(a) => gamebackup::speed(a)?,
        Cmd::GameBackup(c) => gamebackup::run(c)?,
        Cmd::Status { key } => {
            let ctx = Ctx::load()?;
            for g in ctx.cfg.games.iter().filter(|g| key.as_ref().is_none_or(|k| k == &g.key)) {
                let r = sync::check(&ctx, g);
                println!(
                    "{:<24} {:<14} 本地 {} 个文件 {:>9}  云端 {} 个版本{}",
                    g.key,
                    format!("{:?}", r.status),
                    r.local_files,
                    human(r.local_size),
                    r.snapshot_count,
                    r.message.map(|m| format!("  ({m})")).unwrap_or_default()
                );
                println!("    Steam: {:?}", steam::health(&ctx.cfg, g));
            }
        }
        Cmd::Scan { key } => {
            let cfg = config::load()?;
            let g = cfg.require_game(&key)?;
            let local = snapshot::local_state(g)?;
            for (name, p) in &local.roots {
                println!("{{{name}}} = {}", p.display());
            }
            for f in &local.files {
                println!("{:>10}  {}", human(f.size), f.logical);
            }
            println!("共 {} 个文件，{}，内容哈希 {}", local.files.len(), human(local.total_size), local.hash);
        }
        Cmd::Sync { key, use_local, use_cloud } => {
            let ctx = Ctx::load()?;
            let g = ctx.game(&key)?;
            let res = if use_local {
                Resolution::UseLocal
            } else if use_cloud {
                Resolution::UseCloud
            } else {
                Resolution::Auto
            };
            let out = sync::sync(&ctx, &g, res, Trigger::SyncNow)?;
            println!("{}", serde_json::to_string_pretty(&out)?);
        }
        Cmd::Backup { key, message } => {
            let ctx = Ctx::load()?;
            let g = ctx.game(&key)?;
            println!("{}", serde_json::to_string_pretty(&sync::backup_now(&ctx, &g, &message)?)?);
        }
        Cmd::Snapshots { key } => {
            let ctx = Ctx::load()?;
            let list = ctx.cloud()?.list_snapshots(&key)?;
            let head = sync::mainline_head(&list).map(|h| h.id);
            for s in list.iter().rev() {
                println!(
                    "{} {} {:<14} {:<12} {:>9} {}{}{}",
                    s.id,
                    s.created_at.with_timezone(&chrono::Local).format("%Y-%m-%d %H:%M"),
                    s.device_name,
                    format!("{:?}", s.trigger),
                    human(s.total_size),
                    if Some(&s.id) == head.as_ref() { "[最新] " } else { "" },
                    if s.locked { "[锁定] " } else { "" },
                    s.comment
                );
            }
        }
        Cmd::Restore { key, id } => {
            let ctx = Ctx::load()?;
            let g = ctx.game(&key)?;
            println!("{}", serde_json::to_string_pretty(&sync::restore(&ctx, &g, &id)?)?);
        }
        Cmd::Join { key, accept, decline, leave } => {
            let ctx = Ctx::load()?;
            let g = ctx.game(&key)?;
            if leave {
                println!("{}", serde_json::to_string_pretty(&join::leave(&ctx, &g)?)?);
                return Ok(());
            }
            let Some(s) = join::suggestion(&ctx, &g)? else {
                println!("没有可以共用的存档线");
                return Ok(());
            };
            println!("{}", serde_json::to_string_pretty(&s)?);
            if accept {
                println!("{}", serde_json::to_string_pretty(&join::accept(&ctx, &g, &s)?)?);
            } else if decline {
                join::decline(&key, &s.def)?;
            }
        }
        Cmd::Lines { key, shared, own, follow, name } => {
            let choice = match (shared, own, follow) {
                (true, _, _) => Some(sync::LineChoice::Shared),
                (_, true, _) => Some(sync::LineChoice::Independent { name }),
                (_, _, Some(line)) => Some(sync::LineChoice::Follow { line }),
                _ => None,
            };
            if let Some(choice) = choice {
                let ctx = Ctx::load()?;
                let g = ctx.game(&key)?;
                println!("{}", serde_json::to_string_pretty(&sync::use_line(&ctx, &g, choice)?)?);
            }
            let ctx = Ctx::load()?;
            let g = ctx.game(&key)?;
            for l in sync::lines(&ctx, &g)? {
                println!(
                    "{} {:<10} {:<20} {}",
                    if l.current { "*" } else { " " },
                    l.id,
                    if l.shared { "共用".to_string() } else { l.name },
                    l.devices.join(", ")
                );
            }
        }
        Cmd::SteamCloud(cmd) => steam_cloud(cmd)?,
        Cmd::Rules(RulesCmd::Check { files }) => {
            let mut bad = 0;
            for f in &files {
                let c = ludonimbus_core::crosssaves::check_file(f).with_context(|| f.display().to_string())?;
                println!("{} {}", if c.problems.is_empty() { "ok" } else { "✗ " }, f.display());
                for n in &c.notes {
                    println!("    {n}");
                }
                for p in &c.problems {
                    println!("  ✗ {p}");
                }
                bad += usize::from(!c.problems.is_empty());
            }
            if bad > 0 {
                bail!("{bad} of {} rule file(s) need changes", files.len());
            }
        }
        Cmd::Steam(cmd) => match cmd {
            SteamCmd::Accounts => {
                let cfg = config::load()?;
                let root = steam::root(&cfg).ok_or_else(|| anyhow!("没有找到 Steam"))?;
                println!("Steam: {}", root.display());
                for a in steam::accounts(&root) {
                    println!(
                        "{:<12} {:<20} 快捷方式 {}{}",
                        a.id,
                        a.persona.unwrap_or_default(),
                        a.shortcut_count,
                        if a.most_recent { "  (最近登录)" } else { "" }
                    );
                }
            }
            SteamCmd::Shortcuts { account } => {
                let account = account_or_default(account)?;
                let cfg = config::load()?;
                for c in games::shortcut_candidates(&cfg, &account)? {
                    println!(
                        "{:<11} {:<24} 规则:{:<20} {}{}",
                        c.appid,
                        c.suggested_name,
                        c.adapter.unwrap_or_else(|| "-".into()),
                        c.exe,
                        c.added_key.map(|k| format!("  [已添加: {k}]")).unwrap_or_default()
                    );
                }
            }
            SteamCmd::Add { appid, account, name, adapter, saves } => {
                let account = account_or_default(account)?;
                let g = games::add_from_shortcut(&account, appid, games::NewGame { name, adapter, saves, key: None, ludusavi_title: None })?;
                println!("已添加 {}（{}）", g.name, g.key);
            }
            SteamCmd::Enable { key } => {
                games::steam_enable(&key)?;
                println!("已启用：从 Steam 启动时会自动同步存档");
            }
            SteamCmd::Disable { key } => {
                games::steam_disable(&key)?;
                println!("已恢复原来的快捷方式");
            }
            SteamCmd::Repair { key } => {
                games::steam_repair(&key)?;
                println!("已修复");
            }
            SteamCmd::Health => {
                let cfg = config::load()?;
                for g in &cfg.games {
                    println!("{:<24} {:?}", g.key, steam::health(&cfg, g));
                }
            }
        },
        Cmd::Cloud(cmd) => match cmd {
            CloudCmd::Folder { path } => {
                std::fs::create_dir_all(&path)?;
                cloud::switch_to(CloudConfig::LocalFolder { path: path.clone() }, None)?;
                println!("云端已设置为文件夹 {}", path.display());
                hint_migrate()?;
            }
            CloudCmd::Webdav { url, user, folder } => {
                let c = CloudConfig::WebDav { url, username: user, folder };
                connect_store(c)?;
            }
            CloudCmd::S3 { endpoint, region, bucket, prefix, access_key, path_style } => {
                let c = CloudConfig::S3 { endpoint, region, bucket, prefix, access_key_id: access_key, path_style };
                connect_store(c)?;
            }
            CloudCmd::Baidu { app_key, secret_key, app_name } => {
                let cancel = std::sync::atomic::AtomicBool::new(false);
                let name = cloud::connect_baidu(
                    &app_key,
                    &secret_key,
                    &app_name,
                    |code| {
                        println!("请在浏览器中登录百度账号并输入授权码：{}", code.user_code);
                        println!("授权页面：{}", code.browser_url());
                        if !code.qrcode_url.is_empty() {
                            println!("也可以用百度网盘 App 扫描二维码：{}", code.qrcode_url);
                        }
                        let _ = opener::open_browser(code.browser_url());
                        println!("等待授权…");
                    },
                    &cancel,
                )?;
                println!("已连接百度网盘：{name}");
                hint_migrate()?;
            }
            CloudCmd::Migrate => {
                let r = cloud::migrate_previous(|p| {
                    if p.done < p.total {
                        println!("[{}/{}] {}", p.done + 1, p.total, p.game);
                    }
                })?;
                println!("已复制 {} 个版本（{}），{} 个已存在而跳过，涉及 {} 个游戏", r.copied, human(r.bytes), r.skipped, r.games);
            }
            CloudCmd::Google { client_id, client_secret } => {
                let client = oauth::GoogleClient::resolve(&client_id, &client_secret).ok_or_else(|| anyhow!("需要 --client-id（发布版会内置）"))?;
                println!("请在打开的浏览器中登录 Google 账号…");
                let auth = oauth::authorize(&client, std::time::Duration::from_secs(300))?;
                oauth::store_refresh_token(&auth.refresh_token)?;
                cloud::switch_to(
                    CloudConfig::GoogleDrive {
                        client_id: client_id.clone(),
                        client_secret: client_secret.clone(),
                        folder: "Ludonimbus".into(),
                        account: auth.email.clone(),
                    },
                    None,
                )?;
                println!("已连接 Google Drive：{}", auth.email.unwrap_or_default());
                hint_migrate()?;
            }
            CloudCmd::Info => {
                let ctx = Ctx::load()?;
                let a = ctx.cloud()?.account()?;
                println!("{}  {}", ctx.cloud()?.kind(), a.label);
                if let (Some(u), Some(t)) = (a.used, a.total) {
                    println!("已用 {} / {}", human(u), human(t));
                }
            }
            CloudCmd::Devices => {
                let ctx = Ctx::load()?;
                for d in ctx.cloud()?.list_devices()? {
                    println!("{:<30} {:<16} {}", d.id, d.name, d.last_seen.with_timezone(&chrono::Local).format("%Y-%m-%d %H:%M"));
                }
            }
            CloudCmd::Disconnect => {
                cloud::disconnect()?;
                println!("已断开（云端数据保留）");
            }
        },
        Cmd::DeviceName { name } => {
            if name.trim().is_empty() {
                bail!("名称不能为空");
            }
            config::update(|c| {
                c.device.name = name.trim().to_string();
                Ok(())
            })?;
            let ctx = Ctx::load()?;
            if let Ok(c) = ctx.cloud() {
                let _ = c.put_device(&cloud::device_info(&ctx.cfg));
            }
            println!("设备名已改为 {name}");
        }
        Cmd::Discover { ludusavi: full, folder } => {
            let cfg = config::load()?;
            let mut list = discover::quick(&cfg);
            if let Some(dir) = folder {
                list.extend(discover::folder_scan(&cfg, &dir));
            }
            if full {
                list.extend(discover::ludusavi_scan(&cfg)?);
            }
            for f in &list {
                let rule =
                    f.adapter_name.clone().or_else(|| f.ludusavi_title.as_ref().map(|t| format!("Ludusavi: {t}"))).unwrap_or_else(|| "需要指定存档".into());
                let mut flags = Vec::new();
                if f.steam_cloud {
                    flags.push("Steam 云存档".to_string());
                }
                if f.store_game {
                    flags.push("Steam 商店游戏".to_string());
                }
                if let Some(k) = &f.added_key {
                    flags.push(format!("已添加：{k}"));
                }
                if f.save_files > 0 {
                    flags.push(format!("{} 个存档文件 {}", f.save_files, human(f.save_bytes)));
                }
                if f.source == "emulator" && !f.args.is_empty() {
                    flags.push(format!("启动：{}", f.args));
                }
                println!("{:<9} {:<40} {:<32} {}", f.source, f.name, rule, flags.join("，"));
            }
            println!("共 {} 个", list.len());
        }
        Cmd::Ludusavi { query, files } => {
            if files {
                for f in ludusavi::resolve_files(&query)? {
                    println!("{}", f.display());
                }
            } else {
                for t in ludusavi::search(&query)? {
                    println!("{t}");
                }
            }
        }
        Cmd::Paths => {
            println!("数据目录：{}", paths::data_dir().display());
            println!("启动包装器：{}", paths::runner_exe().display());
            println!("Ludusavi：{}", ludusavi::exe_path().map(|p| p.display().to_string()).unwrap_or_else(|| "未找到".into()));
        }
    }
    Ok(())
}
