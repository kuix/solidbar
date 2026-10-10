//! System-wide keyboard shortcuts, configurable in the settings.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

use solidbar_core::engine::Engine;
use solidbar_shared::ipc::ShowReason;
use solidbar_shared::shortcuts::GlobalAction;
use tauri::{AppHandle, Manager};
use tauri_plugin_global_shortcut::{GlobalShortcutExt, Shortcut, ShortcutState};

use crate::{panel, tray};

/// The accelerator currently registered for each action.
#[derive(Default)]
pub struct Registered(Mutex<BTreeMap<GlobalAction, String>>);

pub fn register_all(app: &AppHandle) {
    let settings = app.state::<Arc<Engine>>().settings();
    for action in GlobalAction::ALL {
        if let Some(accelerator) = settings.shortcut(action)
            && let Err(error) = register(app, action, accelerator)
        {
            log::warn!("could not register the shortcut for {action:?}: {error}");
        }
    }
}

/// Replaces the shortcut of `action`; `None` turns it off. Keeps the old one on failure.
pub fn change(
    app: &AppHandle,
    action: GlobalAction,
    accelerator: Option<&str>,
) -> Result<(), String> {
    if let Some(accelerator) = accelerator {
        accelerator
            .parse::<Shortcut>()
            .map_err(|_| "That key combination can't be used as a shortcut".to_owned())?;
        let taken_by_other = registered(app)
            .iter()
            .any(|(other, existing)| *other != action && existing == accelerator);
        if taken_by_other {
            return Err("Another Solidbar shortcut already uses that combination".to_owned());
        }
    }
    let previous = unregister(app, action);
    let Some(accelerator) = accelerator else {
        return Ok(());
    };
    register(app, action, accelerator).inspect_err(|_| {
        if let Some(previous) = previous {
            let _ = register(app, action, &previous);
        }
    })
}

/// Stops all shortcuts while the recorder captures a new combination.
pub fn suspend(app: &AppHandle) {
    if let Err(error) = app.global_shortcut().unregister_all() {
        log::warn!("could not pause the shortcuts: {error}");
    }
}

pub fn resume(app: &AppHandle) {
    let shortcuts = registered(app);
    for (action, accelerator) in shortcuts {
        if !app.global_shortcut().is_registered(accelerator.as_str())
            && let Err(error) = register(app, action, &accelerator)
        {
            log::warn!("could not restore the shortcut for {action:?}: {error}");
        }
    }
}

fn registered(app: &AppHandle) -> BTreeMap<GlobalAction, String> {
    app.state::<Registered>().0.lock().unwrap().clone()
}

fn register(app: &AppHandle, action: GlobalAction, accelerator: &str) -> Result<(), String> {
    app.global_shortcut()
        .on_shortcut(accelerator, move |app, _shortcut, event| {
            if event.state() == ShortcutState::Pressed {
                run(app, action);
            }
        })
        .map_err(|_| "That shortcut is already used by another app".to_owned())?;
    app.state::<Registered>()
        .0
        .lock()
        .unwrap()
        .insert(action, accelerator.to_owned());
    Ok(())
}

fn unregister(app: &AppHandle, action: GlobalAction) -> Option<String> {
    let accelerator = app
        .state::<Registered>()
        .0
        .lock()
        .unwrap()
        .remove(&action)?;
    if let Err(error) = app.global_shortcut().unregister(accelerator.as_str()) {
        log::warn!("could not remove the shortcut {accelerator}: {error}");
    }
    Some(accelerator)
}

fn run(app: &AppHandle, action: GlobalAction) {
    match action {
        GlobalAction::TogglePanel => {
            if panel::is_visible(app) {
                panel::hide(app);
            } else {
                panel::show(app, tray::icon_rect(app), ShowReason::Toggle);
            }
        }
        GlobalAction::NewEntry => {
            panel::show(app, tray::icon_rect(app), ShowReason::NewEntry);
        }
        GlobalAction::ToggleTimer => {
            let engine = app.state::<Arc<Engine>>().inner().clone();
            tauri::async_runtime::spawn(async move {
                // Errors are announced by the engine.
                let _ = engine.toggle_timer().await;
            });
        }
    }
}
