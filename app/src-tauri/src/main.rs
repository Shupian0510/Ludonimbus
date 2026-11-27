#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod achievements;
mod agent;
mod art;
mod busy;
mod commands;
mod gamebackup;
mod handheld;
mod library;
mod nx;
mod quark_login;
mod savelibrary;
mod steam_plugin;
mod updater;
mod watcher;

use ludonimbus_core::{config, tr};
use tauri::menu::{Menu, MenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Manager, RunEvent, Runtime, WindowEvent};

fn tray_menu<R: Runtime>(app: &AppHandle<R>) -> tauri::Result<Menu<R>> {
    let open = MenuItem::with_id(app, "open", tr!("打开 Ludonimbus", "Open Ludonimbus"), true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", tr!("退出", "Quit"), true, None::<&str>)?;
    Menu::with_items(app, &[&open, &quit])
}

/// Rebuild the tray menu in the current language (after it changes in Settings).
pub fn relabel_tray<R: Runtime>(app: &AppHandle<R>) {
    if let (Some(tray), Ok(menu)) = (app.tray_by_id("main"), tray_menu(app)) {
        let _ = tray.set_menu(Some(menu));
    }
}

pub fn show_main<R: Runtime>(app: &AppHandle<R>) {
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.show();
        let _ = w.unminimize();
        let _ = w.set_focus();
    }
}

fn main() {
    let desktop = *handheld::mode() == handheld::Mode::Desktop;
    let mut builder = tauri::Builder::default();
    // The handheld screen and the wrapper's questions run as their own
    // process next to the tray app (see handheld.rs).
    if desktop {
        builder = builder.plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| show_main(app)));
    }
    builder
        .plugin(tauri_plugin_updater::Builder::new().build())
        .register_asynchronous_uri_scheme_protocol("ludoart", art::handle)
        .setup(move |app| {
            // Reading the configuration applies the interface language.
            let _ = config::load();
            if !desktop {
                if let Some(w) = app.get_webview_window("main") {
                    let _ = w.set_fullscreen(true);
                    let _ = w.set_focus();
                }
                return Ok(());
            }
            if let Err(e) = ludonimbus_core::cloud::move_secrets_out_of_config() {
                ludonimbus_core::logger::log("app", format!("could not move the Baidu SecretKey to the credential store: {e:#}"));
            }
            // Linux: the wrapper Steam's launch options point at lives outside the AppImage.
            #[cfg(unix)]
            if let Err(e) = ludonimbus_core::paths::install_runner() {
                ludonimbus_core::logger::log("app", format!("could not install the launch wrapper: {e}"));
            }
            let menu = tray_menu(app.handle())?;
            let mut tray = TrayIconBuilder::with_id("main")
                .tooltip("Ludonimbus")
                .menu(&menu)
                .show_menu_on_left_click(false)
                .on_menu_event(|app, e| match e.id.as_ref() {
                    "open" => show_main(app),
                    "quit" => app.exit(0),
                    _ => {}
                })
                .on_tray_icon_event(|tray, e| {
                    if let TrayIconEvent::Click { button: MouseButton::Left, button_state: MouseButtonState::Up, .. } = e {
                        show_main(tray.app_handle());
                    }
                });
            if let Some(icon) = app.default_window_icon() {
                tray = tray.icon(icon.clone());
            }
            tray.build(app)?;
            watcher::spawn(app.handle().clone());
            agent::spawn(app.handle().clone());
            // The Switch client's LAN connection, when turned on.
            nx::spawn();
            achievements::spawn(app.handle().clone());
            // Import the minutes Steam already counted for linked games, once
            // per game and device, before any new session is added to them.
            std::thread::spawn(|| {
                if let Ok(cfg) = ludonimbus_core::config::load() {
                    for g in cfg.games.iter().filter(|g| g.steam.is_some()) {
                        if ludonimbus_core::playtime::load_local(&g.key).is_none() {
                            let _ = ludonimbus_core::playtime::ensure(&cfg, g);
                        }
                    }
                }
            });
            let handle = app.handle().clone();
            std::thread::spawn(move || steam_plugin::refresh_installed(&handle));
            std::thread::spawn(|| {
                let Ok(cfg) = config::load() else { return };
                // Start OpenList early when the cloud is Quark, so the first sync doesn't wait for it.
                if matches!(cfg.cloud, config::CloudConfig::Quark { .. }) {
                    let _ = ludonimbus_core::openlist::ensure_running();
                }
                // A cloud that 0.6 wrote is brought to the current layout
                // now, so the first upload after playing doesn't wait for it.
                if let Err(e) = ludonimbus_core::cloud::upgrade_if_needed(&cfg) {
                    ludonimbus_core::logger::log("cloud", format!("the cloud was not upgraded: {e:#}"));
                }
            });
            Ok(())
        })
        .on_window_event(move |window, event| {
            // Keep running in the tray so the background watcher can back up
            // games started outside Steam. The handheld screen just closes,
            // and so do the other windows (the achievement popup, the Quark
            // sign-in): their code waits for them to be gone.
            if let WindowEvent::CloseRequested { api, .. } = event {
                if desktop && window.label() == "main" {
                    api.prevent_close();
                    let _ = window.hide();
                }
            }
        })
        .invoke_handler(tauri::generate_handler![
            commands::get_overview,
            commands::check_game,
            commands::sync_game,
            commands::game_lines,
            commands::use_game_line,
            commands::game_line_members,
            commands::other_saves,
            commands::take_save,
            commands::undo_replace,
            commands::join_suggestion,
            commands::join_answer,
            commands::backup_game,
            commands::list_snapshots,
            commands::set_shelf,
            commands::collection_stats,
            commands::screenshot_gallery,
            commands::screenshot_fetch,
            commands::screenshot_sync,
            commands::screenshot_reveal,
            commands::get_snapshot,
            commands::restore_snapshot,
            commands::set_snapshot,
            commands::delete_snapshot,
            commands::deleted_snapshots,
            commands::restore_deleted_snapshot,
            commands::purge_deleted_snapshot,
            commands::export_snapshot,
            commands::import_shared_save,
            commands::local_files,
            commands::open_path,
            commands::open_save_folder,
            commands::steam_accounts,
            commands::steam_shortcuts,
            commands::add_from_shortcut,
            commands::add_custom,
            commands::cloud_games,
            commands::add_from_cloud,
            commands::update_game,
            commands::remove_game,
            commands::relocate_auto,
            commands::set_game_exe,
            commands::pick_exe,
            commands::pick_folder,
            commands::folder_to_pattern,
            commands::steam_set_enabled,
            commands::steam_repair,
            commands::steam_cloud_apps,
            commands::steam_cloud_add,
            commands::steam_cloud_status,
            commands::steam_cloud_take_over,
            commands::steam_cloud_hand_back,
            commands::steam_cloud_open_properties,
            commands::steam_cloud_backup_all,
            commands::steam_cloud_take_over_all,
            commands::get_settings,
            commands::save_settings,
            commands::get_language,
            commands::set_language,
            commands::get_onboarding,
            commands::finish_onboarding,
            commands::openlist_status,
            commands::openlist_install,
            commands::openlist_pick_zip,
            commands::cloud_connect_quark,
            commands::cloud_use_folder,
            commands::cloud_folder_suggestions,
            commands::steam_find_roots,
            commands::steam_set_root,
            commands::cloud_connect_google,
            commands::cloud_disconnect,
            commands::cloud_test,
            commands::cloud_connect,
            commands::cloud_migrate,
            commands::cloud_dismiss_previous,
            commands::cloud_connect_baidu,
            library::scan_quick,
            library::scan_ludusavi,
            library::scan_folder,
            library::game_folders,
            library::add_found,
            library::detect_adapter,
            library::savefind_suggest,
            library::savefind_watch_start,
            library::savefind_watch_stop,
            library::sgdb_status,
            library::sgdb_set_key,
            library::sgdb_clear_key,
            library::sgdb_search,
            library::sgdb_images,
            library::art_set_url,
            library::art_set_file,
            library::pick_image,
            library::steam_add_game,
            library::steam_add_games,
            library::steam_remove_game,
            commands::cloud_cancel_login,
            commands::cloud_account,
            commands::list_devices,
            commands::ludusavi_search,
            commands::ludusavi_preview,
            steam_plugin::steam_plugin_status,
            steam_plugin::steam_plugin_install,
            steam_plugin::steam_plugin_remove,
            updater::check_update,
            updater::install_update,
            gamebackup::speed_test,
            gamebackup::speed_test_cancel,
            gamebackup::game_backup_info,
            gamebackup::game_backup_preview,
            gamebackup::game_backup_set,
            gamebackup::game_backup_start,
            gamebackup::game_backup_resolve_target,
            gamebackup::game_backup_restore,
            gamebackup::game_backup_delete,
            gamebackup::game_backup_cancel,
            gamebackup::game_backup_job,
            gamebackup::game_backup_take_install,
            gamebackup::game_backup_request_install,
            savelibrary::refresh_cloud_library,
            savelibrary::cloud_game_info,
            savelibrary::other_installs,
            savelibrary::add_install,
            savelibrary::link_games,
            savelibrary::unlink_game,
            savelibrary::transfer_plan,
            savelibrary::transfer_apply,
            handheld::launch_mode,
            handheld::prompt_answer,
            handheld::set_fullscreen,
            handheld::quit_handheld,
            handheld::handheld_info,
            handheld::set_handheld_mode,
            handheld::steam_add_ludonimbus,
            handheld::newer_version,
            handheld::dismiss_newer,
            nx::nx_status,
            nx::nx_set_enabled,
            nx::nx_start_pairing,
            nx::nx_cancel_pairing,
            nx::nx_remove_device,
            achievements::achievements_view,
            achievements::achievements_candidates,
            achievements::achievements_link,
            achievements::achievements_link_file,
            achievements::achievements_pick_file,
            achievements::achievements_unlink,
            achievements::achievements_refresh,
            achievements::achievements_icon,
            achievements::achievements_game_icon,
            achievements::achievements_set_popup,
            achievements::achievements_settings,
            achievements::achievements_save_settings,
            achievements::achievements_set_ps4_key,
            achievements::achievements_test_popup,
            achievements::achievement_toast_next,
            achievements::ra_connect,
            achievements::ra_disconnect,
            achievements::ra_recent_games,
            gamebackup::game_backup_pick_file,
        ])
        .build(tauri::generate_context!())
        .expect("error while running Ludonimbus")
        .run(move |_app, event| {
            if let RunEvent::Exit = event {
                if !desktop {
                    return;
                }
                agent::remove_agent_file();
                // OpenList (Quark) runs while the app does.
                ludonimbus_core::openlist::stop();
            }
        });
}
