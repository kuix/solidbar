mod commands;
mod locale;
mod panel;
mod shortcuts;
mod sink;
mod tray;
mod updates;

use std::sync::Arc;

use solidbar_core::auth::{FileTokenStore, KeychainTokenStore, TokenStore};
use solidbar_core::engine::{Engine, EngineDeps};
use solidbar_core::ports::{SystemClock, UrlOpener};
use tauri::menu::{Menu, PredefinedMenuItem, Submenu};
use tauri::{AppHandle, Manager};
use tauri_plugin_deep_link::DeepLinkExt;
use tauri_plugin_opener::OpenerExt;

fn main() {
    tauri::Builder::default()
        .plugin(
            tauri_plugin_log::Builder::new()
                .level(log::LevelFilter::Info)
                .build(),
        )
        .plugin(tauri_nspanel::init())
        .plugin(tauri_plugin_global_shortcut::Builder::new().build())
        .plugin(tauri_plugin_deep_link::init())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            None,
        ))
        .plugin(tauri_plugin_updater::Builder::new().build())
        // Edit commands (⌘C, ⌘V, ⌘Z…) need a menu even though it's never visible. The default
        // menu also has "Close Window" (⌘W), which would destroy the panel, so build our own.
        .menu(app_menu)
        .manage(panel::PanelState::default())
        .manage(shortcuts::Registered::default())
        .manage(updates::Updates::default())
        .invoke_handler(tauri::generate_handler![
            commands::bootstrap,
            commands::auth_login,
            commands::auth_cancel,
            commands::auth_logout,
            commands::select_org,
            commands::select_date,
            commands::refresh,
            commands::prepare_form,
            commands::save_entry,
            commands::delete_entries,
            commands::adjust_entry,
            commands::start_timer,
            commands::stop_timer,
            commands::start_break,
            commands::update_settings,
            commands::set_shortcut,
            commands::shortcuts_suspend,
            commands::shortcuts_resume,
            commands::set_autostart,
            commands::get_autostart,
            commands::panel_hide,
            commands::panel_set_pinned,
            commands::open_in_browser,
            commands::quit,
            commands::update_get,
            commands::update_check,
            commands::update_install,
        ])
        .setup(|app| {
            app.set_activation_policy(tauri::ActivationPolicy::Accessory);
            let handle = app.handle();
            let engine = start_engine(handle)?;
            app.manage(engine.clone());

            panel::init(handle)?;
            tray::init(handle)?;
            tray::set_status(handle, &engine.app_view().tray)?;
            shortcuts::register_all(handle);

            let links = engine.clone();
            app.deep_link().on_open_url(move |event| {
                for url in event.urls() {
                    links.deliver_redirect(url.as_str());
                }
            });
            tauri::async_runtime::spawn(engine.clone().run_periodic());
            // A debug build would offer to replace itself with the latest release.
            if !cfg!(debug_assertions) {
                tauri::async_runtime::spawn(updates::run_periodic(handle.clone()));
            }
            tauri::async_runtime::spawn(async move {
                // Failures show up in the panel; nothing more to do here.
                let _ = engine.refresh(false).await;
            });
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running Solidbar");
}

fn app_menu(app: &AppHandle) -> tauri::Result<Menu<tauri::Wry>> {
    let solidbar = Submenu::with_items(
        app,
        "Solidbar",
        true,
        &[
            &PredefinedMenuItem::about(app, Some("About Solidbar"), None)?,
            &PredefinedMenuItem::separator(app)?,
            &PredefinedMenuItem::quit(app, Some("Quit Solidbar"))?,
        ],
    )?;
    let edit = Submenu::with_items(
        app,
        "Edit",
        true,
        &[
            &PredefinedMenuItem::undo(app, None)?,
            &PredefinedMenuItem::redo(app, None)?,
            &PredefinedMenuItem::separator(app)?,
            &PredefinedMenuItem::cut(app, None)?,
            &PredefinedMenuItem::copy(app, None)?,
            &PredefinedMenuItem::paste(app, None)?,
            &PredefinedMenuItem::select_all(app, None)?,
        ],
    )?;
    Menu::with_items(app, &[&solidbar, &edit])
}

fn start_engine(app: &AppHandle) -> Result<Arc<Engine>, Box<dyn std::error::Error>> {
    let data_dir = app.path().app_data_dir()?;
    // Every debug rebuild changes the code signature, which makes the keychain prompt again.
    let tokens: Arc<dyn TokenStore> = if cfg!(debug_assertions) {
        Arc::new(FileTokenStore::new(data_dir.join("session-debug.json")))
    } else {
        Arc::new(KeychainTokenStore::new(&app.config().identifier)?)
    };
    Ok(Engine::new(EngineDeps {
        http: solidbar_core::http_client(),
        clock: Arc::new(SystemClock),
        tokens,
        sink: Arc::new(sink::TauriSink::new(app.clone())),
        opener: Arc::new(Browser(app.clone())),
        data_dir,
        hour12: locale::uses_12_hour_clock(),
        system_zone: jiff::tz::TimeZone::system(),
    }))
}

struct Browser(AppHandle);

impl UrlOpener for Browser {
    fn open(&self, url: &str) -> Result<(), String> {
        self.0
            .opener()
            .open_url(url, None::<&str>)
            .map_err(|error| error.to_string())
    }
}
