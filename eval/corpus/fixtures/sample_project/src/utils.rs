//! Utility functions for the application

/// Parse input string into structured data
pub fn parse_input(input: &str) -> Result<String, String> {
    if input.is_empty() {
        return Err("Empty input".to_string());
    }
    Ok(input.trim().to_string())
}

/// Validate email format
pub fn validate_email(email: &str) -> bool {
    email.contains('@') && email.contains('.')
}

/// Helper function for data transformation
pub fn helper<T, F>(data: T, transform: F) -> T
where
    F: FnOnce(T) -> T,
{
    transform(data)
}

/// Data processing functionality
pub struct DataProcessor;

impl DataProcessor {
    /// Create a new DataProcessor
    pub fn new() -> Self {
        Self
    }

    /// Process data and return transformed result
    pub fn process(&self, data: &str) -> String {
        data.to_uppercase()
    }
}
