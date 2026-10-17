//! Today's date, the key of everything made once a day (the week's goals,
//! the day's plan in work.rs).

/// Today: `2026-09-24`, local time.
pub fn date() -> String {
    jiff::Zoned::now().date().to_string()
}
