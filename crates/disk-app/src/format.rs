//! Human-readable formatting helpers.

/// Format a byte count the way the Finder does, using decimal (1000) units:
/// `"0 bytes"`, `"500 bytes"`, `"1.5 KB"`, `"2.5 GB"`.
pub fn human_size(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["KB", "MB", "GB", "TB", "PB"];
    if bytes < 1000 {
        return format!("{bytes} bytes");
    }
    let mut value = bytes as f64 / 1000.0;
    let mut unit = 0;
    while value >= 1000.0 && unit < UNITS.len() - 1 {
        value /= 1000.0;
        unit += 1;
    }
    format!("{value:.1} {}", UNITS[unit])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_bytes_below_one_kilobyte() {
        assert_eq!(human_size(0), "0 bytes");
        assert_eq!(human_size(500), "500 bytes");
        assert_eq!(human_size(999), "999 bytes");
    }

    #[test]
    fn formats_kilobytes() {
        assert_eq!(human_size(1_000), "1.0 KB");
        assert_eq!(human_size(1_500), "1.5 KB");
    }

    #[test]
    fn formats_larger_units() {
        assert_eq!(human_size(1_000_000), "1.0 MB");
        assert_eq!(human_size(2_500_000_000), "2.5 GB");
        assert_eq!(human_size(1_000_000_000_000), "1.0 TB");
    }
}
