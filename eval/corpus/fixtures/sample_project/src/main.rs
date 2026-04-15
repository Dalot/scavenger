//! Sample project for eval testing
//!
//! This project has G1/G2/G3 dependency relationships for testing
//! the scavenger eval system.

mod config;
mod http;
mod logger;
mod middleware;
mod utils;

use config::{parse_config, validate_config};
use http::{HttpServer, route_handler};
use middleware::logging::RequestContext;
use utils::{DataProcessor, parse_input};

/// Application entry point
fn main() {
    let config = parse_config();

    if let Err(e) = validate_config(&config) {
        eprintln!("Config error: {}", e);
        std::process::exit(1);
    }

    let server = HttpServer::new(config);
    server.start();

    // Example usage
    let processor = DataProcessor::new();
    let result = processor.process("hello world");
    println!("Processed: {}", result);

    // Handle a request
    let ctx = RequestContext::new("/api/test", "GET");
    let response = server.handle_request(ctx);
    println!("Response: {} - {}", response.status, response.body);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_input() {
        assert_eq!(parse_input("test").unwrap(), "test");
    }

    #[test]
    fn test_route_handler() {
        let response = route_handler("/health").unwrap();
        assert_eq!(response.status, 200);
    }
}
