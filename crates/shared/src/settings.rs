//! Preferences stored on this Mac.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::clock::ClockFormat;
use crate::instance::InstanceSettings;
use crate::shortcuts::GlobalAction;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Theme {
    #[default]
    System,
    Dark,
    Light,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub instance: InstanceSettings,
    /// The membership (organization) new entries go to.
    pub active_membership_id: Option<String>,
    /// `HH:MM`; where the first entry of an empty day starts.
    pub workday_start: String,
    pub theme: Theme,
    /// 24- or 12-hour times; by default like macOS.
    pub clock: ClockFormat,
    /// Show the timer or today's total next to the menu bar icon.
    pub menu_bar_text: bool,
    /// Accelerators per global action; `None` disables the shortcut.
    pub shortcuts: BTreeMap<GlobalAction, Option<String>>,
    /// N, T, S, B, E and R in the list. Can be turned off (WCAG 2.1.4), e.g. for Voice Control,
    /// where a spoken word could set one off.
    pub single_key_shortcuts: bool,
    pub launch_at_login_offered: bool,
    /// The MCP server for AI agents.
    pub agents: AgentAccess,
    /// Membership id → its favorites, in the order they were added.
    pub favorites: BTreeMap<String, Vec<Favorite>>,
}

/// An entry kept to be added again with a click (the entry form's ☆).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Favorite {
    pub project_id: Option<String>,
    pub task_id: Option<String>,
    #[serde(default)]
    pub description: String,
    /// `None` leaves the duration to be typed.
    #[serde(default)]
    pub minutes: Option<u32>,
    #[serde(default)]
    pub billable: bool,
}

impl Favorite {
    /// The same work and notes: saved again, a favorite replaces the one it's like.
    pub fn is_like(&self, other: &Favorite) -> bool {
        self.project_id == other.project_id
            && self.task_id == other.task_id
            && self.description == other.description
    }
}

/// The local MCP server that lets AI agents read and add time entries (Settings › AI agents).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct AgentAccess {
    pub enabled: bool,
    /// Picked once, a free one, so the address agents were given keeps working.
    pub port: Option<u16>,
    /// What agents send as `Authorization: Bearer …`.
    pub key: Option<String>,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            instance: InstanceSettings::default(),
            active_membership_id: None,
            workday_start: "09:00".to_owned(),
            theme: Theme::System,
            clock: ClockFormat::System,
            menu_bar_text: true,
            shortcuts: GlobalAction::ALL
                .into_iter()
                .map(|action| (action, Some(action.default_accelerator().to_owned())))
                .collect(),
            single_key_shortcuts: true,
            launch_at_login_offered: false,
            agents: AgentAccess::default(),
            favorites: BTreeMap::new(),
        }
    }
}

impl Settings {
    pub fn shortcut(&self, action: GlobalAction) -> Option<&str> {
        match self.shortcuts.get(&action) {
            Some(accelerator) => accelerator.as_deref(),
            None => Some(action.default_accelerator()),
        }
    }
}

/// A partial update from the settings view; `None` fields stay unchanged.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct SettingsPatch {
    pub workday_start: Option<String>,
    pub theme: Option<Theme>,
    pub clock: Option<ClockFormat>,
    pub menu_bar_text: Option<bool>,
    pub single_key_shortcuts: Option<bool>,
    pub launch_at_login_offered: Option<bool>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_fields_fall_back_to_defaults() {
        let settings: Settings = serde_json::from_str(r#"{ "theme": "dark" }"#).unwrap();
        assert_eq!(settings.theme, Theme::Dark);
        assert_eq!(settings.workday_start, "09:00");
        assert!(settings.single_key_shortcuts);
        assert!(!settings.agents.enabled);
        assert_eq!(
            settings.shortcut(GlobalAction::TogglePanel),
            Some("Alt+Super+KeyT")
        );
    }

    #[test]
    fn a_disabled_shortcut_stays_disabled() {
        let mut settings = Settings::default();
        settings.shortcuts.insert(GlobalAction::NewEntry, None);
        let round_trip: Settings =
            serde_json::from_str(&serde_json::to_string(&settings).unwrap()).unwrap();
        assert_eq!(round_trip.shortcut(GlobalAction::NewEntry), None);
    }
}
