//! What the UI renders. The backend formats times and totals, so the UI never deals with
//! time zones.

use serde::{Deserialize, Serialize};

use crate::instance::{InstanceSettings, Redirect};
use crate::settings::Settings;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct AppView {
    pub auth: AuthView,
    pub settings: Settings,
    pub orgs: Vec<OrgView>,
    pub active_membership_id: Option<String>,
    /// Signed in to several organizations and none chosen yet: the panel asks first.
    pub choose_org: bool,
    /// Show and read times as `2:30 PM` rather than `14:30` (the setting, or the Mac's).
    pub hour12: bool,
    /// `YYYY-MM-DD` in the user's solidtime time zone.
    pub today: String,
    pub selected_date: String,
    pub week: Option<WeekView>,
    pub timer: Option<TimerView>,
    pub online: bool,
    pub loading: bool,
    /// The person's solidtime time zone differs from the Mac's.
    pub time_zone_hint: Option<String>,
    pub tray: TrayView,
    /// The active organization lets its members track breaks.
    #[serde(default)]
    pub breaks_enabled: bool,
    /// The MCP server for AI agents.
    #[serde(default)]
    pub agents: AgentServerView,
}

/// The MCP server for AI agents, as Settings shows it.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum AgentServerView {
    #[default]
    Off,
    /// Answering at `url`. `bridge` is the binary that relays to it for agents that start
    /// their servers themselves (with the argument [`crate::agents::BRIDGE_ARG`]).
    Running {
        url: String,
        bridge: String,
    },
    Failed {
        message: String,
    },
}

/// What the menu bar shows next to the icon.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct TrayView {
    pub timer_running: bool,
    /// `0:42` for a running timer, otherwise today's total; `None` shows only the icon.
    pub title: Option<String>,
    /// Also what VoiceOver reads for the menu bar item.
    pub tooltip: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum AuthView {
    SignedOut {
        instance: InstanceSettings,
        error: Option<String>,
    },
    SigningIn {
        instance: InstanceSettings,
        /// How the browser will return; `None` while Solidbar is still finding out.
        #[serde(default)]
        redirect: Option<Redirect>,
    },
    SignedIn {
        user_name: String,
        instance_url: String,
    },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct OrgView {
    pub membership_id: String,
    pub name: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct WeekView {
    /// `YYYY-MM-DD` of the first day.
    pub first_day: String,
    /// e.g. `Oct 5 – 11, 2026`.
    pub label: String,
    pub days: Vec<DayView>,
    pub total_seconds: i64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct DayView {
    pub date: String,
    /// `Mon`
    pub weekday_short: String,
    /// `Monday, Oct 5`
    pub label: String,
    pub is_today: bool,
    /// Working time; breaks don't count.
    pub total_seconds: i64,
    #[serde(default)]
    pub break_seconds: i64,
    /// Ordered by start.
    pub entries: Vec<EntryView>,
}

/// Work time or a break (solidtime's time entry `type`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EntryKind {
    #[default]
    Work,
    Break,
}

impl EntryKind {
    pub fn is_work(&self) -> bool {
        *self == Self::Work
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct EntryView {
    pub id: String,
    #[serde(default)]
    pub kind: EntryKind,
    pub project_id: Option<String>,
    pub task_id: Option<String>,
    pub project_name: Option<String>,
    pub task_name: Option<String>,
    pub client_name: Option<String>,
    /// CSS color of the project.
    pub color: Option<String>,
    pub description: String,
    pub billable: bool,
    /// Local wall-clock times, formatted for the Mac's 12/24-hour preference.
    pub start_label: String,
    pub end_label: Option<String>,
    pub duration_seconds: i64,
    pub running: bool,
    /// Saved locally, waiting for solidtime to confirm.
    pub pending: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TimerView {
    pub entry_id: String,
    pub membership_id: Option<String>,
    pub org_name: Option<String>,
    /// RFC 3339; the UI counts up from it.
    pub started_at: String,
    pub description: String,
    pub project_name: Option<String>,
    pub in_active_org: bool,
    #[serde(default)]
    pub kind: EntryKind,
    /// During a break: what "Resume" continues, e.g. the description of the work before it.
    #[serde(default)]
    pub resume_label: Option<String>,
}

/// One choice in the project/task picker.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct WorkOption {
    /// `Break` for the "Break" choice, offered when the organization tracks breaks.
    #[serde(default)]
    pub kind: EntryKind,
    pub project_id: Option<String>,
    pub task_id: Option<String>,
    /// `Project › Task`, or `Project`, or `No project`.
    pub label: String,
    pub client_name: Option<String>,
    pub color: Option<String>,
    /// Default for the billable switch.
    pub billable: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Catalog {
    pub options: Vec<WorkOption>,
    /// Up to five recently used pairs, most recent first (⌘1–⌘5).
    pub recents: Vec<WorkOption>,
}

/// What the entry form starts with.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct FormInit {
    pub entry_id: Option<String>,
    /// Editing a group of entries at once: their ids. The form then changes what they share
    /// (project, task, notes, billable) and leaves their times alone.
    #[serde(default)]
    pub group_ids: Vec<String>,
    pub date: String,
    /// `HH:MM` (24-hour, for `<input type="time">`).
    pub start_time: String,
    /// Empty for new entries, `H:MM` when editing.
    pub duration: String,
    pub project_id: Option<String>,
    pub task_id: Option<String>,
    pub description: String,
    pub billable: bool,
    pub running: bool,
    #[serde(default)]
    pub kind: EntryKind,
}

/// A short message for the person, also announced to VoiceOver.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Notice {
    pub kind: NoticeKind,
    pub text: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NoticeKind {
    Info,
    Error,
}
