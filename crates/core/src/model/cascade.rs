//! ±1h on an entry, keeping the rest of the day intact.
//!
//! Lengthening a finished entry pushes every later finished entry of the same day by the same
//! amount, so the day keeps its shape and nothing overlaps. The updates are ordered so that
//! no intermediate state overlaps either, because organizations may reject overlapping
//! entries on every single request.

use jiff::{SignedDuration, Timestamp};
use thiserror::Error;

use super::entry::{TimeEntry, entries_on};
use crate::time::Calendar;

/// Entries can't be shortened below this.
pub const MIN_DURATION: SignedDuration = SignedDuration::from_secs(60);

/// One `PUT` of the plan, with the values it replaces so it can be undone.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PlannedPut {
    pub id: String,
    pub start: Timestamp,
    pub end: Option<Timestamp>,
    pub original_start: Timestamp,
    pub original_end: Option<Timestamp>,
}

impl PlannedPut {
    /// The update that restores the entry.
    pub fn inverse(&self) -> PlannedPut {
        PlannedPut {
            id: self.id.clone(),
            start: self.original_start,
            end: self.original_end,
            original_start: self.start,
            original_end: self.end,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Error)]
pub enum AdjustError {
    #[error("That time entry no longer exists")]
    NotFound,
    #[error("A time entry must be at least one minute long")]
    TooShort,
    #[error("The day's entries would run past midnight")]
    CrossesMidnight,
    #[error("The day's entries would run into the running timer")]
    OverlapsRunningTimer,
}

/// Plans the updates for adding `delta` (negative to remove time) to the entry `id`.
///
/// - Finished entry: its end moves, and so does every later finished entry of its day.
///   Apply the returned updates in order.
/// - Running entry: its start moves the other way (more time means an earlier start).
pub fn plan_adjust(
    calendar: &Calendar,
    entries: &[TimeEntry],
    id: &str,
    delta: SignedDuration,
    now: Timestamp,
) -> Result<Vec<PlannedPut>, AdjustError> {
    let target = entries
        .iter()
        .find(|entry| entry.id == id)
        .ok_or(AdjustError::NotFound)?;
    let date = calendar.date_of(target.start);
    let day = calendar.day_bounds(date);

    let Some(original_end) = target.end else {
        let start = target.start - delta;
        if now.duration_since(start) < MIN_DURATION {
            return Err(AdjustError::TooShort);
        }
        if start < day.start {
            return Err(AdjustError::CrossesMidnight);
        }
        return Ok(vec![PlannedPut {
            id: target.id.clone(),
            start,
            end: None,
            original_start: target.start,
            original_end: None,
        }]);
    };

    let new_end = original_end + delta;
    if new_end.duration_since(target.start) < MIN_DURATION {
        return Err(AdjustError::TooShort);
    }

    let same_day: Vec<&TimeEntry> = entries_on(calendar, date, entries)
        .filter(|entry| entry.id != target.id)
        .collect();
    let mut later: Vec<&TimeEntry> = same_day
        .iter()
        .copied()
        .filter(|entry| !entry.is_running() && entry.start >= original_end)
        .collect();
    later.sort_by_key(|entry| entry.start);

    let shifted = later.iter().map(|entry| PlannedPut {
        id: entry.id.clone(),
        start: entry.start + delta,
        end: entry.end.map(|end| end + delta),
        original_start: entry.start,
        original_end: entry.end,
    });
    let target_put = PlannedPut {
        id: target.id.clone(),
        start: target.start,
        end: Some(new_end),
        original_start: target.start,
        original_end: target.end,
    };

    let latest_end = shifted
        .clone()
        .filter_map(|put| put.end)
        .chain([new_end])
        .max()
        .expect("the target is always part of the plan");
    if latest_end > day.end {
        return Err(AdjustError::CrossesMidnight);
    }
    if delta.is_positive() {
        let pushed_into_timer = same_day
            .iter()
            .filter(|entry| entry.is_running() && entry.start >= original_end)
            .any(|running| latest_end > running.start);
        if pushed_into_timer {
            return Err(AdjustError::OverlapsRunningTimer);
        }
    }

    // Growing: make room from the back, so each entry moves into space already vacated.
    // Shrinking: shrink the target first, then pull the others forward in order.
    let plan = if delta.is_positive() {
        shifted.rev().chain([target_put]).collect()
    } else {
        [target_put].into_iter().chain(shifted).collect()
    };
    Ok(plan)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::testing::{budapest, finished, running, ts};
    use proptest::prelude::*;

    const HOUR: SignedDuration = SignedDuration::from_hours(1);

    fn ids(plan: &[PlannedPut]) -> Vec<&str> {
        plan.iter().map(|put| put.id.as_str()).collect()
    }

    fn day() -> Vec<TimeEntry> {
        vec![
            finished("a", "2026-10-09T07:00:00Z", "2026-10-09T08:00:00Z"),
            finished("b", "2026-10-09T08:00:00Z", "2026-10-09T10:00:00Z"),
            finished("c", "2026-10-09T10:30:00Z", "2026-10-09T11:00:00Z"),
            finished("tomorrow", "2026-10-10T07:00:00Z", "2026-10-10T08:00:00Z"),
        ]
    }

    #[test]
    fn adding_an_hour_pushes_later_entries_from_the_back() {
        let plan = plan_adjust(&budapest(), &day(), "b", HOUR, ts("2026-10-09T12:00:00Z")).unwrap();
        assert_eq!(ids(&plan), ["c", "b"]);
        assert_eq!(plan[0].start, ts("2026-10-09T11:30:00Z"));
        assert_eq!(plan[0].end, Some(ts("2026-10-09T12:00:00Z")));
        assert_eq!(plan[1].start, ts("2026-10-09T08:00:00Z"));
        assert_eq!(plan[1].end, Some(ts("2026-10-09T11:00:00Z")));
    }

    #[test]
    fn removing_an_hour_shrinks_first_then_pulls_forward() {
        let plan = plan_adjust(
            &budapest(),
            &day(),
            "a",
            -HOUR / 2,
            ts("2026-10-09T12:00:00Z"),
        )
        .unwrap();
        assert_eq!(ids(&plan), ["a", "b", "c"]);
        assert_eq!(plan[0].end, Some(ts("2026-10-09T07:30:00Z")));
        assert_eq!(plan[1].start, ts("2026-10-09T07:30:00Z"));
        assert_eq!(plan[2].end, Some(ts("2026-10-09T10:30:00Z")));
    }

    #[test]
    fn the_last_entry_of_the_day_moves_alone() {
        let plan = plan_adjust(&budapest(), &day(), "c", HOUR, ts("2026-10-09T12:00:00Z")).unwrap();
        assert_eq!(ids(&plan), ["c"]);
    }

    #[test]
    fn inverse_restores_the_original_values() {
        let plan = plan_adjust(&budapest(), &day(), "b", HOUR, ts("2026-10-09T12:00:00Z")).unwrap();
        let undo = plan[1].inverse();
        assert_eq!(undo.end, Some(ts("2026-10-09T10:00:00Z")));
        assert_eq!(undo.original_end, plan[1].end);
    }

    #[test]
    fn refuses_to_go_below_one_minute() {
        let result = plan_adjust(&budapest(), &day(), "a", -HOUR, ts("2026-10-09T12:00:00Z"));
        assert_eq!(result, Err(AdjustError::TooShort));
    }

    #[test]
    fn refuses_to_push_past_midnight() {
        let entries = [
            finished("a", "2026-10-09T19:00:00Z", "2026-10-09T20:00:00Z"),
            finished("b", "2026-10-09T21:00:00Z", "2026-10-09T21:30:00Z"),
        ];
        let result = plan_adjust(&budapest(), &entries, "a", HOUR, ts("2026-10-09T23:00:00Z"));
        assert_eq!(result, Err(AdjustError::CrossesMidnight));
    }

    #[test]
    fn refuses_to_push_into_a_running_timer_but_may_pull_away_from_it() {
        let entries = [
            finished("a", "2026-10-09T07:00:00Z", "2026-10-09T08:00:00Z"),
            running("timer", "2026-10-09T08:30:00Z"),
        ];
        let now = ts("2026-10-09T09:00:00Z");
        assert_eq!(
            plan_adjust(&budapest(), &entries, "a", HOUR, now),
            Err(AdjustError::OverlapsRunningTimer)
        );
        let shrink = plan_adjust(&budapest(), &entries, "a", -HOUR / 2, now).unwrap();
        assert_eq!(ids(&shrink), ["a"]);
    }

    #[test]
    fn running_entries_move_their_start() {
        let entries = [running("timer", "2026-10-09T08:30:00Z")];
        let now = ts("2026-10-09T09:00:00Z");
        let plan = plan_adjust(&budapest(), &entries, "timer", HOUR, now).unwrap();
        assert_eq!(plan[0].start, ts("2026-10-09T07:30:00Z"));
        assert_eq!(plan[0].end, None);
        assert_eq!(
            plan_adjust(&budapest(), &entries, "timer", -HOUR, now),
            Err(AdjustError::TooShort)
        );
    }

    #[test]
    fn unknown_entries_are_reported() {
        let result = plan_adjust(
            &budapest(),
            &day(),
            "gone",
            HOUR,
            ts("2026-10-09T12:00:00Z"),
        );
        assert_eq!(result, Err(AdjustError::NotFound));
    }

    /// A day of non-overlapping finished entries as (start, end) minutes after local midnight.
    fn arbitrary_day() -> impl Strategy<Value = Vec<(i64, i64)>> {
        prop::collection::vec((0i64..120, 1i64..240), 1..7).prop_map(|parts| {
            let mut cursor = 0;
            parts
                .into_iter()
                .map(|(gap, length)| {
                    let start = cursor + gap;
                    cursor = start + length;
                    (start, cursor)
                })
                .filter(|(_, end)| *end <= 24 * 60)
                .collect()
        })
    }

    fn overlapping(entries: &[TimeEntry]) -> bool {
        entries.iter().enumerate().any(|(index, first)| {
            entries[index + 1..].iter().any(|second| {
                first.start < second.end.unwrap() && second.start < first.end.unwrap()
            })
        })
    }

    proptest! {
        #[test]
        fn cascades_never_overlap_and_preserve_the_shape_of_the_day(
            spans in arbitrary_day(),
            target_seed in any::<prop::sample::Index>(),
            delta_minutes in prop::sample::select(vec![-120i64, -60, -30, 30, 60, 120]),
        ) {
            prop_assume!(!spans.is_empty());
            let calendar = budapest();
            let midnight = calendar.day_bounds(jiff::civil::date(2026, 10, 9)).start;
            let at = |minutes: i64| midnight + SignedDuration::from_mins(minutes);
            let mut entries: Vec<TimeEntry> = spans
                .iter()
                .enumerate()
                .map(|(index, (start, end))| TimeEntry {
                    end: Some(at(*end)),
                    ..running(&format!("e{index}"), &at(*start).to_string())
                })
                .collect();
            let target = target_seed.index(entries.len());
            let delta = SignedDuration::from_mins(delta_minutes);
            let (target_start, target_end) = spans[target];
            let now = at(24 * 60);

            match plan_adjust(&calendar, &entries, &format!("e{target}"), delta, now) {
                Ok(plan) => {
                    for put in &plan {
                        let entry = entries.iter_mut().find(|entry| entry.id == put.id).unwrap();
                        prop_assert_eq!(entry.start, put.original_start);
                        prop_assert_eq!(entry.end, put.original_end);
                        entry.start = put.start;
                        entry.end = put.end;
                        prop_assert!(!overlapping(&entries), "overlap after updating {}", put.id);
                    }
                    for (index, (start, end)) in spans.iter().enumerate() {
                        let entry = &entries[index];
                        let (expected_start, expected_end) = if index == target {
                            (*start, end + delta_minutes)
                        } else if *start >= target_end {
                            (start + delta_minutes, end + delta_minutes)
                        } else {
                            (*start, *end)
                        };
                        prop_assert_eq!(entry.start, at(expected_start));
                        prop_assert_eq!(entry.end, Some(at(expected_end)));
                    }
                }
                Err(AdjustError::TooShort) => {
                    prop_assert!(target_end + delta_minutes - target_start < 1);
                }
                Err(AdjustError::CrossesMidnight) => {
                    let latest = spans
                        .iter()
                        .enumerate()
                        .filter(|(index, (start, _))| *index == target || *start >= target_end)
                        .map(|(_, (_, end))| end + delta_minutes)
                        .max()
                        .unwrap();
                    prop_assert!(latest > 24 * 60);
                }
                Err(other) => prop_assert!(false, "unexpected {other:?}"),
            }
        }
    }
}
