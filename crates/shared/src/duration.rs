//! Parsing and formatting of the durations people type into the entry form.

use thiserror::Error;

/// The longest duration a single entry may have. Entries never cross midnight.
pub const MAX_MINUTES: u32 = 24 * 60;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Error)]
pub enum DurationError {
    #[error("Enter a duration, for example 2, 1:30 or 45m")]
    Empty,
    #[error("Use a duration like 2, 1.5, 1:30, 2h or 45m")]
    Invalid,
    #[error("The duration must be at least one minute")]
    Zero,
    #[error("A time entry can't be longer than 24 hours")]
    TooLong,
}

/// Parses a duration typed by a person into whole minutes.
///
/// Accepted forms: hours (`2`, `1.5`, `1,5`), `H:MM` (`1:30`), unit suffixes (`2h`, `45m`,
/// `1h30m`, `1h 30`). A bare number means hours, as in Harvest.
pub fn parse_minutes(input: &str) -> Result<u32, DurationError> {
    let text = input.trim().to_ascii_lowercase().replace(',', ".");
    if text.is_empty() {
        return Err(DurationError::Empty);
    }
    let minutes = if let Some((hours, minutes)) = text.split_once(':') {
        whole_number(hours)? * 60 + minutes_of_hour(minutes)?
    } else if let Some((hours, rest)) = text.split_once('h') {
        let rest = rest.trim();
        let extra = if rest.is_empty() {
            0
        } else {
            minutes_of_hour(rest.strip_suffix('m').unwrap_or(rest).trim_end())?
        };
        decimal_hours_as_minutes(hours.trim_end())? + extra
    } else if let Some(minutes) = text.strip_suffix('m') {
        whole_number(minutes.trim_end())?
    } else {
        decimal_hours_as_minutes(&text)?
    };
    match minutes {
        0 => Err(DurationError::Zero),
        minutes if minutes > u64::from(MAX_MINUTES) => Err(DurationError::TooLong),
        minutes => Ok(minutes as u32),
    }
}

/// Formats seconds as `H:MM`, rounding down to the minute.
pub fn format_hm(seconds: i64) -> String {
    let minutes = seconds.max(0) / 60;
    format!("{}:{:02}", minutes / 60, minutes % 60)
}

fn whole_number(text: &str) -> Result<u64, DurationError> {
    if text.is_empty() || !text.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err(DurationError::Invalid);
    }
    // Only digits remain, so the only possible failure is overflow.
    text.parse().map_err(|_| DurationError::TooLong)
}

/// The `MM` in `H:MM` or `1h30`: one or two digits, below 60.
fn minutes_of_hour(text: &str) -> Result<u64, DurationError> {
    match whole_number(text)? {
        minutes if text.len() <= 2 && minutes < 60 => Ok(minutes),
        _ => Err(DurationError::Invalid),
    }
}

/// `1.5` → 90. Validates the characters first because `f64::from_str` also accepts
/// signs, exponents, `inf` and `NaN`.
fn decimal_hours_as_minutes(text: &str) -> Result<u64, DurationError> {
    let digits = text.bytes().filter(u8::is_ascii_digit).count();
    let points = text.bytes().filter(|byte| *byte == b'.').count();
    if digits == 0 || points > 1 || digits + points != text.len() {
        return Err(DurationError::Invalid);
    }
    let hours: f64 = text.parse().map_err(|_| DurationError::Invalid)?;
    Ok((hours * 60.0).round() as u64)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bare_numbers_are_hours() {
        assert_eq!(parse_minutes("2"), Ok(120));
        assert_eq!(parse_minutes(" 8 "), Ok(480));
    }

    #[test]
    fn decimal_hours_accept_point_and_comma() {
        assert_eq!(parse_minutes("1.5"), Ok(90));
        assert_eq!(parse_minutes("1,5"), Ok(90));
        assert_eq!(parse_minutes(".25"), Ok(15));
        assert_eq!(parse_minutes("0.1"), Ok(6));
        assert_eq!(parse_minutes("1.333"), Ok(80));
    }

    #[test]
    fn colon_separates_hours_and_minutes() {
        assert_eq!(parse_minutes("1:30"), Ok(90));
        assert_eq!(parse_minutes("0:45"), Ok(45));
        assert_eq!(parse_minutes("2:05"), Ok(125));
        assert_eq!(parse_minutes("1:60"), Err(DurationError::Invalid));
        assert_eq!(parse_minutes("1:"), Err(DurationError::Invalid));
        assert_eq!(parse_minutes(":30"), Err(DurationError::Invalid));
    }

    #[test]
    fn unit_suffixes() {
        assert_eq!(parse_minutes("2h"), Ok(120));
        assert_eq!(parse_minutes("2H"), Ok(120));
        assert_eq!(parse_minutes("1.5h"), Ok(90));
        assert_eq!(parse_minutes("45m"), Ok(45));
        assert_eq!(parse_minutes("90m"), Ok(90));
        assert_eq!(parse_minutes("1h30m"), Ok(90));
        assert_eq!(parse_minutes("1h 30m"), Ok(90));
        assert_eq!(parse_minutes("1h30"), Ok(90));
        assert_eq!(parse_minutes("1h 60m"), Err(DurationError::Invalid));
        assert_eq!(parse_minutes("1.5m"), Err(DurationError::Invalid));
    }

    #[test]
    fn rejects_empty_zero_and_too_long() {
        assert_eq!(parse_minutes(""), Err(DurationError::Empty));
        assert_eq!(parse_minutes("   "), Err(DurationError::Empty));
        assert_eq!(parse_minutes("0"), Err(DurationError::Zero));
        assert_eq!(parse_minutes("0:00"), Err(DurationError::Zero));
        assert_eq!(parse_minutes("0.001"), Err(DurationError::Zero));
        assert_eq!(parse_minutes("24"), Ok(MAX_MINUTES));
        assert_eq!(parse_minutes("24:01"), Err(DurationError::TooLong));
        assert_eq!(parse_minutes("25h"), Err(DurationError::TooLong));
    }

    #[test]
    fn rejects_garbage_that_float_parsing_would_accept() {
        for input in [
            "abc", "-1", "+1", "1e2", "inf", "NaN", "1..5", "1.5.5", "1:30:00", "h", "m",
        ] {
            assert_eq!(
                parse_minutes(input),
                Err(DurationError::Invalid),
                "{input:?}"
            );
        }
    }

    #[test]
    fn formats_hours_and_minutes() {
        assert_eq!(format_hm(0), "0:00");
        assert_eq!(format_hm(59), "0:00");
        assert_eq!(format_hm(60), "0:01");
        assert_eq!(format_hm(5_400), "1:30");
        assert_eq!(format_hm(36_000 + 59), "10:00");
        assert_eq!(format_hm(-30), "0:00");
    }
}
