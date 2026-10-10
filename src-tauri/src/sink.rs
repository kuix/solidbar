//! Delivers engine updates to the webview, the menu bar and the window appearance.

use std::sync::Mutex;

use solidbar_core::ports::EventSink;
use solidbar_shared::ipc::{AppChanged, CatalogChanged, Event, NoticeShown};
use solidbar_shared::settings::Theme;
use solidbar_shared::view::{AppView, Catalog, Notice, TrayView};
use tauri::{AppHandle, Emitter, Manager};

use crate::{panel, tray};

pub struct TauriSink {
    app: AppHandle,
    /// Only touch AppKit when something visible changed.
    tray: Mutex<Option<TrayView>>,
    theme: Mutex<Option<Theme>>,
}

impl TauriSink {
    pub fn new(app: AppHandle) -> Self {
        Self {
            app,
            tray: Mutex::new(None),
            theme: Mutex::new(None),
        }
    }

    fn update_tray(&self, view: &TrayView) {
        let mut last = self.tray.lock().unwrap();
        if last.as_ref() == Some(view) {
            return;
        }
        if let Err(error) = tray::set_status(&self.app, view) {
            log::warn!("could not update the menu bar item: {error}");
        }
        *last = Some(view.clone());
    }

    fn update_theme(&self, theme: Theme) {
        let mut last = self.theme.lock().unwrap();
        if *last == Some(theme) {
            return;
        }
        if let Some(window) = self.app.get_webview_window(panel::LABEL) {
            let native = match theme {
                Theme::System => None,
                Theme::Dark => Some(tauri::Theme::Dark),
                Theme::Light => Some(tauri::Theme::Light),
            };
            if let Err(error) = window.set_theme(native) {
                log::warn!("could not change the appearance: {error}");
            }
        }
        *last = Some(theme);
    }
}

impl EventSink for TauriSink {
    fn app_changed(&self, view: &AppView) {
        self.update_tray(&view.tray);
        self.update_theme(view.settings.theme);
        let _ = self.app.emit(AppChanged::NAME, view);
    }

    fn catalog_changed(&self, catalog: &Catalog) {
        let _ = self.app.emit(CatalogChanged::NAME, catalog);
    }

    fn notice(&self, notice: &Notice) {
        let _ = self.app.emit(NoticeShown::NAME, notice);
    }
}
