//! Time windows and durations expressed the way Prometheus and the SRE
//! Workbook talk about them (`5m`, `1h`, `30d`).

use std::fmt;
use std::str::FromStr;
use std::time::Duration;

use crate::error::{Result, SlokitError};

const SECS_PER_MINUTE: u64 = 60;
const SECS_PER_HOUR: u64 = 60 * SECS_PER_MINUTE;
const SECS_PER_DAY: u64 = 24 * SECS_PER_HOUR;
const SECS_PER_WEEK: u64 = 7 * SECS_PER_DAY;

/// A non-zero span of time, stored as whole seconds.
///
/// `Window` round-trips with the Prometheus duration grammar used in range
/// selectors (`rate(...[5m])`) and renders back to the shortest exact unit, so
/// `Window::hours(1)` displays as `1h` and `Window::days(30)` as `30d`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Window {
    secs: u64,
}

impl Window {
    /// Build a window from a raw number of seconds.
    pub const fn from_secs(secs: u64) -> Self {
        Self { secs }
    }

    /// Build a window of `s` seconds.
    pub const fn seconds(s: u64) -> Self {
        Self::from_secs(s)
    }

    /// Build a window of `m` minutes.
    pub const fn minutes(m: u64) -> Self {
        Self::from_secs(m * SECS_PER_MINUTE)
    }

    /// Build a window of `h` hours.
    pub const fn hours(h: u64) -> Self {
        Self::from_secs(h * SECS_PER_HOUR)
    }

    /// Build a window of `d` days.
    pub const fn days(d: u64) -> Self {
        Self::from_secs(d * SECS_PER_DAY)
    }

    /// The window length in whole seconds.
    pub const fn as_secs(&self) -> u64 {
        self.secs
    }

    /// The window length in seconds as `f64`, for ratio math.
    pub fn as_secs_f64(&self) -> f64 {
        self.secs as f64
    }

    /// The window length expressed in days (may be fractional).
    pub fn as_days_f64(&self) -> f64 {
        self.secs as f64 / SECS_PER_DAY as f64
    }

    /// Convert to a [`std::time::Duration`].
    pub const fn as_duration(&self) -> Duration {
        Duration::from_secs(self.secs)
    }

    /// Parse a Prometheus-style duration such as `30s`, `5m`, `1h`, `3d`, `2w`.
    ///
    /// Compound durations (`1h30m`) are accepted and summed. A bare zero or an
    /// empty string is rejected, since a zero-length window is never useful.
    pub fn parse(input: &str) -> Result<Self> {
        let s = input.trim();
        if s.is_empty() {
            return Err(SlokitError::InvalidDuration(
                "empty duration string".to_string(),
            ));
        }

        let mut total: u64 = 0;
        let mut number = String::new();
        let mut saw_unit = false;

        for ch in s.chars() {
            if ch.is_ascii_digit() {
                number.push(ch);
                continue;
            }
            if number.is_empty() {
                return Err(SlokitError::InvalidDuration(format!(
                    "unit '{ch}' without a preceding number in '{input}'"
                )));
            }
            let value: u64 = number.parse().map_err(|_| {
                SlokitError::InvalidDuration(format!("number out of range in '{input}'"))
            })?;
            let unit_secs = match ch {
                's' => 1,
                'm' => SECS_PER_MINUTE,
                'h' => SECS_PER_HOUR,
                'd' => SECS_PER_DAY,
                'w' => SECS_PER_WEEK,
                other => {
                    return Err(SlokitError::InvalidDuration(format!(
                        "unknown unit '{other}' in '{input}' (expected s, m, h, d, w)"
                    )))
                }
            };
            total = total
                .checked_add(value.checked_mul(unit_secs).ok_or_else(|| {
                    SlokitError::InvalidDuration(format!("duration overflow in '{input}'"))
                })?)
                .ok_or_else(|| {
                    SlokitError::InvalidDuration(format!("duration overflow in '{input}'"))
                })?;
            number.clear();
            saw_unit = true;
        }

        if !number.is_empty() {
            return Err(SlokitError::InvalidDuration(format!(
                "trailing number without a unit in '{input}'"
            )));
        }
        if !saw_unit {
            return Err(SlokitError::InvalidDuration(format!(
                "no unit found in '{input}'"
            )));
        }
        if total == 0 {
            return Err(SlokitError::InvalidDuration(format!(
                "duration must be greater than zero: '{input}'"
            )));
        }

        Ok(Self::from_secs(total))
    }

    /// Render to the shortest exact Prometheus duration unit.
    ///
    /// Picks the largest unit (`d` > `h` > `m` > `s`) that divides the window
    /// evenly, matching how `sloth` names its windows.
    pub fn prometheus(&self) -> String {
        let s = self.secs;
        if s % SECS_PER_DAY == 0 {
            format!("{}d", s / SECS_PER_DAY)
        } else if s % SECS_PER_HOUR == 0 {
            format!("{}h", s / SECS_PER_HOUR)
        } else if s % SECS_PER_MINUTE == 0 {
            format!("{}m", s / SECS_PER_MINUTE)
        } else {
            format!("{s}s")
        }
    }
}

impl fmt::Display for Window {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.prometheus())
    }
}

impl FromStr for Window {
    type Err = SlokitError;

    fn from_str(s: &str) -> Result<Self> {
        Window::parse(s)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_single_units() {
        assert_eq!(Window::parse("30s").unwrap(), Window::seconds(30));
        assert_eq!(Window::parse("5m").unwrap(), Window::minutes(5));
        assert_eq!(Window::parse("1h").unwrap(), Window::hours(1));
        assert_eq!(Window::parse("3d").unwrap(), Window::days(3));
        assert_eq!(Window::parse("2w").unwrap(), Window::days(14));
    }

    #[test]
    fn parses_compound_durations() {
        assert_eq!(
            Window::parse("1h30m").unwrap(),
            Window::from_secs(90 * SECS_PER_MINUTE)
        );
    }

    #[test]
    fn rejects_bad_durations() {
        assert!(Window::parse("").is_err());
        assert!(Window::parse("10").is_err());
        assert!(Window::parse("h").is_err());
        assert!(Window::parse("5x").is_err());
        assert!(Window::parse("0s").is_err());
    }

    #[test]
    fn renders_shortest_unit() {
        assert_eq!(Window::days(30).prometheus(), "30d");
        assert_eq!(Window::hours(6).prometheus(), "6h");
        assert_eq!(Window::minutes(30).prometheus(), "30m");
        assert_eq!(Window::from_secs(90 * SECS_PER_MINUTE).prometheus(), "90m");
        assert_eq!(Window::seconds(45).prometheus(), "45s");
    }

    #[test]
    fn day_fraction_is_exact() {
        assert_eq!(Window::days(30).as_days_f64(), 30.0);
        assert_eq!(Window::hours(12).as_days_f64(), 0.5);
    }
}
