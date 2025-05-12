use std::time::{Duration, SystemTime, UNIX_EPOCH};

#[derive(Clone, Debug)]
pub struct Token {
    pub mint: String,
    pub name: String,
    pub usd_market_cap: f64,
}

pub fn calculate_time_difference(timestamp_ms: u64) -> Option<Duration> {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .ok()?
        .as_millis() as u64;

    if timestamp_ms > now {
        Some(Duration::from_millis(timestamp_ms - now))
    } else {
        Some(Duration::from_millis(now - timestamp_ms))
    }
}

#[macro_export]
macro_rules! debug_println {
    ($($arg:tt)*) => (#[cfg(debug_assertions)] println!($($arg)*));
}

#[macro_export]
macro_rules! debug_eprintln {
    ($($arg:tt)*) => (#[cfg(debug_assertions)] eprintln!($($arg)*));
}
