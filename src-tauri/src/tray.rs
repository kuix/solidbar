//! The menu bar icon: template icons, the time shown next to it, and its click handling.

use objc2::MainThreadMarker;
use objc2_app_kit::{NSFont, NSFontWeightRegular};
use solidbar_shared::view::TrayView;
use tauri::AppHandle;
use tauri::image::Image;
use tauri::menu::{Menu, MenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIcon, TrayIconBuilder, TrayIconEvent};

use crate::panel::{self, Anchor};
use crate::window;

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
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                let app = tray.app_handle();
                if window::is_windowed(app) {
                    window::toggle(app);
                } else {
                    panel::toggle_from_tray(app, anchor_of(tray));
                }
            }
        })
        .build(app)?;

    use_monospaced_digits(&tray)?;
    Ok(())
}

/// Where the menu bar icon is: on the screen whose menu bar was clicked last, with several.
///
/// The click event's `rect` is in pixels, scaled for the icon's screen, which can't be told
/// apart from another screen's pixels; AppKit's own frames are in points.
pub fn anchor(app: &AppHandle) -> Option<Anchor> {
    anchor_of(&app.tray_by_id(TRAY_ID)?)
}

fn anchor_of(tray: &TrayIcon) -> Option<Anchor> {
    tray.with_inner_tray_icon(|inner| {
        let mtm = MainThreadMarker::new()?;
        let window = inner.ns_status_item()?.button(mtm)?.window()?;
        Some(Anchor {
            icon: window.frame(),
            screen: window.screen()?.frame(),
        })
    })
    .ok()
    .flatten()
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
