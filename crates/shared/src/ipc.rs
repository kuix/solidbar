//! The contract between the UI and the backend: command names with their argument and result
//! types, and event names with their payloads. Both sides use these definitions, so a renamed
//! command or changed payload fails to compile instead of failing at runtime.
//!
//! Every command takes a single argument named `args`.

use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::instance::InstanceSettings;
use crate::settings::SettingsPatch;
use crate::shortcuts::GlobalAction;
use crate::update::UpdateView;
use crate::view::{AppView, Catalog, EntryKind, FormInit, Notice};

pub trait Command {
    const NAME: &'static str;
    type Args: Serialize + DeserializeOwned;
    type Output: Serialize + DeserializeOwned;
}

pub trait Event {
    const NAME: &'static str;
    type Payload: Serialize + DeserializeOwned;
}

/// The error every command returns, already worded for the person using the app.
#[derive(Clone, Debug, PartialEq, Eq, Error, Serialize, Deserialize)]
#[error("{message}")]
pub struct CommandError {
    pub message: String,
}

impl CommandError {
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

macro_rules! commands {
    ($($(#[$doc:meta])* $command:ident = $name:literal, $args:ty => $output:ty;)*) => {
        $(
            $(#[$doc])*
            pub struct $command;
            impl Command for $command {
                const NAME: &'static str = $name;
                type Args = $args;
                type Output = $output;
            }
        )*
        /// Every command name, for checking the backend registers them all.
        pub const COMMAND_NAMES: &[&str] = &[$($name),*];
    };
}

macro_rules! events {
    ($($(#[$doc:meta])* $event:ident = $name:literal, $payload:ty;)*) => {
        $(
            $(#[$doc])*
            pub struct $event;
            impl Event for $event {
                const NAME: &'static str = $name;
                type Payload = $payload;
            }
        )*
    };
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct BootstrapView {
    pub app: AppView,
    pub catalog: Catalog,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct MembershipArgs {
    pub membership_id: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DateArgs {
    /// `YYYY-MM-DD`
    pub date: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RefreshArgs {
    /// Also reload projects and tasks, ignoring the cache.
    pub force: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct FormArgs {
    pub date: String,
    pub entry_id: Option<String>,
    /// Edit these entries together (a row of continued entries).
    #[serde(default)]
    pub group_ids: Vec<String>,
}

/// The entry form as submitted.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct EntryDraft {
    /// `None` creates a new entry.
    pub id: Option<String>,
    /// Saves project, task, notes, billable and kind to all of these, keeping their times.
    #[serde(default)]
    pub group_ids: Vec<String>,
    #[serde(default)]
    pub kind: EntryKind,
    pub date: String,
    pub project_id: Option<String>,
    pub task_id: Option<String>,
    pub description: String,
    pub billable: bool,
    /// As typed, e.g. `1:30`; validated by the backend too.
    pub duration: String,
    /// `HH:MM`
    pub start_time: String,
    /// Save as a running timer (⌘Enter).
    pub start_timer: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct EntriesArgs {
    pub ids: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AdjustArgs {
    pub id: String,
    /// ±60 for the −/+ buttons.
    pub delta_minutes: i64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum TimerSource {
    /// Continue an existing entry: a new timer with its project, task and description.
    Entry { id: String },
    /// The most recently used project/task pair (⌥⌘S).
    LastUsed,
    /// Back to work after a break: continues the work entry before it.
    Resume,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ShortcutArgs {
    pub action: GlobalAction,
    /// `None` turns the shortcut off.
    pub accelerator: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PinArgs {
    pub pinned: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AutostartArgs {
    pub enabled: bool,
}

/// Why the panel appeared; decides where focus goes.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ShowReason {
    Toggle,
    NewEntry,
}

commands! {
    Bootstrap = "bootstrap", () => BootstrapView;
    AuthLogin = "auth_login", InstanceSettings => ();
    AuthCancel = "auth_cancel", () => ();
    AuthLogout = "auth_logout", () => ();
    SelectOrg = "select_org", MembershipArgs => ();
    SelectDate = "select_date", DateArgs => ();
    Refresh = "refresh", RefreshArgs => ();
    PrepareForm = "prepare_form", FormArgs => FormInit;
    SaveEntry = "save_entry", EntryDraft => ();
    DeleteEntries = "delete_entries", EntriesArgs => ();
    AdjustEntry = "adjust_entry", AdjustArgs => ();
    StartTimer = "start_timer", TimerSource => ();
    StopTimer = "stop_timer", () => ();
    /// Stops the work timer and starts a break at the same moment.
    StartBreak = "start_break", () => ();
    UpdateSettings = "update_settings", SettingsPatch => ();
    SetShortcut = "set_shortcut", ShortcutArgs => ();
    SuspendShortcuts = "shortcuts_suspend", () => ();
    ResumeShortcuts = "shortcuts_resume", () => ();
    SetAutostart = "set_autostart", AutostartArgs => ();
    GetAutostart = "get_autostart", () => bool;
    PanelHide = "panel_hide", () => ();
    PanelSetPinned = "panel_set_pinned", PinArgs => ();
    OpenInBrowser = "open_in_browser", () => ();
    Quit = "quit", () => ();
    GetUpdate = "update_get", () => UpdateView;
    /// Checks now and reports the result, including "up to date" and errors.
    CheckForUpdate = "update_check", () => UpdateView;
    /// Downloads and installs the update found by the last check, then restarts the app.
    InstallUpdate = "update_install", () => ();
}

events! {
    AppChanged = "view://app", AppView;
    CatalogChanged = "view://catalog", Catalog;
    NoticeShown = "view://notice", Notice;
    PanelShown = "panel://shown", ShowReason;
    PanelHidden = "panel://hidden", ();
    UpdateChanged = "update://changed", UpdateView;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn command_names_are_unique() {
        let mut names = COMMAND_NAMES.to_vec();
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), COMMAND_NAMES.len());
    }

    #[test]
    fn timer_sources_are_tagged() {
        let json = serde_json::to_value(TimerSource::Entry { id: "e1".into() }).unwrap();
        assert_eq!(json, serde_json::json!({ "kind": "entry", "id": "e1" }));
    }
}
