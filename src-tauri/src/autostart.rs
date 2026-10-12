//! Launch at login. On macOS 13 and later Solidbar registers itself with SMAppService, so it
//! appears under Login Items › Open at Login with its name and icon, and switching it off there
//! shows in Settings too. macOS 12 gets a launch agent that names the app's bundle
//! (`AssociatedBundleIdentifiers`); without it, Login Items show the developer and no icon.

use std::env::current_exe;

use auto_launch::{AutoLaunch, AutoLaunchBuilder, MacOSLaunchMode};
use tauri::{AppHandle, Manager};

pub struct Autostart {
    launcher: AutoLaunch,
    /// The launch agent Solidbar 0.1 installed, replaced by SMAppService where available.
    old_agent: Option<AutoLaunch>,
}

impl Autostart {
    pub fn new(app: &AppHandle) -> Result<Self, String> {
        let name = app.package_info().name.clone();
        let bundle_id = app.config().identifier.clone();
        let exe = current_exe()
            .and_then(|path| path.canonicalize())
            .map_err(|error| error.to_string())?
            .display()
            .to_string();
        let launch_agent = || {
            AutoLaunchBuilder::new()
                .set_app_name(&name)
                .set_app_path(&exe)
                .set_macos_launch_mode(MacOSLaunchMode::LaunchAgent)
                .set_bundle_identifiers(&[&bundle_id])
                .build()
                .map_err(|error| error.to_string())
        };
        // Fails before macOS 13.
        let app_service = AutoLaunchBuilder::new()
            .set_macos_launch_mode(MacOSLaunchMode::SMAppService)
            .build();
        Ok(match app_service {
            Ok(launcher) => Self {
                launcher,
                old_agent: Some(launch_agent()?),
            },
            Err(_) => Self {
                launcher: launch_agent()?,
                old_agent: None,
            },
        })
    }

    pub fn is_enabled(&self) -> bool {
        self.launcher.is_enabled().unwrap_or(false)
    }

    pub fn set_enabled(&self, enabled: bool) -> Result<(), String> {
        let result = if enabled {
            self.launcher.enable()
        } else {
            self.launcher.disable()
        };
        result.map_err(|error| error.to_string())
    }

    /// Brings a login item from Solidbar 0.1 up to date, keeping the person's choice: its launch
    /// agent becomes an SMAppService registration, or (on macOS 12) is rewritten so it names the
    /// bundle and the app's current location.
    fn migrate(&self) {
        match &self.old_agent {
            Some(old_agent) if old_agent.is_enabled().unwrap_or(false) => {
                match self.launcher.enable() {
                    Ok(()) => {
                        if let Err(error) = old_agent.disable() {
                            log::warn!("couldn't remove the old launch agent: {error}");
                        }
                    }
                    Err(error) => log::warn!("couldn't register as a login item: {error}"),
                }
            }
            None if self.is_enabled() => {
                if let Err(error) = self.launcher.enable() {
                    log::warn!("couldn't update the launch agent: {error}");
                }
            }
            _ => {}
        }
    }
}

pub fn setup(app: &AppHandle) {
    match Autostart::new(app) {
        Ok(autostart) => {
            autostart.migrate();
            app.manage(autostart);
        }
        Err(error) => log::warn!("launch at login is unavailable: {error}"),
    }
}
