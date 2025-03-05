use std::time::{SystemTime, UNIX_EPOCH};

use crate::ROUND_SIZE_MS;

/// Returns the current time in multiples of 100milliseconds since the UNIX epoch
pub fn get_current_round() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("Time went backwards")
        .as_millis() as u64 / ROUND_SIZE_MS
}
