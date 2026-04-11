use serde::Serialize;

#[allow(dead_code)]
#[derive(Debug, Clone, Serialize)]
pub struct Task {
    pub name: String,
    pub description: String,
    pub expected_files: Vec<String>,
}
