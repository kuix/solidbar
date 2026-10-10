use jiff::Timestamp;
use jiff::civil::Date;
use serde::{Deserialize, Serialize};
use solidbar_shared::view::EntryKind;

use crate::time::Calendar;

/// A time entry as the rest of the app reasons about it (mapped from the API resource).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TimeEntry {
    pub id: String,
    pub start: Timestamp,
    /// `None` while the timer is running.
    pub end: Option<Timestamp>,
    pub project_id: Option<String>,
    pub task_id: Option<String>,
    pub description: String,
    pub billable: bool,
    /// Work or a break; caches written before breaks existed hold only work.
    #[serde(default)]
    pub kind: EntryKind,
}

impl TimeEntry {
    pub fn is_running(&self) -> bool {
        self.end.is_none()
    }

    pub fn is_break(&self) -> bool {
        self.kind == EntryKind::Break
    }

    /// Elapsed whole seconds; a running entry counts up to `now`.
    pub fn duration_seconds(&self, now: Timestamp) -> i64 {
        self.end
            .unwrap_or(now)
            .duration_since(self.start)
            .as_secs()
            .max(0)
    }
}

/// Entries belong to the day on which they start, as in solidtime's own views.
pub fn entries_on<'a>(
    calendar: &Calendar,
    date: Date,
    entries: &'a [TimeEntry],
) -> impl Iterator<Item = &'a TimeEntry> {
    let bounds = calendar.day_bounds(date);
    entries
        .iter()
        .filter(move |entry| bounds.contains(entry.start))
}
