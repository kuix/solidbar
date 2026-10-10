//! The popover-like panel anchored under the menu bar icon.
//!
//! All tauri-nspanel usage lives in this module so it can be swapped out if needed.

// `panel_event!` requires explicit `-> ()` return types.
#![allow(clippy::unused_unit)]

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use objc2_app_kit::{NSApplicationActivationOptions, NSRunningApplication, NSWorkspace};
use solidbar_core::engine::Engine;
use solidbar_shared::ipc::{Event, PanelHidden, PanelShown, ShowReason};
use tauri::{AppHandle, Emitter, Manager, PhysicalPosition, Rect};
use tauri_nspanel::{
    CollectionBehavior, ManagerExt, PanelLevel, StyleMask, WebviewWindowExt, tauri_panel,
};

pub const LABEL: &str = "main";

/// Gap between the bottom of the menu bar icon and the top of the panel, in points.
const ANCHOR_GAP: f64 = 4.0;
/// Margin kept between the panel and the screen edge, in points.
const SCREEN_MARGIN: f64 = 8.0;
/// A tray click this soon after the panel hid itself (on resign key) must not reopen it:
/// the click on the icon is what took the key status away.
const REOPEN_GUARD: Duration = Duration::from_millis(250);

tauri_panel! {
    panel!(SolidbarPanel {
        config: {
            can_become_key_window: true,
            can_become_main_window: false,
            is_floating_panel: true
        }
    })

    panel_event!(SolidbarPanelEvents {
        window_did_resign_key(notification: &NSNotification) -> ()
    })
}

#[derive(Default)]
pub struct PanelState {
    pinned: AtomicBool,
    hidden_at: Mutex<Option<Instant>>,
    /// The app that was frontmost when the panel opened, so focus can go back to it.
    previous_app_pid: Mutex<Option<i32>>,
}

pub fn init(app: &AppHandle) -> tauri::Result<()> {
    let window = app
        .get_webview_window(LABEL)
        .expect("the main window is declared in tauri.conf.json");
    let panel = window
        .to_panel::<SolidbarPanel>()
        .map_err(|error| tauri::Error::Anyhow(error.into()))?;

    panel.set_level(PanelLevel::Floating.value());
    panel
        .add_style_mask(StyleMask::empty().nonactivating_panel().into())
        .map_err(|error| tauri::Error::Anyhow(error.into()))?;
    panel.set_collection_behavior(
        CollectionBehavior::new()
            .can_join_all_spaces()
            .full_screen_auxiliary()
            .into(),
    );
    panel.set_has_shadow(true);

    let handler = SolidbarPanelEvents::new();
    let handle = app.clone();
    handler.window_did_resign_key(move |_notification| {
        let state = handle.state::<PanelState>();
        if !state.pinned.load(Ordering::Relaxed) {
            hide(&handle);
        }
    });
    panel.set_event_handler(Some(handler.as_ref()));

    Ok(())
}

pub fn is_visible(app: &AppHandle) -> bool {
    app.get_webview_panel(LABEL)
        .map(|panel| panel.is_visible())
        .unwrap_or(false)
}

/// Toggles the panel from a click on the menu bar icon whose bounds are `icon`.
pub fn toggle_from_tray(app: &AppHandle, icon: Rect) {
    if is_visible(app) {
        hide(app);
        return;
    }
    let state = app.state::<PanelState>();
    let just_hidden = state
        .hidden_at
        .lock()
        .unwrap()
        .is_some_and(|at| at.elapsed() < REOPEN_GUARD);
    if !just_hidden {
        show(app, Some(icon), ShowReason::Toggle);
    }
}

/// Shows the panel under the menu bar icon (when its bounds are known) and makes it key.
pub fn show(app: &AppHandle, icon: Option<Rect>, reason: ShowReason) {
    let Ok(panel) = app.get_webview_panel(LABEL) else {
        return;
    };
    remember_frontmost_app(app);
    if let Some(icon) = icon {
        position_under(app, icon);
    }
    panel.show_and_make_key();
    let _ = app.emit(PanelShown::NAME, reason);
    // Back to today, with fresh data if it's getting old.
    let engine = app.state::<Arc<Engine>>().inner().clone();
    tauri::async_runtime::spawn(async move { engine.panel_opened().await });
}

pub fn hide(app: &AppHandle) {
    let Ok(panel) = app.get_webview_panel(LABEL) else {
        return;
    };
    if !panel.is_visible() {
        return;
    }
    panel.hide();
    let state = app.state::<PanelState>();
    *state.hidden_at.lock().unwrap() = Some(Instant::now());
    restore_previous_app(&state);
    let _ = app.emit(PanelHidden::NAME, ());
}

pub fn set_pinned(app: &AppHandle, pinned: bool) {
    app.state::<PanelState>()
        .pinned
        .store(pinned, Ordering::Relaxed);
}

/// Centers the panel horizontally under the icon, clamped to the screen holding the icon.
fn position_under(app: &AppHandle, icon: Rect) {
    let Some(window) = app.get_webview_window(LABEL) else {
        return;
    };
    // tray-icon reports the icon bounds already in physical pixels (scaled by the backing
    // factor of the menu bar it sits in), so the scale passed here is never used.
    let icon_position = icon.position.to_physical::<f64>(1.0);
    let icon_size = icon.size.to_physical::<f64>(1.0);
    let icon_center_x = icon_position.x + icon_size.width / 2.0;

    let monitors = app.available_monitors().unwrap_or_default();
    let monitor = monitors.into_iter().find(|monitor| {
        let origin = monitor.position();
        let size = monitor.size();
        let left = f64::from(origin.x);
        let right = left + f64::from(size.width);
        let top = f64::from(origin.y);
        let bottom = top + f64::from(size.height);
        (left..right).contains(&icon_center_x) && (top..bottom).contains(&icon_position.y)
    });
    let Some(monitor) = monitor else {
        return;
    };
    let scale = monitor.scale_factor();
    let Ok(panel_size) = window.outer_size() else {
        return;
    };

    let screen_left = f64::from(monitor.position().x);
    let screen_right = screen_left + f64::from(monitor.size().width);
    let margin = SCREEN_MARGIN * scale;
    let width = f64::from(panel_size.width);
    let x = (icon_center_x - width / 2.0)
        .min(screen_right - width - margin)
        .max(screen_left + margin);
    let y = icon_position.y + icon_size.height + ANCHOR_GAP * scale;

    let _ = window.set_position(PhysicalPosition::new(x.round() as i32, y.round() as i32));
}

fn remember_frontmost_app(app: &AppHandle) {
    let own_pid = std::process::id() as i32;
    let frontmost = NSWorkspace::sharedWorkspace()
        .frontmostApplication()
        .map(|running| running.processIdentifier())
        .filter(|pid| *pid != own_pid);
    if frontmost.is_some() {
        *app.state::<PanelState>().previous_app_pid.lock().unwrap() = frontmost;
    }
}

/// The panel is non-activating, so normally the previous app never loses active status.
/// If something activated Solidbar anyway, hand activation back.
fn restore_previous_app(state: &PanelState) {
    if !NSRunningApplication::currentApplication().isActive() {
        return;
    }
    let Some(pid) = *state.previous_app_pid.lock().unwrap() else {
        return;
    };
    if let Some(previous) = NSRunningApplication::runningApplicationWithProcessIdentifier(pid) {
        previous.activateWithOptions(NSApplicationActivationOptions::empty());
    }
}
