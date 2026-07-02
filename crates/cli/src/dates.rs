use anyhow::{Context, bail};
use chrono::{DateTime, Duration, Local, NaiveDate, NaiveDateTime, TimeZone};

/// Parse a `--since`/`--until` argument.
///
/// Accepts `YYYY-MM-DD`, `YYYY-MM-DDTHH:MM`, or relative `7d`/`24h`/`2w`.
/// A bare date means start-of-day; pass `end_of_day` for `--until` semantics.
pub fn parse_date_arg(raw: &str, end_of_day: bool) -> anyhow::Result<DateTime<Local>> {
    let s = raw.trim();
    if let Some(dt) = parse_relative(s)? {
        return Ok(dt);
    }
    if let Ok(dt) = NaiveDateTime::parse_from_str(s, "%Y-%m-%dT%H:%M") {
        return local_from_naive(dt);
    }
    if let Ok(date) = NaiveDate::parse_from_str(s, "%Y-%m-%d") {
        let time = if end_of_day {
            date.and_hms_opt(23, 59, 59)
        } else {
            date.and_hms_opt(0, 0, 0)
        };
        return local_from_naive(time.expect("valid constant time"));
    }
    bail!("invalid date '{raw}' (expected YYYY-MM-DD, YYYY-MM-DDTHH:MM, or 7d/24h/2w)")
}

fn parse_relative(s: &str) -> anyhow::Result<Option<DateTime<Local>>> {
    let Some(unit) = s.chars().last().filter(|c| "dhw".contains(*c)) else {
        return Ok(None);
    };
    let num = &s[..s.len() - 1];
    if num.is_empty() || !num.chars().all(|c| c.is_ascii_digit()) {
        return Ok(None);
    }
    let n: i64 = num.parse().context("relative date amount")?;
    let delta = match unit {
        'h' => Duration::hours(n),
        'd' => Duration::days(n),
        'w' => Duration::weeks(n),
        _ => unreachable!("filtered above"),
    };
    Ok(Some(Local::now() - delta))
}

fn local_from_naive(dt: NaiveDateTime) -> anyhow::Result<DateTime<Local>> {
    Local
        .from_local_datetime(&dt)
        .single()
        .context("ambiguous local time")
}

#[cfg(test)]
mod tests {
    use super::parse_date_arg;
    use chrono::{Datelike, Local, Timelike};

    #[test]
    fn absolute_date_parses_to_midnight() {
        let dt = parse_date_arg("2026-06-01", false).unwrap();
        assert_eq!((dt.year(), dt.month(), dt.day()), (2026, 6, 1));
        assert_eq!(dt.hour(), 0);
    }

    #[test]
    fn until_date_parses_to_end_of_day() {
        let dt = parse_date_arg("2026-06-01", true).unwrap();
        assert_eq!(dt.hour(), 23);
        assert_eq!(dt.minute(), 59);
    }

    #[test]
    fn datetime_parses() {
        let dt = parse_date_arg("2026-06-01T14:30", false).unwrap();
        assert_eq!((dt.hour(), dt.minute()), (14, 30));
    }

    #[test]
    fn relative_days_are_in_the_past() {
        let dt = parse_date_arg("7d", false).unwrap();
        assert!(dt < Local::now());
    }

    #[test]
    fn garbage_is_rejected() {
        assert!(parse_date_arg("yesterday", false).is_err());
        assert!(parse_date_arg("7x", false).is_err());
    }
}
