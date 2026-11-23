#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod agent;
mod art;
mod busy;
mod commands;
mod library;
mod quark_login;
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
    tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| show_main(app)))
        .plugin(tauri_plugin_updater::Builder::new().build())
        .register_uri_scheme_protocol("ludoart", art::handle)
        .setup(|app| {
            // Reading the configuration applies the interface language.
            let _ = config::load();
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
                    if let TrayIconEvent::Click {
                        button: MouseButton::Left,
                        button_state: MouseButtonState::Up,
                        ..
                    } = e
                    {
                        show_main(tray.app_handle());
                    }
                });
            if let Some(icon) = app.default_window_icon() {
                tray = tray.icon(icon.clone());
            }
            tray.build(app)?;
            watcher::spawn(app.handle().clone());
            agent::spawn(app.handle().clone());
            let handle = app.handle().clone();
            std::thread::spawn(move || steam_plugin::refresh_installed(&handle));
            // Start OpenList early when the cloud is Quark, so the first sync doesn't wait for it.
            std::thread::spawn(|| {
                if matches!(config::load().map(|c| c.cloud), Ok(config::CloudConfig::Quark { .. })) {
                    let _ = ludonimbus_core::openlist::ensure_running();
                }
            });
            Ok(())
        })
        .on_window_event(|window, event| {
            // Keep running in the tray so the background watcher can back up
            // games started outside Steam.
            if let WindowEvent::CloseRequested { api, .. } = event {
                api.prevent_close();
                let _ = window.hide();
            }
        })
        .invoke_handler(tauri::generate_handler![
            commands::get_overview,
            commands::check_game,
            commands::sync_game,
            commands::backup_game,
            commands::list_snapshots,
            commands::get_snapshot,
            commands::restore_snapshot,
            commands::set_snapshot,
            commands::delete_snapshot,
            commands::export_snapshot,
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
        ])
        .build(tauri::generate_context!())
        .expect("error while running Ludonimbus")
        .run(|_app, event| {
            if let RunEvent::Exit = event {
                agent::remove_agent_file();
                // OpenList (Quark) runs while the app does.
                ludonimbus_core::openlist::stop();
            }
        });
}
