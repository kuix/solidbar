use objc2_foundation::{NSDateFormatter, NSLocale, NSString};

/// Whether the person's region shows times with AM/PM (System Settings › Language & Region).
pub fn uses_12_hour_clock() -> bool {
    let locale = NSLocale::currentLocale();
    // "j" is the locale's preferred hour symbol; the resulting pattern contains "a" (the
    // AM/PM marker) only for 12-hour clocks.
    NSDateFormatter::dateFormatFromTemplate_options_locale(
        &NSString::from_str("j"),
        0,
        Some(&locale),
    )
    .is_some_and(|pattern| pattern.to_string().contains('a'))
}
