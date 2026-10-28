//! Shared numeric and calendar helpers for the read models, so every page
//! computes success, averages and weeks the same way.

use chrono::{Datelike, Duration, Local, NaiveDate, TimeZone};

use crate::store::SessionRow;

pub const DAY_MS: i64 = 86_400_000;
pub const HOUR_MS: f64 = 3_600_000.0;

/// Below this many sessions a rate or comparison is noise, so it is shown
/// greyed out or not at all.
pub const MIN_SAMPLE: usize = 5;

pub fn mean(xs: impl Iterator<Item = f64>) -> Option<f64> {
    let v: Vec<f64> = xs.collect();
    (!v.is_empty()).then(|| v.iter().sum::<f64>() / v.len() as f64)
}

pub fn ratio(num: i64, den: i64) -> Option<f64> {
    (den > 0).then(|| num as f64 / den as f64)
}

/// How many of `rows` are rated, and their mean outcome score.
pub fn success<'a>(rows: impl IntoIterator<Item = &'a SessionRow>) -> (usize, Option<f64>) {
    let scores: Vec<f64> = rows.into_iter().filter_map(SessionRow::score).collect();
    let rate = (!scores.is_empty()).then(|| scores.iter().sum::<f64>() / scores.len() as f64);
    (scores.len(), rate)
}

pub fn local_date(ms: i64) -> Option<NaiveDate> {
    Local
        .timestamp_millis_opt(ms)
        .single()
        .map(|d| d.date_naive())
}

/// Local midnight at the start of `d`, in Unix milliseconds.
pub fn local_midnight_ms(d: NaiveDate) -> i64 {
    d.and_hms_opt(0, 0, 0)
        .and_then(|d| d.and_local_timezone(Local).earliest())
        .map(|d| d.timestamp_millis())
        .unwrap_or(0)
}

/// The Monday of the week containing `d`.
pub fn monday(d: NaiveDate) -> NaiveDate {
    d - Duration::days(d.weekday().num_days_from_monday() as i64)
}

/// The Monday of the local week a timestamp falls in.
pub fn week_of(ms: i64) -> Option<NaiveDate> {
    local_date(ms).map(monday)
}

pub fn pct(x: f64) -> String {
    format!("{:.0}%", x * 100.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn monday_of_week() {
        let thu = NaiveDate::from_ymd_opt(2026, 10, 29).unwrap();
        assert_eq!(monday(thu), NaiveDate::from_ymd_opt(2026, 10, 26).unwrap());
        assert_eq!(monday(monday(thu)), monday(thu));
    }

    #[test]
    fn mean_and_ratio_of_nothing_are_none() {
        assert_eq!(mean(std::iter::empty()), None);
        assert_eq!(ratio(3, 0), None);
        assert_eq!(ratio(1, 4), Some(0.25));
    }
}
