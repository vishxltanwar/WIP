//! Simple file-based logging for Rice Studio (no console window in release builds)

use std::fs::OpenOptions;
use std::io::Write;
use std::sync::Mutex;

static LOGGER: Mutex<Option<std::fs::File>> = Mutex::new(None);

pub fn init() {
    let path = crate::apply::data_dir().join("rice-studio.log");
    if let Ok(f) = OpenOptions::new().create(true).append(true).open(&path) {
        *LOGGER.lock().unwrap() = Some(f);
        info("Rice Studio started");
    }
}

pub fn info(msg: &str) {
    write("INFO", msg);
}

#[allow(dead_code)]
pub fn warn(msg: &str) {
    write("WARN", msg);
}

#[allow(dead_code)]
pub fn error(msg: &str) {
    write("ERROR", msg);
}

fn write(level: &str, msg: &str) {
    if let Ok(mut guard) = LOGGER.lock() {
        if let Some(f) = guard.as_mut() {
            let ts = chrono_free_timestamp();
            let _ = writeln!(f, "[{}] {} {}", ts, level, msg);
            let _ = f.flush();
        }
    }
}

fn chrono_free_timestamp() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let secs = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
    // Simple UTC-ish timestamp
    format!("{}", secs)
}