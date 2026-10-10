//! Time entries and the rules for creating and adjusting them.

pub mod cascade;
mod entry;
pub mod recents;
pub mod stacking;

pub use entry::{TimeEntry, entries_on};

#[cfg(test)]
pub(crate) mod testing {
    use jiff::Timestamp;
    use jiff::tz::TimeZone;

    use super::TimeEntry;
    use crate::time::{Calendar, WeekStart};

    pub fn budapest() -> Calendar {
        Calendar::new(TimeZone::get("Europe/Budapest").unwrap(), WeekStart::Monday)
    }

    pub fn ts(text: &str) -> Timestamp {
        text.parse().unwrap()
    }

    pub fn running(id: &str, start: &str) -> TimeEntry {
        TimeEntry {
            id: id.to_owned(),
            start: ts(start),
            end: None,
            project_id: None,
            task_id: None,
            description: String::new(),
            billable: false,
        }
    }

    pub fn finished(id: &str, start: &str, end: &str) -> TimeEntry {
        TimeEntry {
            end: Some(ts(end)),
            ..running(id, start)
        }
    }
}
