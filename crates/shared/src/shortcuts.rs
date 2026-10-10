//! System-wide shortcuts and their defaults.
//!
//! Accelerators use the `global-hotkey` syntax: modifiers (`Alt`, `Super`, `Control`, `Shift`)
//! joined with `+`, followed by a `KeyboardEvent.code` value such as `KeyT` or `Digit1`.

use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GlobalAction {
    /// Show or hide the panel.
    TogglePanel,
    /// Show the panel with the new entry form open.
    NewEntry,
    /// Stop the running timer, or restart the most recent project/task pair.
    ToggleTimer,
}

impl GlobalAction {
    pub const ALL: [GlobalAction; 3] = [Self::TogglePanel, Self::NewEntry, Self::ToggleTimer];

    /// Harvest-compatible defaults: ⌥⌘T, ⌥⌘N, ⌥⌘S.
    pub const fn default_accelerator(self) -> &'static str {
        match self {
            Self::TogglePanel => "Alt+Super+KeyT",
            Self::NewEntry => "Alt+Super+KeyN",
            Self::ToggleTimer => "Alt+Super+KeyS",
        }
    }

    pub const fn label(self) -> &'static str {
        match self {
            Self::TogglePanel => "Show or hide Solidbar",
            Self::NewEntry => "New time entry",
            Self::ToggleTimer => "Start or stop the timer",
        }
    }
}
