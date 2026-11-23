//! `ludonimbus` command line: scripting and troubleshooting.

use anyhow::{anyhow, bail, Result};
use clap::{Parser, Subcommand};
use ludonimbus_core::cloud::{self, oauth};
use ludonimbus_core::config::{self, CloudConfig};
use ludonimbus_core::snapshot::{self, Trigger};
use ludonimbus_core::sync::{self, Ctx, Resolution};
use ludonimbus_core::{games, ludusavi, paths, steam};
use std::path::PathBuf;

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
    /// Steam integration
    #[command(subcommand)]
    Steam(SteamCmd),
    /// Cloud provider
    #[command(subcommand)]
    Cloud(CloudCmd),
    /// Rename this device
    DeviceName { name: String },
    /// Search Ludusavi's save database
    Ludusavi {
        query: String,
        /// Show the save files found for this exact title instead of searching
        #[arg(long)]
        files: bool,
    },
    /// Print the data folder
    Paths,
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

fn human(bytes: u64) -> String {
    const U: [&str; 4] = ["B", "KB", "MB", "GB"];
    let mut v = bytes as f64;
    let mut i = 0;
    while v >= 1024.0 && i < U.len() - 1 {
        v /= 1024.0;
        i += 1;
    }
    if i == 0 { format!("{bytes} B") } else { format!("{v:.1} {}", U[i]) }
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
                let g = games::add_from_shortcut(
                    &account,
                    appid,
                    games::NewGame { name, adapter, saves, key: None },
                )?;
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
            CloudCmd::S3 {
                endpoint,
                region,
                bucket,
                prefix,
                access_key,
                path_style,
            } => {
                let c = CloudConfig::S3 {
                    endpoint,
                    region,
                    bucket,
                    prefix,
                    access_key_id: access_key,
                    path_style,
                };
                connect_store(c)?;
            }
            CloudCmd::Baidu {
                app_key,
                secret_key,
                app_name,
            } => {
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
                println!(
                    "已复制 {} 个版本（{}），{} 个已存在而跳过，涉及 {} 个游戏",
                    r.copied,
                    human(r.bytes),
                    r.skipped,
                    r.games
                );
            }
            CloudCmd::Google { client_id, client_secret } => {
                let client = oauth::GoogleClient::resolve(&client_id, &client_secret)
                    .ok_or_else(|| anyhow!("需要 --client-id（发布版会内置）"))?;
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
