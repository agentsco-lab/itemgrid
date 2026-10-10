//! Times as item/grid writes them: local "YYYY-MM-DD HH:MM:SS" in the
//! manifests and the flash state (shared with the port's flash-safely.sh),
//! and file names' "YYYY-MM-DD-HHMMSS". Worked out here, not by date(1).

use chrono::{DateTime, Local, NaiveDateTime, TimeZone, Utc};

/// Now, local, "YYYY-MM-DD HH:MM:SS".
pub fn now() -> String {
    Local::now().format("%Y-%m-%d %H:%M:%S").to_string()
}

/// Now, local, for a file name: "YYYY-MM-DD-HHMMSS".
pub fn file_stamp() -> String {
    Local::now().format("%Y-%m-%d-%H%M%S").to_string()
}

/// A local "YYYY-MM-DD HH:MM:SS" as UTC seconds; 0 if it is not one.
pub fn local_secs(stamp: &str) -> i64 {
    NaiveDateTime::parse_from_str(stamp.trim(), "%Y-%m-%d %H:%M:%S")
        .ok()
        .and_then(|t| Local.from_local_datetime(&t).earliest())
        .map(|t| t.timestamp())
        .unwrap_or(0)
}

/// UTC seconds in words: "YYYY-MM-DD HH:MM UTC".
pub fn when_utc(utc: i64) -> String {
    DateTime::<Utc>::from_timestamp(utc, 0).map(|t| t.format("%Y-%m-%d %H:%M UTC").to_string()).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stamps_round_trip() {
        let s = now();
        assert_eq!(s.len(), 19);
        assert!(local_secs(&s) > 1_700_000_000);
        assert_eq!(local_secs("not a time"), 0);
        assert_eq!(when_utc(0), "1970-01-01 00:00 UTC");
    }
}
