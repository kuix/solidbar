//! Turning "2 hours on this day" into a start and end.
//!
//! New entries stack: they start where the day's last finished entry ends, or at the start
//! of the workday when the day is empty. This never overlaps existing entries, which matters
//! for organizations that reject overlapping time entries.

use jiff::civil::{Date, Time};
use jiff::{SignedDuration, Timestamp};
use thiserror::Error;

use super::entry::{TimeEntry, entries_on};
use crate::time::{Bounds, Calendar};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Error)]
pub enum IntervalError {
    #[error("The start time must be on the selected day")]
    StartOutsideDay,
    #[error("A time entry can't continue past midnight")]
    CrossesMidnight,
}

/// Where a new entry on `date` starts unless the person picks another time.
pub fn default_start(
    calendar: &Calendar,
    date: Date,
    entries: &[TimeEntry],
    workday_start: Time,
) -> Timestamp {
    entries_on(calendar, date, entries)
        .filter_map(|entry| entry.end)
        .max()
        .unwrap_or_else(|| calendar.at(date, workday_start))
}

/// The interval of an entry that starts at `start` and lasts `minutes`, which must stay
/// within `date`.
pub fn interval_on(
    calendar: &Calendar,
    date: Date,
    start: Timestamp,
    minutes: u32,
) -> Result<Bounds, IntervalError> {
    let day = calendar.day_bounds(date);
    if !day.contains(start) {
        return Err(IntervalError::StartOutsideDay);
    }
    let end = start + SignedDuration::from_mins(i64::from(minutes));
    if end > day.end {
        return Err(IntervalError::CrossesMidnight);
    }
    Ok(Bounds { start, end })
}

/// Whether an interval would overlap a running timer. A running entry occupies everything
/// from its start onwards. solidtime doesn't reject this, so the form only warns.
pub fn overlaps_running_timer(interval: Bounds, entries: &[TimeEntry]) -> bool {
    entries
        .iter()
        .filter(|entry| entry.is_running())
        .any(|running| interval.end > running.start)
}

/// Start of a timer started from the form: `elapsed_minutes` already worked count as if the
/// timer had been started that long ago.
pub fn timer_start(now: Timestamp, elapsed_minutes: Option<u32>) -> Timestamp {
    now - SignedDuration::from_mins(i64::from(elapsed_minutes.unwrap_or(0)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::testing::{budapest, finished, running, ts};
    use jiff::civil::{date, time};

    const NINE: Time = time(9, 0, 0, 0);

    #[test]
    fn empty_day_starts_at_the_workday_start() {
        let start = default_start(&budapest(), date(2026, 10, 9), &[], NINE);
        assert_eq!(start, ts("2026-10-09T07:00:00Z"));
    }

    #[test]
    fn stacks_after_the_latest_finished_entry_of_that_day() {
        let entries = [
            finished("a", "2026-10-09T07:00:00Z", "2026-10-09T09:00:00Z"),
            finished("b", "2026-10-09T10:00:00Z", "2026-10-09T10:30:00Z"),
            finished("other-day", "2026-10-10T07:00:00Z", "2026-10-10T12:00:00Z"),
        ];
        let start = default_start(&budapest(), date(2026, 10, 9), &entries, NINE);
        assert_eq!(start, ts("2026-10-09T10:30:00Z"));
    }

    #[test]
    fn early_entries_win_over_the_workday_start() {
        let entries = [finished(
            "a",
            "2026-10-09T05:00:00Z",
            "2026-10-09T06:00:00Z",
        )];
        let start = default_start(&budapest(), date(2026, 10, 9), &entries, NINE);
        assert_eq!(start, ts("2026-10-09T06:00:00Z"));
    }

    #[test]
    fn running_entries_do_not_move_the_stack() {
        let entries = [
            finished("a", "2026-10-09T07:00:00Z", "2026-10-09T08:00:00Z"),
            running("timer", "2026-10-09T09:00:00Z"),
        ];
        let start = default_start(&budapest(), date(2026, 10, 9), &entries, NINE);
        assert_eq!(start, ts("2026-10-09T08:00:00Z"));
    }

    #[test]
    fn interval_must_end_by_midnight() {
        let calendar = budapest();
        let day = date(2026, 10, 9);
        let late = ts("2026-10-09T20:00:00Z"); // 22:00 local
        assert_eq!(
            interval_on(&calendar, day, late, 120),
            Ok(Bounds {
                start: late,
                end: ts("2026-10-09T22:00:00Z")
            })
        );
        assert_eq!(
            interval_on(&calendar, day, late, 121),
            Err(IntervalError::CrossesMidnight)
        );
        assert_eq!(
            interval_on(&calendar, day, ts("2026-10-09T22:00:00Z"), 30),
            Err(IntervalError::StartOutsideDay)
        );
    }

    #[test]
    fn warns_when_an_interval_reaches_into_a_running_timer() {
        let entries = [running("timer", "2026-10-09T09:00:00Z")];
        let before = Bounds {
            start: ts("2026-10-09T08:00:00Z"),
            end: ts("2026-10-09T09:00:00Z"),
        };
        let into = Bounds {
            start: ts("2026-10-09T08:00:00Z"),
            end: ts("2026-10-09T09:00:01Z"),
        };
        assert!(!overlaps_running_timer(before, &entries));
        assert!(overlaps_running_timer(into, &entries));
    }

    #[test]
    fn timer_from_the_form_counts_already_worked_minutes() {
        let now = ts("2026-10-09T10:00:00Z");
        assert_eq!(timer_start(now, None), now);
        assert_eq!(timer_start(now, Some(90)), ts("2026-10-09T08:30:00Z"));
    }
}
