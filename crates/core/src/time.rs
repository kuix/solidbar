//! Calendar arithmetic in the user's solidtime time zone and week start.
//!
//! Days are not always 24 hours long: on DST transitions they last 23 or 25 hours, so day
//! bounds always come from the time zone rules, never from adding 24 hours.

use jiff::civil::{Date, Time, Weekday};
use jiff::tz::TimeZone;
use jiff::{Timestamp, Unit};
use serde::{Deserialize, Serialize};

/// solidtime's `week_start` profile setting.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum WeekStart {
    #[default]
    Monday,
    Tuesday,
    Wednesday,
    Thursday,
    Friday,
    Saturday,
    Sunday,
}

impl WeekStart {
    fn weekday(self) -> Weekday {
        match self {
            Self::Monday => Weekday::Monday,
            Self::Tuesday => Weekday::Tuesday,
            Self::Wednesday => Weekday::Wednesday,
            Self::Thursday => Weekday::Thursday,
            Self::Friday => Weekday::Friday,
            Self::Saturday => Weekday::Saturday,
            Self::Sunday => Weekday::Sunday,
        }
    }
}

/// A half-open UTC interval `[start, end)`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Bounds {
    pub start: Timestamp,
    pub end: Timestamp,
}

impl Bounds {
    pub fn contains(&self, instant: Timestamp) -> bool {
        self.start <= instant && instant < self.end
    }
}

/// Seven consecutive days starting on the user's first day of the week.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Week {
    pub first_day: Date,
}

impl Week {
    pub fn days(&self) -> [Date; 7] {
        std::array::from_fn(|offset| {
            self.first_day
                .checked_add(jiff::Span::new().days(offset as i64))
                .expect("dates within the supported range")
        })
    }

    pub fn last_day(&self) -> Date {
        self.days()[6]
    }

    pub fn contains(&self, date: Date) -> bool {
        self.first_day <= date && date <= self.last_day()
    }

    pub fn previous(&self) -> Week {
        Week {
            first_day: self.first_day.saturating_sub(jiff::Span::new().days(7)),
        }
    }

    pub fn next(&self) -> Week {
        Week {
            first_day: self.first_day.saturating_add(jiff::Span::new().days(7)),
        }
    }
}

/// The user's calendar settings, from `GET /api/v1/users/me`.
#[derive(Clone, Debug, PartialEq)]
pub struct Calendar {
    pub time_zone: TimeZone,
    pub week_start: WeekStart,
}

impl Calendar {
    pub fn new(time_zone: TimeZone, week_start: WeekStart) -> Self {
        Self {
            time_zone,
            week_start,
        }
    }

    /// Uses the IANA zone name from the solidtime profile, falling back to the Mac's zone
    /// when the name is unknown.
    pub fn from_profile(time_zone: &str, week_start: WeekStart) -> Self {
        let time_zone = TimeZone::get(time_zone).unwrap_or_else(|_| TimeZone::system());
        Self::new(time_zone, week_start)
    }

    pub fn date_of(&self, instant: Timestamp) -> Date {
        instant.to_zoned(self.time_zone.clone()).date()
    }

    /// The instant of a wall-clock time on a date. Times skipped by a DST gap resolve to the
    /// later side; repeated times resolve to the earlier occurrence.
    pub fn at(&self, date: Date, time: Time) -> Timestamp {
        date.to_datetime(time)
            .to_zoned(self.time_zone.clone())
            .expect("dates within the supported range")
            .timestamp()
    }

    pub fn day_bounds(&self, date: Date) -> Bounds {
        Bounds {
            start: self.start_of(date),
            end: self.start_of(date.tomorrow().expect("dates within the supported range")),
        }
    }

    pub fn week_of(&self, date: Date) -> Week {
        let first_day = if date.weekday() == self.week_start.weekday() {
            date
        } else {
            date.nth_weekday(-1, self.week_start.weekday())
                .expect("dates within the supported range")
        };
        Week { first_day }
    }

    pub fn week_bounds(&self, week: Week) -> Bounds {
        Bounds {
            start: self.start_of(week.first_day),
            end: self.start_of(week.next().first_day),
        }
    }

    fn start_of(&self, date: Date) -> Timestamp {
        date.to_zoned(self.time_zone.clone())
            .and_then(|zoned| zoned.start_of_day())
            .expect("dates within the supported range")
            .timestamp()
    }
}

/// Formats an instant the way solidtime's API expects it: UTC, whole seconds, `Z` suffix.
pub fn to_api(instant: Timestamp) -> String {
    let whole_seconds = instant
        .round(
            jiff::TimestampRound::new()
                .smallest(Unit::Second)
                .mode(jiff::RoundMode::Trunc),
        )
        .expect("rounding down to seconds stays in range");
    whole_seconds.strftime("%Y-%m-%dT%H:%M:%SZ").to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use jiff::civil::{date, time};

    fn budapest() -> Calendar {
        Calendar::new(TimeZone::get("Europe/Budapest").unwrap(), WeekStart::Monday)
    }

    fn ts(text: &str) -> Timestamp {
        text.parse().unwrap()
    }

    #[test]
    fn day_bounds_follow_the_time_zone() {
        let bounds = budapest().day_bounds(date(2026, 10, 9));
        assert_eq!(bounds.start, ts("2026-10-08T22:00:00Z"));
        assert_eq!(bounds.end, ts("2026-10-09T22:00:00Z"));
    }

    #[test]
    fn dst_days_are_23_and_25_hours_long() {
        let calendar = budapest();
        let spring = calendar.day_bounds(date(2026, 3, 29));
        assert_eq!(spring.end.duration_since(spring.start).as_hours(), 23);
        let autumn = calendar.day_bounds(date(2026, 10, 25));
        assert_eq!(autumn.end.duration_since(autumn.start).as_hours(), 25);
    }

    #[test]
    fn wall_clock_times_in_a_dst_gap_resolve_forward() {
        // 02:30 does not exist on 2026-03-29 in Budapest; clocks jump from 02:00 to 03:00.
        let instant = budapest().at(date(2026, 3, 29), time(2, 30, 0, 0));
        assert_eq!(instant, ts("2026-03-29T01:30:00Z"));
        assert_eq!(
            budapest().at(date(2026, 3, 29), time(9, 0, 0, 0)),
            ts("2026-03-29T07:00:00Z")
        );
    }

    #[test]
    fn date_of_uses_local_midnight() {
        let calendar = budapest();
        assert_eq!(
            calendar.date_of(ts("2026-10-09T21:59:59Z")),
            date(2026, 10, 9)
        );
        assert_eq!(
            calendar.date_of(ts("2026-10-09T22:00:00Z")),
            date(2026, 10, 10)
        );
    }

    #[test]
    fn weeks_start_on_the_configured_day() {
        let monday = budapest();
        // 2026-10-09 is a Friday.
        assert_eq!(
            monday.week_of(date(2026, 10, 9)).first_day,
            date(2026, 10, 5)
        );
        assert_eq!(
            monday.week_of(date(2026, 10, 5)).first_day,
            date(2026, 10, 5)
        );
        assert_eq!(
            monday.week_of(date(2026, 10, 11)).first_day,
            date(2026, 10, 5)
        );

        let sunday = Calendar::new(monday.time_zone.clone(), WeekStart::Sunday);
        assert_eq!(
            sunday.week_of(date(2026, 10, 9)).first_day,
            date(2026, 10, 4)
        );
        assert_eq!(
            sunday.week_of(date(2026, 10, 11)).first_day,
            date(2026, 10, 11)
        );
    }

    #[test]
    fn week_days_and_navigation() {
        let week = budapest().week_of(date(2026, 12, 30));
        assert_eq!(week.first_day, date(2026, 12, 28));
        assert_eq!(week.last_day(), date(2027, 1, 3));
        assert!(week.contains(date(2027, 1, 1)));
        assert!(!week.contains(date(2027, 1, 4)));
        assert_eq!(week.next().first_day, date(2027, 1, 4));
        assert_eq!(week.previous().first_day, date(2026, 12, 21));
    }

    #[test]
    fn week_bounds_span_a_dst_change() {
        let calendar = budapest();
        let bounds = calendar.week_bounds(calendar.week_of(date(2026, 10, 25)));
        assert_eq!(bounds.start, ts("2026-10-18T22:00:00Z"));
        assert_eq!(bounds.end, ts("2026-10-25T23:00:00Z"));
    }

    #[test]
    fn unknown_profile_zone_falls_back_to_the_system_zone() {
        let calendar = Calendar::from_profile("Mars/Olympus_Mons", WeekStart::Monday);
        assert_eq!(calendar.time_zone, TimeZone::system());
    }

    #[test]
    fn api_timestamps_are_utc_whole_seconds() {
        assert_eq!(
            to_api(ts("2026-10-09T07:15:42.987Z")),
            "2026-10-09T07:15:42Z"
        );
        assert_eq!(
            to_api(ts("2026-10-09T09:15:00+02:00")),
            "2026-10-09T07:15:00Z"
        );
    }

    #[test]
    fn week_start_deserializes_from_solidtime_values() {
        let week_start: WeekStart = serde_json::from_str("\"sunday\"").unwrap();
        assert_eq!(week_start, WeekStart::Sunday);
    }
}
