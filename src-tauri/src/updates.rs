//! Looks for new releases on GitHub (tauri-plugin-updater reads `latest.json` from the
//! endpoint in tauri.conf.json, and checks its signature) and installs them.

use std::sync::Mutex;
use std::time::Duration;

use solidbar_shared::ipc::{CommandError, Event, UpdateChanged};
use solidbar_shared::update::{UpdateState, UpdateView};
use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_updater::{Update, UpdaterExt};

/// Give the app time to settle (and the network to come up at login) before the first check.
const FIRST_CHECK_DELAY: Duration = Duration::from_secs(30);
const CHECK_INTERVAL: Duration = Duration::from_secs(6 * 60 * 60);

#[derive(Default)]
pub struct Updates {
    state: Mutex<UpdateState>,
    /// What the last successful check found.
    found: Mutex<Option<Update>>,
}

pub fn view(app: &AppHandle) -> UpdateView {
    UpdateView {
        current_version: app.package_info().version.to_string(),
        state: app.state::<Updates>().state.lock().unwrap().clone(),
    }
}

fn set_state(app: &AppHandle, state: UpdateState) {
    *app.state::<Updates>().state.lock().unwrap() = state;
    if let Err(error) = app.emit(UpdateChanged::NAME, view(app)) {
        log::warn!("could not send the update status: {error}");
    }
}

fn is_busy(app: &AppHandle) -> bool {
    matches!(
        *app.state::<Updates>().state.lock().unwrap(),
        UpdateState::Checking | UpdateState::Installing { .. }
    )
}

/// Checks for an update. A check the person asked for shows its progress and its errors; a
/// background check only reports what it finds, so being offline doesn't show up as a failure.
pub async fn check(app: &AppHandle, requested: bool) -> UpdateView {
    if is_busy(app) {
        return view(app);
    }
    if requested {
        set_state(app, UpdateState::Checking);
    }
    let result = match app.updater() {
        Ok(updater) => updater.check().await,
        Err(error) => Err(error),
    };
    match result {
        Ok(Some(update)) => {
            let version = update.version.clone();
            *app.state::<Updates>().found.lock().unwrap() = Some(update);
            set_state(app, UpdateState::Available { version });
        }
        Ok(None) => {
            *app.state::<Updates>().found.lock().unwrap() = None;
            set_state(app, UpdateState::UpToDate);
        }
        Err(error) => {
            log::warn!("could not check for updates: {error}");
            if requested {
                set_state(
                    app,
                    UpdateState::Failed {
                        message: format!("Couldn't check for updates: {error}"),
                    },
                );
            }
        }
    }
    view(app)
}

/// Downloads and installs the update the last check found, then restarts into it.
pub async fn install(app: &AppHandle) -> Result<(), CommandError> {
    if is_busy(app) {
        return Ok(());
    }
    let update = app.state::<Updates>().found.lock().unwrap().clone();
    let Some(update) = update else {
        return Err(CommandError::new(
            "There's no update to install. Check for updates first.",
        ));
    };
    let version = update.version.clone();
    set_state(
        app,
        UpdateState::Installing {
            version: version.clone(),
            percent: None,
        },
    );

    let mut received = 0_u64;
    let mut shown = None;
    let progress = |chunk: usize, total: Option<u64>| {
        received += chunk as u64;
        let percent = total
            .filter(|total| *total > 0)
            .map(|total| (received * 100 / total).min(100) as u8);
        if percent != shown {
            shown = percent;
            set_state(
                app,
                UpdateState::Installing {
                    version: version.clone(),
                    percent,
                },
            );
        }
    };
    match update.download_and_install(progress, || {}).await {
        Ok(()) => {
            log::info!("installed Solidbar {version}; restarting");
            app.restart()
        }
        Err(error) => {
            log::warn!("could not install the update: {error}");
            let message = format!("Couldn't install the update: {error}");
            set_state(
                app,
                UpdateState::Failed {
                    message: message.clone(),
                },
            );
            Err(CommandError::new(message))
        }
    }
}

/// Checks in the background now and then; release builds only.
pub async fn run_periodic(app: AppHandle) {
    tokio::time::sleep(FIRST_CHECK_DELAY).await;
    loop {
        check(&app, false).await;
        tokio::time::sleep(CHECK_INTERVAL).await;
    }
}
