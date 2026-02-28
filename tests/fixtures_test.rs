// T059: Verify fixture files parse correctly with known symbol counts.

use std::path::Path;

#[test]
fn test_rust_fixture_symbols() {
    let src = std::fs::read_to_string("tests/fixtures/sample_project/src/main.rs").unwrap();
    let result = scavenger::graph::index::parse_file(Path::new("main.rs"), &src).unwrap();
    let names: Vec<&str> = result.symbols.iter().map(|s| s.name.as_str()).collect();
    assert!(names.contains(&"main"), "missing main, found: {names:?}");
    assert!(names.contains(&"load_config"), "missing load_config");
    assert!(names.contains(&"Config"), "missing Config");
    assert!(names.contains(&"Server"), "missing Server");
    assert!(names.contains(&"Status"), "missing Status");
    assert!(result.symbols.len() >= 5, "expected >=5 symbols, got {}", result.symbols.len());
}

#[test]
fn test_python_fixture_symbols() {
    let src = std::fs::read_to_string("tests/fixtures/sample_project/src/utils.py").unwrap();
    let result = scavenger::graph::index::parse_file(Path::new("utils.py"), &src).unwrap();
    let names: Vec<&str> = result.symbols.iter().map(|s| s.name.as_str()).collect();
    assert!(names.contains(&"parse_input"), "missing parse_input, found: {names:?}");
    assert!(names.contains(&"validate_email"), "missing validate_email");
    assert!(names.contains(&"DataProcessor"), "missing DataProcessor");
    assert!(result.symbols.len() >= 3, "expected >=3 symbols, got {}", result.symbols.len());
}

#[test]
fn test_typescript_fixture_symbols() {
    let src = std::fs::read_to_string("tests/fixtures/sample_project/src/api.ts").unwrap();
    let result = scavenger::graph::index::parse_file(Path::new("api.ts"), &src).unwrap();
    let names: Vec<&str> = result.symbols.iter().map(|s| s.name.as_str()).collect();
    assert!(names.contains(&"User"), "missing User, found: {names:?}");
    assert!(names.contains(&"fetchUser"), "missing fetchUser");
    assert!(names.contains(&"UserService"), "missing UserService");
    assert!(names.contains(&"Role"), "missing Role");
    assert!(result.symbols.len() >= 4, "expected >=4 symbols, got {}", result.symbols.len());
}

#[test]
fn test_go_fixture_symbols() {
    let src = std::fs::read_to_string("tests/fixtures/sample_project/src/handler.go").unwrap();
    let result = scavenger::graph::index::parse_file(Path::new("handler.go"), &src).unwrap();
    let names: Vec<&str> = result.symbols.iter().map(|s| s.name.as_str()).collect();
    assert!(names.contains(&"Handler"), "missing Handler struct, found: {names:?}");
    assert!(names.contains(&"NewHandler"), "missing NewHandler");
    assert!(names.contains(&"ProcessRequest"), "missing ProcessRequest");
    assert!(names.contains(&"Router"), "missing Router");
    assert!(result.symbols.len() >= 4, "expected >=4 symbols, got {}", result.symbols.len());
}

#[test]
fn test_java_fixture_symbols() {
    let src = std::fs::read_to_string("tests/fixtures/sample_project/src/Service.java").unwrap();
    let result = scavenger::graph::index::parse_file(Path::new("Service.java"), &src).unwrap();
    let names: Vec<&str> = result.symbols.iter().map(|s| s.name.as_str()).collect();
    assert!(names.contains(&"ServiceBase"), "missing ServiceBase, found: {names:?}");
    assert!(names.contains(&"UserService"), "missing UserService");
    assert!(names.contains(&"Permission"), "missing Permission");
    assert!(result.symbols.len() >= 3, "expected >=3 symbols, got {}", result.symbols.len());
}
