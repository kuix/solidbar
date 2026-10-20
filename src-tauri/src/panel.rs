//! The popover-like panel anchored under the menu bar icon.
//!
//! Dragged away from the icon by its header, the panel detaches as Harvest's does: it stays
//! open when something else takes the focus, until it's closed, and opens under the icon again.
//! Detached, it has the window buttons in its header, and the close button closes it.
//!
//! All tauri-nspanel usage lives in this module so it can be swapped out if needed.

// `panel_event!` requires explicit `-> ()` return types.
#![allow(clippy::unused_unit)]

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use objc2_app_kit::{
    NSApplicationActivationOptions, NSRunningApplication, NSWindowButton, NSWindowStyleMask,
    NSWindowTitleVisibility, NSWorkspace,
};
use solidbar_core::engine::Engine;
use solidbar_shared::ipc::{Event, PanelDetached, PanelHidden, PanelShown, ShowReason};
use tauri::{AppHandle, Emitter, Manager};
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
/// How far the panel can be from where it was put under the icon, in points, and still count
/// as being there: frames can come back off by a rounding.
const ANCHOR_SLACK: f64 = 1.0;
/// What the panel's style gains while it's detached: a title bar, see-through and without its
/// title, so the window buttons sit in the header. Only Close works: a panel of a fixed size
/// that opens from the menu bar has nothing to minimize or zoom to.
const DETACHED_STYLE: NSWindowStyleMask = NSWindowStyleMask::Titled
    .union(NSWindowStyleMask::Closable)
    .union(NSWindowStyleMask::FullSizeContentView);
/// Where the window buttons go, in points from the panel's top left corner: the close
/// button's left edge, and the middle of the header's row of buttons (`.header` in app.css).
const WINDOW_BUTTONS_LEFT: f64 = 13.0;
const WINDOW_BUTTONS_MIDDLE: f64 = 21.0;

tauri_panel! {
    panel!(SolidbarPanel {
        config: {
            can_become_key_window: true,
            can_become_main_window: false,
            is_floating_panel: true
        }
    })

    panel_event!(SolidbarPanelEvents {
        window_did_resign_key(notification: &NSNotification) -> (),
        window_did_become_key(notification: &NSNotification) -> (),
        window_did_move(notification: &NSNotification) -> (),
        window_should_close(window: &NSWindow) -> Bool
    })
}

// `tauri_panel!` above brings `Bool`, `NSPanel`, `NSPoint`, `NSRect` and `NSSize` into scope.

/// Where the panel opens, in AppKit screen coordinates: points, from the bottom left of the
/// main screen, y growing upwards. Points rather than pixels, because screens side by side can
/// have different scale factors.
#[derive(Clone, Copy, Debug)]
pub struct Anchor {
    /// The menu bar icon.
    pub icon: NSRect,
    /// The screen whose menu bar holds the icon.
    pub screen: NSRect,
}

#[derive(Default)]
pub struct PanelState {
    pinned: AtomicBool,
    /// Dragged away from the icon since it opened.
    detached: AtomicBool,
    /// The top left corner the panel was last put at under the icon.
    anchored_at: Mutex<Option<NSPoint>>,
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
        if !state.pinned.load(Ordering::Relaxed) && !state.detached.load(Ordering::Relaxed) {
            hide(&handle);
        }
    });
    let handle = app.clone();
    // Putting the panel under the icon moves it too, so only a move away from there counts.
    handler.window_did_move(move |_notification| {
        let Ok(panel) = handle.get_webview_panel(LABEL) else {
            return;
        };
        let state = handle.state::<PanelState>();
        let anchored_at = *state.anchored_at.lock().unwrap();
        if left_its_anchor(anchored_at, panel.as_panel().frame())
            && !state.detached.swap(true, Ordering::Relaxed)
        {
            // Once the drag is over: a new style mask rebuilds the window's frame view.
            later(&handle, |app| set_window_buttons(app, true));
        }
    });
    let handle = app.clone();
    // AppKit puts the window buttons back where a title bar has them now and then.
    handler.window_did_become_key(move |_notification| {
        if handle
            .state::<PanelState>()
            .detached
            .load(Ordering::Relaxed)
            && let Ok(panel) = handle.get_webview_panel(LABEL)
        {
            place_window_buttons(panel.as_panel());
        }
    });
    let handle = app.clone();
    // The close button closes the panel as Escape does, so it opens under the icon again.
    handler.window_should_close(move |_window| {
        // Once the click is over: closing takes the button away.
        later(&handle, hide);
        Bool::NO
    });
    panel.set_event_handler(Some(handler.as_ref()));

    Ok(())
}

pub fn is_visible(app: &AppHandle) -> bool {
    app.get_webview_panel(LABEL)
        .map(|panel| panel.is_visible())
        .unwrap_or(false)
}

/// Toggles the panel from a click on the menu bar icon.
pub fn toggle_from_tray(app: &AppHandle, anchor: Option<Anchor>) {
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
        show(app, anchor, ShowReason::Toggle);
    }
}

/// Shows the panel under the menu bar icon (when it's known where that is, and the panel isn't
/// detached) and makes it key.
pub fn show(app: &AppHandle, anchor: Option<Anchor>, reason: ShowReason) {
    let Ok(panel) = app.get_webview_panel(LABEL) else {
        return;
    };
    remember_frontmost_app(app);
    let state = app.state::<PanelState>();
    if let Some(anchor) = anchor.filter(|_| !state.detached.load(Ordering::Relaxed)) {
        position_under(panel.as_panel(), anchor, &state);
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
    // Detached until closed: it opens under the icon again.
    if state.detached.swap(false, Ordering::Relaxed) {
        set_window_buttons(app, false);
    }
    *state.hidden_at.lock().unwrap() = Some(Instant::now());
    restore_previous_app(&state);
    let _ = app.emit(PanelHidden::NAME, ());
}

pub fn set_pinned(app: &AppHandle, pinned: bool) {
    app.state::<PanelState>()
        .pinned
        .store(pinned, Ordering::Relaxed);
}

/// Runs `task` on the main thread after the event being handled there. (`run_on_main_thread`
/// alone would run it right away, from the main thread.)
fn later(app: &AppHandle, task: fn(&AppHandle)) {
    let handle = app.clone();
    tauri::async_runtime::spawn(async move {
        let app = handle.clone();
        let _ = handle.run_on_main_thread(move || task(&app));
    });
}

/// Gives the detached panel its window buttons, or takes them away for the popover under the
/// icon, and tells the UI, which makes room for them in the header.
fn set_window_buttons(app: &AppHandle, shown: bool) {
    let Ok(panel) = app.get_webview_panel(LABEL) else {
        return;
    };
    let window = panel.as_panel();
    let style = if shown {
        window.styleMask().union(DETACHED_STYLE)
    } else {
        window.styleMask().difference(DETACHED_STYLE)
    };
    if let Err(error) = panel.set_style_mask(style) {
        log::warn!("couldn't change the panel's window buttons: {error}");
        return;
    }
    if shown {
        window.setTitleVisibility(NSWindowTitleVisibility::Hidden);
        window.setTitlebarAppearsTransparent(true);
        place_window_buttons(window);
    }
    let _ = app.emit(PanelDetached::NAME, shown);
}

/// Lines the window buttons up with the header's buttons, at its left end.
fn place_window_buttons(window: &NSPanel) {
    let [Some(close), Some(minimize), Some(zoom)] = [
        NSWindowButton::CloseButton,
        NSWindowButton::MiniaturizeButton,
        NSWindowButton::ZoomButton,
    ]
    .map(|kind| window.standardWindowButton(kind)) else {
        return;
    };
    // The buttons are in the title bar view, in the title bar's container.
    // SAFETY: plain reads of the view hierarchy, on the main thread.
    let Some(title_bar) = (unsafe { close.superview() }) else {
        return;
    };
    let Some(container) = (unsafe { title_bar.superview() }) else {
        return;
    };
    // The title bar as tall as the header's row of buttons is from the top, twice, so the
    // buttons centered in it are in line with them. y grows upwards.
    let height = 2.0 * WINDOW_BUTTONS_MIDDLE;
    let mut frame = container.frame();
    frame.size.height = height;
    frame.origin.y = window.frame().size.height - height;
    container.setFrame(frame);
    title_bar.setFrame(NSRect::new(NSPoint::new(0.0, 0.0), frame.size));
    let size = close.frame().size;
    let spacing = minimize.frame().origin.x - close.frame().origin.x;
    for (index, button) in [close, minimize, zoom].iter().enumerate() {
        button.setFrameOrigin(NSPoint::new(
            WINDOW_BUTTONS_LEFT + index as f64 * spacing,
            ((height - size.height) / 2.0).round(),
        ));
    }
}

fn position_under(panel: &NSPanel, anchor: Anchor, state: &PanelState) {
    let top_left = top_left_under(anchor, panel.frame().size.width);
    // Before the move, which tells `window_did_move` about it.
    *state.anchored_at.lock().unwrap() = Some(top_left);
    panel.setFrameTopLeftPoint(top_left);
}

/// Whether a panel at `frame` was moved away from `anchored_at`, where it was put under the
/// icon. One that was never put there hasn't been.
fn left_its_anchor(anchored_at: Option<NSPoint>, frame: NSRect) -> bool {
    let Some(anchored_at) = anchored_at else {
        return false;
    };
    // y grows upwards, so the top edge is the origin's y plus the height.
    let top = frame.origin.y + frame.size.height;
    (frame.origin.x - anchored_at.x).abs() > ANCHOR_SLACK
        || (top - anchored_at.y).abs() > ANCHOR_SLACK
}

/// The top left corner of a panel `width` points wide: centered under the icon, and within
/// the screen holding the icon.
fn top_left_under(anchor: Anchor, width: f64) -> NSPoint {
    let Anchor { icon, screen } = anchor;
    let left = screen.origin.x + SCREEN_MARGIN;
    let right = screen.origin.x + screen.size.width - width - SCREEN_MARGIN;
    let x = (icon.origin.x + icon.size.width / 2.0 - width / 2.0)
        .min(right)
        .max(left);
    // y grows upwards: the panel's top edge goes just below the icon's bottom edge.
    let top = icon.origin.y - ANCHOR_GAP;
    NSPoint::new(x.round(), top.round())
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

#[cfg(test)]
mod tests {
    use super::*;

    fn rect(x: f64, y: f64, width: f64, height: f64) -> NSRect {
        NSRect::new(NSPoint::new(x, y), NSSize::new(width, height))
    }

    /// A 1512×982 point built-in screen; its menu bar is 24 points tall.
    const MAIN: (f64, f64, f64, f64) = (0.0, 0.0, 1512.0, 982.0);

    fn anchor(icon_x: f64, screen: (f64, f64, f64, f64)) -> Anchor {
        let (x, y, width, height) = screen;
        Anchor {
            icon: rect(icon_x, y + height - 24.0, 30.0, 24.0),
            screen: rect(x, y, width, height),
        }
    }

    #[test]
    fn opens_centered_under_the_icon() {
        let top_left = top_left_under(anchor(1000.0, MAIN), 380.0);
        assert_eq!((top_left.x, top_left.y), (825.0, 954.0));
    }

    #[test]
    fn stays_on_the_screen_near_its_edges() {
        let near_right = top_left_under(anchor(1470.0, MAIN), 380.0);
        assert_eq!(near_right.x, 1512.0 - 380.0 - 8.0);
        let near_left = top_left_under(anchor(10.0, MAIN), 380.0);
        assert_eq!(near_left.x, 8.0);
    }

    #[test]
    fn opens_on_the_screen_whose_menu_bar_was_clicked() {
        // A 2560×1440 screen to the left of the main one, top-aligned, so its origin is negative.
        let left_screen = (-2560.0, 982.0 - 1440.0, 2560.0, 1440.0);
        let top_left = top_left_under(anchor(-1300.0, left_screen), 380.0);
        assert_eq!((top_left.x, top_left.y), (-1475.0, 954.0));
        let near_its_right_edge = top_left_under(anchor(-40.0, left_screen), 380.0);
        assert_eq!(near_its_right_edge.x, -380.0 - 8.0);

        // A screen above the main one.
        let above = (200.0, 982.0, 1920.0, 1080.0);
        let top_left = top_left_under(anchor(1000.0, above), 380.0);
        assert_eq!(
            (top_left.x, top_left.y),
            (825.0, 982.0 + 1080.0 - 24.0 - 4.0)
        );
    }

    #[test]
    fn detaches_only_when_dragged_away_from_the_icon() {
        let anchored_at = Some(NSPoint::new(825.0, 954.0));
        // A 380×520 panel whose top left corner is at (825, 954).
        let under_the_icon = rect(825.0, 954.0 - 520.0, 380.0, 520.0);
        assert!(!left_its_anchor(anchored_at, under_the_icon));
        let off_by_a_rounding = rect(825.5, 954.0 - 520.0 - 0.5, 380.0, 520.0);
        assert!(!left_its_anchor(anchored_at, off_by_a_rounding));
        let dragged_down = rect(825.0, 954.0 - 520.0 - 40.0, 380.0, 520.0);
        assert!(left_its_anchor(anchored_at, dragged_down));
        let dragged_sideways = rect(600.0, 954.0 - 520.0, 380.0, 520.0);
        assert!(left_its_anchor(anchored_at, dragged_sideways));
        assert!(!left_its_anchor(None, dragged_down));
    }
}
