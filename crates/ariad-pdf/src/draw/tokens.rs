//! Expansion of page and date tokens in overlay text.

use ariad_core::tools::params::DateFormat;

use crate::{PdfError, ToolClock, ToolCtx};

/// Values substituted into a template. Dates are resolved only when requested.
#[derive(Clone, Debug, Default)]
pub struct TokenContext<'a> {
    /// Current displayed number.
    pub number: Option<u32>,
    /// Final displayed number.
    pub total: Option<u32>,
    /// Displayed page label.
    pub label: Option<&'a str>,
    /// Original input filename.
    pub filename: Option<&'a str>,
    /// Explicit ISO date, which takes precedence over other date sources.
    pub date: Option<&'a str>,
    /// Date formatting style.
    pub date_format: DateFormat,
}

/// Resolve a date without consulting the wall clock.
///
/// An explicit date takes precedence over `SOURCE_DATE_EPOCH`, which takes
/// precedence over the injected clock. Invalid supplied values are rejected.
pub fn resolve_date(explicit: Option<&str>, ctx: &ToolCtx) -> Result<String, PdfError> {
    if explicit.is_some() {
        return resolve_date_with_epoch(explicit, None, ctx.clock());
    }
    let epoch = match std::env::var("SOURCE_DATE_EPOCH") {
        Ok(value) => Some(value),
        Err(std::env::VarError::NotPresent) => None,
        Err(std::env::VarError::NotUnicode(_)) => return Err(invalid_date()),
    };
    resolve_date_with_epoch(None, epoch.as_deref(), ctx.clock())
}

fn invalid_date() -> PdfError {
    PdfError::InvalidParams("date must be a valid YYYY-MM-DD or an available clock date".into())
}

fn resolve_date_with_epoch(
    explicit: Option<&str>,
    source_epoch: Option<&str>,
    clock: Option<ToolClock>,
) -> Result<String, PdfError> {
    if let Some(date) = explicit {
        return validate_date(date);
    }
    let seconds = if let Some(raw) = source_epoch {
        raw.trim().parse::<i64>().map_err(|_| invalid_date())?
    } else if let Some(clock) = clock {
        clock
            .epoch_secs
            .checked_add(i64::from(clock.utc_offset_secs))
            .ok_or_else(invalid_date)?
    } else {
        return Err(PdfError::InvalidParams(
            "date is required when no clock is available".into(),
        ));
    };
    // Restrict to four-digit ISO years, and do not silently clamp invalid input.
    if !(-62_135_596_800..=253_402_300_799).contains(&seconds) {
        return Err(invalid_date());
    }
    let days = seconds.div_euclid(86_400);
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let year = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = year + i64::from(month <= 2);
    Ok(format!("{year:04}-{month:02}-{day:02}"))
}

fn validate_date(date: &str) -> Result<String, PdfError> {
    let b = date.as_bytes();
    if b.len() != 10
        || b[4] != b'-'
        || b[7] != b'-'
        || !b
            .iter()
            .enumerate()
            .all(|(i, c)| i == 4 || i == 7 || c.is_ascii_digit())
    {
        return Err(invalid_date());
    }
    let number = |range: std::ops::Range<usize>| -> u32 {
        b[range]
            .iter()
            .fold(0, |v, c| v * 10 + u32::from(*c - b'0'))
    };
    let year = number(0..4);
    let month = number(5..7);
    let day = number(8..10);
    let leap = year.is_multiple_of(4) && (!year.is_multiple_of(100) || year.is_multiple_of(400));
    let max_day = match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if leap => 29,
        2 => 28,
        _ => 0,
    };
    if year == 0 || day == 0 || day > max_day {
        return Err(invalid_date());
    }
    Ok(date.to_owned())
}

/// Expand known tokens and escaped braces; unknown or malformed tokens fail.
pub fn expand(
    template: &str,
    values: &TokenContext<'_>,
    ctx: &ToolCtx,
) -> Result<String, PdfError> {
    let mut out = String::new();
    let mut chars = template.chars().peekable();
    while let Some(ch) = chars.next() {
        match ch {
            '{' if chars.peek() == Some(&'{') => {
                chars.next();
                out.push('{');
            }
            '}' if chars.peek() == Some(&'}') => {
                chars.next();
                out.push('}');
            }
            '{' => {
                let mut name = String::new();
                loop {
                    match chars.next() {
                        Some('}') => break,
                        Some('{') | None => {
                            return Err(PdfError::InvalidParams("malformed template token".into()));
                        }
                        Some(c) => name.push(c),
                    }
                }
                let missing =
                    || PdfError::InvalidParams(format!("template token {{{name}}} is unavailable"));
                match name.as_str() {
                    "n" => out.push_str(&values.number.ok_or_else(missing)?.to_string()),
                    "total" => out.push_str(&values.total.ok_or_else(missing)?.to_string()),
                    "label" => out.push_str(values.label.ok_or_else(missing)?),
                    "filename" => out.push_str(values.filename.ok_or_else(missing)?),
                    "date" => {
                        let date = resolve_date(values.date, ctx)?;
                        match values.date_format {
                            DateFormat::Iso => out.push_str(&date),
                            DateFormat::Dmy => out.push_str(&format!(
                                "{}/{}/{}",
                                &date[8..10],
                                &date[5..7],
                                &date[..4]
                            )),
                            DateFormat::Mdy => out.push_str(&format!(
                                "{}/{}/{}",
                                &date[5..7],
                                &date[8..10],
                                &date[..4]
                            )),
                        }
                    }
                    _ => return Err(PdfError::InvalidParams("unknown template token".into())),
                }
            }
            '}' => return Err(PdfError::InvalidParams("malformed template token".into())),
            _ => out.push(ch),
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn date_sources_and_validation() {
        let clock = Some(ToolClock {
            epoch_secs: 0,
            utc_offset_secs: -3600,
        });
        assert_eq!(
            resolve_date_with_epoch(Some("2024-02-29"), Some("0"), clock).unwrap(),
            "2024-02-29"
        );
        assert_eq!(
            resolve_date_with_epoch(None, Some("0"), clock).unwrap(),
            "1970-01-01"
        );
        assert_eq!(
            resolve_date_with_epoch(None, None, clock).unwrap(),
            "1969-12-31"
        );
        assert!(resolve_date_with_epoch(Some("2023-02-29"), None, clock).is_err());
        assert!(resolve_date_with_epoch(None, Some("invalid"), clock).is_err());
        assert!(resolve_date_with_epoch(None, None, None).is_err());
        assert_eq!(
            resolve_date_with_epoch(None, Some("-62135596800"), None).unwrap(),
            "0001-01-01"
        );
        assert_eq!(
            resolve_date_with_epoch(None, Some("253402300799"), None).unwrap(),
            "9999-12-31"
        );
    }

    #[test]
    fn expands_tokens_and_rejects_bad_braces() {
        let values = TokenContext {
            number: Some(2),
            total: Some(9),
            label: Some("iv"),
            filename: Some("Báo cáo.pdf"),
            date: Some("2024-02-29"),
            date_format: DateFormat::Dmy,
        };
        assert_eq!(
            expand(
                "{{{n}}} {total} {label} {filename} {date}",
                &values,
                &ToolCtx::new()
            )
            .unwrap(),
            "{2} 9 iv Báo cáo.pdf 29/02/2024"
        );
        for text in ["{bad}", "{n", "n}", "{n{}}"] {
            assert!(expand(text, &values, &ToolCtx::new()).is_err());
        }
    }
}
