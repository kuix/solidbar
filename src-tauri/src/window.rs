//! Window mode: dragged away from the menu bar and let go, the panel becomes an ordinary
//! window, with a Dock icon and a place in ⌘Tab, that stays where it's put (closing it only
//! hides it) until "Dock to menu bar" in its header puts the panel back under the icon.
//! Solidbar starts in the mode it was left in, and the window where it was.
//!
//! It's a window of its own rather than the panel changed into one: the panel's style mask
//! can't change once its web view is up (see `panel.rs`), and this one is titled, resizable and
//! activating from the start. Both web views show the same engine state.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use objc2_app_kit::NSWindow;
use objc2_foundation::{NSPoint, ns_string};
use serde::{Deserialize, Serialize};
use solidbar_core::engine::Engine;
use solidbar_shared::ipc::{Event, ShowReason, WindowShown};
use tauri::window::{Effect, EffectState, EffectsBuilder};
use tauri::{
    ActivationPolicy, AppHandle, Emitter, LogicalPosition, Manager, TitleBarStyle, WebviewUrl,
    WebviewWindow, WebviewWindowBuilder, WindowEvent,
};

use crate::{panel, tray};

pub const LABEL: &str = "window";

pub const MENU_HIDE: &str = "app-hide";
pub const MENU_MINIMIZE: &str = "window-minimize";
pub const MENU_ZOOM: &str = "window-zoom";
pub const MENU_CLOSE: &str = "window-close";

/// The panel's size, which the window starts with; narrower, the header wouldn't fit.
const WIDTH: f64 = 380.0;
const HEIGHT: f64 = 520.0;
const MIN_HEIGHT: f64 = 360.0;
/// The close button's left edge and the window buttons' middle, in points from the top left
/// corner: in line with the header's buttons (`.header` in app.css).
const WINDOW_BUTTONS: LogicalPosition<f64> = LogicalPosition { x: 13.0, y: 21.0 };

#[derive(Default)]
pub struct WindowMode {
    windowed: AtomicBool,
}

/// `window.json` in the app's data directory.
#[derive(Default, Serialize, Deserialize)]
struct Saved {
    windowed: bool,
}

pub fn is_windowed(app: &AppHandle) -> bool {
    app.state::<WindowMode>().windowed.load(Ordering::Relaxed)
}

/// At launch: in the window again, if that's how Solidbar was left.
pub fn init(app: &AppHandle) {
    let saved = app
        .path()
        .app_data_dir()
        .ok()
        .and_then(|directory| std::fs::read(directory.join("window.json")).ok())
        .and_then(|bytes| serde_json::from_slice::<Saved>(&bytes).ok())
        .unwrap_or_default();
    if saved.windowed {
        app.state::<WindowMode>()
            .windowed
            .store(true, Ordering::Relaxed);
        let _ = app.set_activation_policy(ActivationPolicy::Regular);
        show(app, ShowReason::Toggle);
    }
}

/// Made while the panel is dragged, so it has loaded by the time the panel is let go.
pub fn prepare(app: &AppHandle) {
    let _ = window(app);
}

/// The panel was dragged away from the menu bar and let go there: the window takes its place.
pub fn undock(app: &AppHandle, top_left: NSPoint) {
    set_windowed(app, true);
    let _ = app.set_activation_policy(ActivationPolicy::Regular);
    let Some(window) = window(app) else {
        return;
    };
    if let Some(ns_window) = ns_window(&window) {
        ns_window.setFrameTopLeftPoint(top_left);
    }
    panel::hide(app);
    reveal(app, &window, ShowReason::Toggle);
}

/// "Dock to menu bar": the panel under the icon again, and no window, Dock icon or ⌘Tab.
pub fn dock(app: &AppHandle) {
    set_windowed(app, false);
    if let Some(window) = app.get_webview_window(LABEL) {
        let _ = window.hide();
    }
    let _ = app.set_activation_policy(ActivationPolicy::Accessory);
    panel::show_docked(app, tray::anchor(app));
}

/// From ⌥⌘N, the Dock icon, or the menu bar icon and ⌥⌘T (through `toggle`).
pub fn show(app: &AppHandle, reason: ShowReason) {
    let Some(window) = window(app) else {
        return;
    };
    reveal(app, &window, reason);
    // Back to today, with fresh data if it's getting old, as when the panel opens.
    let engine = app.state::<Arc<Engine>>().inner().clone();
    tauri::async_runtime::spawn(async move { engine.panel_opened().await });
}

/// The menu bar icon and ⌥⌘T: away if it's what you're looking at, otherwise to the front.
pub fn toggle(app: &AppHandle) {
    let in_front = app.get_webview_window(LABEL).is_some_and(|window| {
        window.is_visible().unwrap_or(false)
            && window.is_focused().unwrap_or(false)
            && !window.is_minimized().unwrap_or(false)
    });
    if in_front {
        // Like ⌘H, so the app you came from has the focus again.
        let _ = app.hide();
    } else {
        show(app, ShowReason::Toggle);
    }
}

/// Solidbar › Hide (⌘H), and Window › Minimize, Zoom and Close, which only ever act on the
/// window.
pub fn menu_event(app: &AppHandle, id: &str) {
    if id == MENU_HIDE {
        // As any app hides; in the panel's place, the panel goes away as with ⎋.
        if is_windowed(app) {
            let _ = app.hide();
        } else {
            panel::hide(app);
        }
        return;
    }
    let Some(window) = app.get_webview_window(LABEL).filter(|_| is_windowed(app)) else {
        return;
    };
    match id {
        MENU_MINIMIZE => {
            let _ = window.minimize();
        }
        MENU_ZOOM => {
            let _ = if window.is_maximized().unwrap_or(false) {
                window.unmaximize()
            } else {
                window.maximize()
            };
        }
        MENU_CLOSE => {
            let _ = window.hide();
        }
        _ => {}
    }
}

fn set_windowed(app: &AppHandle, windowed: bool) {
    app.state::<WindowMode>()
        .windowed
        .store(windowed, Ordering::Relaxed);
    let Ok(directory) = app.path().app_data_dir() else {
        return;
    };
    let saved = serde_json::to_vec(&Saved { windowed }).expect("a bool serializes");
    if let Err(error) = std::fs::write(directory.join("window.json"), saved) {
        log::warn!("couldn't save the window mode: {error}");
    }
}

fn reveal(app: &AppHandle, window: &WebviewWindow, reason: ShowReason) {
    // After `toggle` hid the app.
    let _ = app.show();
    let _ = window.unminimize();
    let _ = window.show();
    let _ = window.set_focus();
    let _ = app.emit(WindowShown::NAME, reason);
}

/// The window, made the first time it's needed.
fn window(app: &AppHandle) -> Option<WebviewWindow> {
    if let Some(window) = app.get_webview_window(LABEL) {
        return Some(window);
    }
    match build(app) {
        Ok(window) => Some(window),
        Err(error) => {
            log::warn!("couldn't make the window: {error}");
            None
        }
    }
}

fn build(app: &AppHandle) -> tauri::Result<WebviewWindow> {
    let window = WebviewWindowBuilder::new(app, LABEL, WebviewUrl::default())
        .title("Solidbar")
        .inner_size(WIDTH, HEIGHT)
        .min_inner_size(WIDTH, MIN_HEIGHT)
        // The window buttons in the header, as in the panel's place.
        .title_bar_style(TitleBarStyle::Overlay)
        .hidden_title(true)
        .traffic_light_position(WINDOW_BUTTONS)
        .transparent(true)
        .effects(
            EffectsBuilder::new()
                .effect(Effect::WindowBackground)
                .state(EffectState::FollowsWindowActiveState)
                .build(),
        )
        .visible(false)
        // Tells the UI it's the window: "Dock to menu bar" in place of the pin, and room for
        // the window buttons.
        .initialization_script("window.__SOLIDBAR_WINDOW__ = true;")
        .build()?;
    // AppKit remembers where it was and its size, across launches too.
    if let Some(ns_window) = ns_window(&window) {
        ns_window.setFrameAutosaveName(ns_string!("Solidbar window"));
    }
    let handle = app.clone();
    // Closing only hides it: it's still the window, from the menu bar icon or the Dock.
    window.on_window_event(move |event| {
        if let WindowEvent::CloseRequested { api, .. } = event {
            api.prevent_close();
            if let Some(window) = handle.get_webview_window(LABEL) {
                let _ = window.hide();
            }
        }
    });
    Ok(window)
}

fn ns_window(window: &WebviewWindow) -> Option<&NSWindow> {
    let pointer = window.ns_window().ok()?;
    // SAFETY: Tauri's NSWindow for `window`, which outlives the borrow; used on the main thread.
    Some(unsafe { &*pointer.cast::<NSWindow>() })
}
