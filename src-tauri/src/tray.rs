//! The menu bar icon: template icons, the time shown next to it, and its click handling.

use objc2::MainThreadMarker;
use objc2_app_kit::{NSFont, NSFontWeightRegular};
use solidbar_shared::view::TrayView;
use tauri::image::Image;
use tauri::menu::{Menu, MenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIcon, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Rect};

use crate::panel;

const TRAY_ID: &str = "solidbar";
const QUIT_ID: &str = "quit";

const ICON_IDLE: &[u8] = include_bytes!("../icons/tray-idle.png");
const ICON_RUNNING: &[u8] = include_bytes!("../icons/tray-running.png");

pub fn init(app: &AppHandle) -> tauri::Result<()> {
    let quit = MenuItem::with_id(app, QUIT_ID, "Quit Solidbar", true, Some("CmdOrCtrl+Q"))?;
    let menu = Menu::with_items(app, &[&quit])?;

    let tray = TrayIconBuilder::with_id(TRAY_ID)
        .icon(Image::from_bytes(ICON_IDLE)?)
        .icon_as_template(true)
        .tooltip("Solidbar")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| {
            if event.id() == QUIT_ID {
                app.exit(0);
            }
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                rect,
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                panel::toggle_from_tray(tray.app_handle(), rect);
            }
        })
        .build(app)?;

    use_monospaced_digits(&tray)?;
    Ok(())
}

/// Bounds of the menu bar icon, used to anchor the panel when it opens from a shortcut.
pub fn icon_rect(app: &AppHandle) -> Option<Rect> {
    app.tray_by_id(TRAY_ID)?.rect().ok().flatten()
}

pub fn set_status(app: &AppHandle, status: &TrayView) -> tauri::Result<()> {
    let Some(tray) = app.tray_by_id(TRAY_ID) else {
        return Ok(());
    };
    let icon = if status.timer_running {
        ICON_RUNNING
    } else {
        ICON_IDLE
    };
    tray.set_icon_with_as_template(Some(Image::from_bytes(icon)?), true)?;
    // A leading space keeps the text from touching the icon.
    tray.set_title(status.title.as_ref().map(|title| format!(" {title}")))?;
    tray.set_tooltip(Some(&status.tooltip))?;
    Ok(())
}

/// Keeps the menu bar text from jittering as the digits change.
fn use_monospaced_digits(tray: &TrayIcon) -> tauri::Result<()> {
    tray.with_inner_tray_icon(|inner| {
        let Some(item) = inner.ns_status_item() else {
            return;
        };
        let mtm = MainThreadMarker::new().expect("tray icon callbacks run on the main thread");
        if let Some(button) = item.button(mtm) {
            // SAFETY: `NSFontWeightRegular` is an immutable AppKit constant.
            let weight = unsafe { NSFontWeightRegular };
            let font =
                NSFont::monospacedDigitSystemFontOfSize_weight(NSFont::systemFontSize(), weight);
            button.setFont(Some(&font));
        }
    })
}
