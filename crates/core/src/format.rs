//! Human-readable formatting shared by the CLI and the app.

const UNITS: [&str; 6] = ["KB", "MB", "GB", "TB", "PB", "EB"];

/// Formats a byte count with binary units and one decimal, for example
/// `4.2 GB`. Values under 1 KB are shown in bytes.
pub fn format_size(bytes: u64) -> String {
    if bytes < 1024 {
        return format!("{bytes} B");
    }
    let mut value = bytes as f64 / 1024.0;
    let mut unit = 0;
    while value >= 1023.95 && unit < UNITS.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }
    format!("{value:.1} {}", UNITS[unit])
}

/// `part` as a percentage of `whole` (0 when `whole` is 0).
pub fn percent(part: u64, whole: u64) -> f64 {
    if whole == 0 {
        0.0
    } else {
        part as f64 * 100.0 / whole as f64
    }
}

/// Converts a Windows FILETIME (100 ns ticks since 1601) to Unix seconds.
pub fn filetime_to_unix(ft: u64) -> i64 {
    const EPOCH_DIFF: i64 = 11_644_473_600;
    (ft / 10_000_000) as i64 - EPOCH_DIFF
}

/// Converts Unix seconds to a UTC civil date-time
/// `(year, month, day, hour, minute, second)`.
pub fn unix_to_utc(secs: i64) -> (i64, u32, u32, u32, u32, u32) {
    let days = secs.div_euclid(86_400);
    let rem = secs.rem_euclid(86_400);
    // Howard Hinnant's days-to-civil algorithm.
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let y = yoe + era * 400 + i64::from(m <= 2);
    (
        y,
        m,
        d,
        (rem / 3600) as u32,
        (rem % 3600 / 60) as u32,
        (rem % 60) as u32,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sizes_use_binary_units_with_one_decimal() {
        assert_eq!(format_size(0), "0 B");
        assert_eq!(format_size(1023), "1023 B");
        assert_eq!(format_size(1024), "1.0 KB");
        assert_eq!(format_size(1536), "1.5 KB");
        assert_eq!(format_size(4_509_715_660), "4.2 GB");
        assert_eq!(format_size(1024 * 1024 - 1), "1.0 MB");
        assert_eq!(format_size(u64::MAX), "16.0 EB");
    }

    #[test]
    fn time_conversions() {
        assert_eq!(filetime_to_unix(116_444_736_000_000_000), 0);
        assert_eq!(unix_to_utc(0), (1970, 1, 1, 0, 0, 0));
        assert_eq!(unix_to_utc(1_792_108_800), (2026, 10, 16, 0, 0, 0));
        assert_eq!(unix_to_utc(951_782_400 + 3661), (2000, 2, 29, 1, 1, 1));
    }

    #[test]
    fn percent_handles_zero() {
        assert_eq!(percent(5, 0), 0.0);
        assert_eq!(percent(1, 4), 25.0);
    }
}
