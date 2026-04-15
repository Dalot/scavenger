//! HTTP server functionality

use crate::config::Config;
use crate::middleware::logging::{LoggingMiddleware, RequestContext};

/// HTTP server handler
pub struct HttpServer {
    config: Config,
    logging_middleware: LoggingMiddleware,
}

impl HttpServer {
    /// Create a new HttpServer with the given config
    pub fn new(config: Config) -> Self {
        Self {
            config,
            logging_middleware: LoggingMiddleware::new(),
        }
    }

    /// Handle incoming HTTP request
    pub fn handle_request(&self, ctx: RequestContext) -> Response {
        self.logging_middleware.process_request(&ctx.path);

        Response {
            status: 200,
            body: format!("Handled {} {}", ctx.method, ctx.path),
        }
    }

    /// Start the server
    pub fn start(&self) {
        println!("Server starting on port {}", self.config.port);
    }
}

/// HTTP response structure
pub struct Response {
    pub status: u16,
    pub body: String,
}

/// Route handler for specific paths
pub fn route_handler(path: &str) -> Option<Response> {
    match path {
        "/" => Some(Response {
            status: 200,
            body: "Welcome".to_string(),
        }),
        "/health" => Some(Response {
            status: 200,
            body: "OK".to_string(),
        }),
        _ => None,
    }
}
