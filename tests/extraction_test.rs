// T062: Unit tests for tree-sitter extraction — all 15 languages.

use scavenger::graph::index::parse_file;
use scavenger::graph::types::NodeKind;
use std::path::Path;

fn assert_extracts(ext: &str, src: &str, min_symbols: usize) {
    let path_str = format!("test.{ext}");
    let result = parse_file(Path::new(&path_str), src).unwrap();
    assert!(
        result.symbols.len() >= min_symbols,
        "{ext}: expected >= {min_symbols} symbols, got {} → {:?}",
        result.symbols.len(),
        result.symbols.iter().map(|s| &s.name).collect::<Vec<_>>()
    );
}

#[test]
fn test_rust_extraction() {
    assert_extracts(
        "rs",
        "fn hello() {}\nstruct Point { x: f64, y: f64 }\nenum Color { Red, Blue }",
        3,
    );
}

#[test]
fn test_python_extraction() {
    assert_extracts(
        "py",
        "def greet(name):\n    pass\n\nclass Animal:\n    def speak(self):\n        pass",
        2,
    );
}

#[test]
fn test_typescript_extraction() {
    assert_extracts(
        "ts",
        "interface Shape { area(): number }\nfunction draw(s: Shape) {}\nclass Circle implements Shape { area() { return 0; } }",
        3,
    );
}

#[test]
fn test_javascript_extraction() {
    assert_extracts(
        "js",
        "function add(a, b) { return a + b; }\nclass Calculator { multiply(a, b) { return a * b; } }",
        2,
    );
}

#[test]
fn test_go_extraction() {
    assert_extracts(
        "go",
        "package main\ntype Point struct { X float64; Y float64 }\nfunc NewPoint(x, y float64) Point { return Point{x, y} }",
        2,
    );
}

#[test]
fn test_java_extraction() {
    assert_extracts(
        "java",
        "class Greeter { public void greet(String name) {} }\nenum Day { MON, TUE }",
        2,
    );
}

#[test]
fn test_cpp_extraction() {
    assert_extracts(
        "cpp",
        "class Vector { public: int x, y; };\nvoid draw(Vector v) {}",
        2,
    );
}

#[test]
fn test_c_extraction() {
    assert_extracts(
        "c",
        "struct Point { int x; int y; };\nvoid draw(struct Point p) {}",
        2,
    );
}

#[test]
fn test_ruby_extraction() {
    assert_extracts(
        "rb",
        "class Dog\n  def bark\n    puts 'woof'\n  end\nend\ndef greet(name)\n  puts name\nend",
        2,
    );
}

#[test]
fn test_php_extraction() {
    assert_extracts(
        "php",
        "<?php\nclass User { public function getName() { return ''; } }\nfunction hello() {}",
        2,
    );
}

#[test]
fn test_csharp_extraction() {
    assert_extracts(
        "cs",
        "class Widget { void Render() {} }\nenum Size { Small, Large }",
        2,
    );
}

#[test]
fn test_swift_extraction() {
    assert_extracts(
        "swift",
        "class Vehicle { func drive() {} }\nfunc park(v: Vehicle) {}",
        2,
    );
}

#[test]
fn test_kotlin_returns_none() {
    let result = parse_file(Path::new("test.kt"), "class Person(val name: String)");
    assert!(result.is_none(), "kotlin not yet supported");
}

#[test]
fn test_scala_returns_none() {
    let result = parse_file(Path::new("test.scala"), "class Animal {}");
    assert!(result.is_none(), "scala not yet supported");
}

#[test]
fn test_hcl_returns_none() {
    let result = parse_file(Path::new("test.tf"), "resource \"aws_instance\" \"web\" {}");
    assert!(result.is_none(), "hcl not yet supported");
}

#[test]
fn test_empty_file_produces_no_symbols() {
    let result = parse_file(Path::new("empty.rs"), "").unwrap();
    assert_eq!(result.symbols.len(), 0);
}

#[test]
fn test_symbols_have_correct_kind() {
    let src = "fn hello() {}\nstruct Point { x: f64 }\nenum Color { Red }";
    let result = parse_file(Path::new("test.rs"), src).unwrap();

    for sym in &result.symbols {
        match sym.name.as_str() {
            "hello" => assert_eq!(sym.kind, NodeKind::Function),
            "Point" => assert!(matches!(sym.kind, NodeKind::Class | NodeKind::Type)),
            "Color" => assert_eq!(sym.kind, NodeKind::Enum),
            _ => {}
        }
    }
}

#[test]
fn test_signature_hash_stable() {
    let src = "fn stable_fn(x: i32) -> bool { true }";
    let r1 = parse_file(Path::new("test.rs"), src).unwrap();
    let r2 = parse_file(Path::new("test.rs"), src).unwrap();
    assert_eq!(r1.symbols[0].signature_hash, r2.symbols[0].signature_hash);
}
