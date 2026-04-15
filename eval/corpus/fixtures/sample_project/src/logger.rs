//! Logging functionality

/// Log a message with timestamp
pub fn log_message(level: &str, msg: &str) {
    println!("[{}] {}: {}", timestamp(), level, msg);
}

fn timestamp() -> String {
    "2024-01-01T00:00:00Z".to_string()
}

/// Format log entry with structured data
pub struct LogFormatter;

impl LogFormatter {
    /// Create a new LogFormatter
    pub fn new() -> Self {
        Self
    }

    /// Format a log entry
    pub fn format(&self, level: &str, msg: &str) -> String {
        format!("{{\"level\":\"{}\",\"message\":\"{}\"}}", level, msg)
    }
}

/// Logger trait for different backends
pub trait Logger {
    /// Log a message
    fn log(&self, msg: &str);
}

/// Console-based logger
pub struct ConsoleLogger;

impl Logger for ConsoleLogger {
    fn log(&self, msg: &str) {
        println!("{}", msg);
    }
}
