//! Times of day as people type them and as Solidbar shows them: 24-hour (`14:30`) or 12-hour
//! (`2:30 PM`), as set in Settings or, by default, as on the Mac.

use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum ClockFormat {
    /// Like macOS (System Settings › General › Date & Time › 24-hour time).
    #[default]
    #[serde(rename = "system")]
    System,
    #[serde(rename = "24h")]
    TwentyFourHour,
    #[serde(rename = "12h")]
    TwelveHour,
}

impl ClockFormat {
    pub fn hour12(self, mac_uses_12_hours: bool) -> bool {
        match self {
            ClockFormat::System => mac_uses_12_hours,
            ClockFormat::TwentyFourHour => false,
            ClockFormat::TwelveHour => true,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Time {
    pub hour: u8,
    pub minute: u8,
}

impl Time {
    const LAST_MINUTE: i32 = 24 * 60 - 1;

    /// Reads what someone typed: `9`, `09:30`, `9.30`, `930`, `14:30`, or with AM/PM `9:30 pm`,
    /// `9p`, `12 a.m.`. Without AM/PM the hour counts from 0 to 23, in either format.
    pub fn parse(text: &str) -> Option<Time> {
        let text: String = text
            .chars()
            .filter(|char| !char.is_whitespace())
            .collect::<String>()
            .to_lowercase();
        let (text, meridiem) = split_meridiem(&text);
        let (hour, minute) = match text.split_once([':', '.']) {
            Some((hour, minute)) if minute.len() == 2 => (number(hour)?, number(minute)?),
            Some(_) => return None,
            None => match text.len() {
                1 | 2 => (number(text)?, 0),
                3 | 4 => {
                    let (hour, minute) = text.split_at(text.len() - 2);
                    (number(hour)?, number(minute)?)
                }
                _ => return None,
            },
        };
        let hour = match meridiem {
            None if hour < 24 => hour,
            Some(pm) if (1..=12).contains(&hour) => hour % 12 + if pm { 12 } else { 0 },
            _ => return None,
        };
        (minute < 60).then_some(Time { hour, minute })
    }

    /// `09:30`, the form the backend and the settings file use.
    pub fn to_hm(self) -> String {
        format!("{:02}:{:02}", self.hour, self.minute)
    }

    /// `09:30` or `9:30 AM`.
    pub fn display(self, hour12: bool) -> String {
        if !hour12 {
            return self.to_hm();
        }
        let suffix = if self.hour < 12 { "AM" } else { "PM" };
        let hour = match self.hour % 12 {
            0 => 12,
            hour => hour,
        };
        format!("{hour}:{:02} {suffix}", self.minute)
    }

    /// Moved by `minutes`, staying within the day.
    pub fn add_minutes(self, minutes: i32) -> Time {
        let total = (i32::from(self.hour) * 60 + i32::from(self.minute) + minutes)
            .clamp(0, Self::LAST_MINUTE);
        Time {
            hour: (total / 60) as u8,
            minute: (total % 60) as u8,
        }
    }
}

/// Splits off `am`/`pm` (also `a`, `p`, `a.m.`); `Some(true)` for PM.
fn split_meridiem(text: &str) -> (&str, Option<bool>) {
    for (suffix, pm) in [
        ("a.m.", false),
        ("p.m.", true),
        ("am", false),
        ("pm", true),
        ("a", false),
        ("p", true),
    ] {
        if let Some(rest) = text.strip_suffix(suffix) {
            return (rest, Some(pm));
        }
    }
    (text, None)
}

fn number(digits: &str) -> Option<u8> {
    if digits.is_empty() || !digits.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    digits.parse().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hm(text: &str) -> Option<String> {
        Time::parse(text).map(Time::to_hm)
    }

    #[test]
    fn reads_24_hour_times() {
        assert_eq!(hm("9").as_deref(), Some("09:00"));
        assert_eq!(hm("09:30").as_deref(), Some("09:30"));
        assert_eq!(hm("9.30").as_deref(), Some("09:30"));
        assert_eq!(hm("930").as_deref(), Some("09:30"));
        assert_eq!(hm("1430").as_deref(), Some("14:30"));
        assert_eq!(hm(" 14 : 05 ").as_deref(), Some("14:05"));
        assert_eq!(hm("0").as_deref(), Some("00:00"));
        assert_eq!(hm("23:59").as_deref(), Some("23:59"));
    }

    #[test]
    fn reads_12_hour_times() {
        assert_eq!(hm("9:30 pm").as_deref(), Some("21:30"));
        assert_eq!(hm("9p").as_deref(), Some("21:00"));
        assert_eq!(hm("9:30 AM").as_deref(), Some("09:30"));
        assert_eq!(hm("12 a.m.").as_deref(), Some("00:00"));
        assert_eq!(hm("12pm").as_deref(), Some("12:00"));
        assert_eq!(hm("1215a").as_deref(), Some("00:15"));
    }

    #[test]
    fn rejects_what_is_not_a_time() {
        for text in [
            "", "24:00", "9:60", "9:3", "13pm", "0am", "12345", "nine", "-1", "9:30:00", "+9",
        ] {
            assert_eq!(hm(text), None, "{text:?}");
        }
    }

    #[test]
    fn shows_either_format() {
        let time = Time::parse("21:05").unwrap();
        assert_eq!(time.display(false), "21:05");
        assert_eq!(time.display(true), "9:05 PM");
        assert_eq!(Time::parse("00:30").unwrap().display(true), "12:30 AM");
        assert_eq!(Time::parse("12:00").unwrap().display(true), "12:00 PM");
    }

    #[test]
    fn steps_stay_within_the_day() {
        let time = Time::parse("23:50").unwrap();
        assert_eq!(time.add_minutes(15).to_hm(), "23:59");
        assert_eq!(
            Time::parse("00:10").unwrap().add_minutes(-15).to_hm(),
            "00:00"
        );
        assert_eq!(
            Time::parse("09:00").unwrap().add_minutes(-60).to_hm(),
            "08:00"
        );
    }

    #[test]
    fn the_format_follows_the_mac_unless_set() {
        assert!(ClockFormat::System.hour12(true));
        assert!(!ClockFormat::System.hour12(false));
        assert!(!ClockFormat::TwentyFourHour.hour12(true));
        assert!(ClockFormat::TwelveHour.hour12(false));
        assert_eq!(
            serde_json::to_value(ClockFormat::TwentyFourHour).unwrap(),
            "24h"
        );
    }
}
