//! HTTP middleware for request handling

use crate::logger::{LogFormatter, log_message};

/// Middleware for handling HTTP request logging
pub struct LoggingMiddleware {
    formatter: LogFormatter,
}

impl LoggingMiddleware {
    /// Create a new LoggingMiddleware
    pub fn new() -> Self {
        Self {
            formatter: LogFormatter::new(),
        }
    }

    /// Process an incoming request through the middleware chain
    pub fn process_request(&self, request_path: &str) {
        let formatted = self
            .formatter
            .format("INFO", &format!("Request: {}", request_path));
        log_message("INFO", &formatted);
    }
}

/// Request context for middleware processing
pub struct RequestContext {
    pub path: String,
    pub method: String,
}

impl RequestContext {
    /// Create a new RequestContext
    pub fn new(path: &str, method: &str) -> Self {
        Self {
            path: path.to_string(),
            method: method.to_string(),
        }
    }
}
