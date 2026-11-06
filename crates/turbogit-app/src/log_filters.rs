//! The Commits pane's filter bar. IntelliJ's Git Log names are kept where they
//! name the same thing, so someone moving between the two tools does not have to
//! learn a second vocabulary.

pub const LABEL_ALL: &str = "All";
pub const LABEL_SELECT: &str = "Select\u{2026}";
pub const LABEL_BRANCH: &str = "Branch";
pub const LABEL_USER: &str = "User";
pub const LABEL_DATE: &str = "Date";
pub const LABEL_PATHS: &str = "Paths";
pub const LABEL_LAST_DAY: &str = "Last 24 hours";
pub const LABEL_LAST_WEEK: &str = "Last 7 days";
pub const LABEL_SELECT_PERIOD: &str = "Select Period";
pub const LABEL_ROOTS: &str = "Roots";
pub const LABEL_AFTER: &str = "After";
pub const LABEL_BEFORE: &str = "Before";

/// A picked period carries its own timestamps. Resolving "Last 24 hours" at
/// click time keeps the cache key stable across frames and keeps reads pure.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum DateFilter {
    #[default]
    All,
    Last24Hours {
        since: i64,
    },
    Last7Days {
        since: i64,
    },
    Period {
        after: i64,
        before: i64,
    },
}

impl DateFilter {
    pub fn is_all(self) -> bool {
        matches!(self, Self::All)
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::All => LABEL_ALL,
            Self::Last24Hours { .. } => LABEL_LAST_DAY,
            Self::Last7Days { .. } => LABEL_LAST_WEEK,
            Self::Period { .. } => LABEL_SELECT_PERIOD,
        }
    }

    pub fn bounds(self) -> (Option<i64>, Option<i64>) {
        match self {
            Self::All => (None, None),
            Self::Last24Hours { since } | Self::Last7Days { since } => (Some(since), None),
            Self::Period { after, before } => (Some(after), Some(before)),
        }
    }
}

/// Parse one `YYYY-MM-DD` bound into unix seconds at the UTC start of that day.
/// A blank bound means no bound, which is how IntelliJ's Select Period dialog
/// lets you ask only for "after" or only for "until". Malformed text is an
/// error, not a blank, so the popup can say which field is wrong.
pub fn parse_day(text: &str) -> Result<Option<i64>, &'static str> {
    let text = text.trim();
    if text.is_empty() {
        return Ok(None);
    }
    let date =
        chrono::NaiveDate::parse_from_str(text, "%Y-%m-%d").map_err(|_| "expected YYYY-MM-DD")?;
    Ok(Some(
        date.and_hms_opt(0, 0, 0)
            .ok_or("expected YYYY-MM-DD")?
            .and_utc()
            .timestamp(),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_blank_bound_is_open_and_a_bad_one_is_an_error() {
        assert_eq!(parse_day(""), Ok(None));
        assert_eq!(parse_day("   "), Ok(None));
        assert!(parse_day("06/11/2026").is_err());
    }

    #[test]
    fn a_day_is_read_at_utc_midnight() {
        assert_eq!(
            parse_day("2026-11-06"),
            Ok(Some(
                chrono::NaiveDate::from_ymd_opt(2026, 11, 6)
                    .unwrap()
                    .and_hms_opt(0, 0, 0)
                    .unwrap()
                    .and_utc()
                    .timestamp()
            ))
        );
    }

    #[test]
    fn each_preset_carries_the_bound_git_will_read() {
        assert_eq!(DateFilter::All.bounds(), (None, None));
        assert_eq!(
            DateFilter::Last24Hours { since: 10 }.bounds(),
            (Some(10), None)
        );
        assert_eq!(
            DateFilter::Period {
                after: 10,
                before: 20
            }
            .bounds(),
            (Some(10), Some(20))
        );
    }
}
