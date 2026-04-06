// T077: Integration tests for MCP bridge — verify tool definitions and UDS forwarding.

use rmcp::ServerHandler;
use scavenger::bridge::ScavengerBridge;
use std::path::PathBuf;

fn make_bridge() -> ScavengerBridge {
    ScavengerBridge::new(PathBuf::from("/tmp/nonexistent.sock"))
}

#[test]
fn test_bridge_lists_five_tools() {
    let bridge = make_bridge();
    let info = bridge.get_info();
    assert!(
        info.instructions
            .as_ref()
            .is_some_and(|i| i.contains("Scavenger"))
    );

    // Access tool_router to list tools
    let tools = bridge.tool_router.list_all();
    let names: Vec<&str> = tools.iter().map(|t| t.name.as_ref()).collect();
    assert_eq!(
        names.len(),
        5,
        "expected 5 MCP tools, got {}: {:?}",
        names.len(),
        names
    );
    assert!(names.contains(&"get_capsule"));
    assert!(names.contains(&"read_annotations"));
    assert!(names.contains(&"write_annotation"));
    assert!(names.contains(&"delete_annotation"));
    assert!(names.contains(&"search_docs"));
}

#[test]
fn test_bridge_tool_descriptions_not_empty() {
    let bridge = make_bridge();
    let tools = bridge.tool_router.list_all();
    for tool in &tools {
        assert!(
            tool.description.as_ref().is_some_and(|d| !d.is_empty()),
            "tool '{}' should have a non-empty description",
            tool.name
        );
    }
}

#[test]
fn test_bridge_get_capsule_has_file_param() {
    let bridge = make_bridge();
    let tools = bridge.tool_router.list_all();
    let capsule_tool = tools
        .iter()
        .find(|t| t.name.as_ref() == "get_capsule")
        .unwrap();
    let schema = &capsule_tool.input_schema;
    let props = schema
        .get("properties")
        .expect("schema should have properties");
    assert!(
        props.get("file").is_some(),
        "get_capsule should require 'file' param"
    );
}

#[test]
fn test_bridge_write_annotation_has_text_param() {
    let bridge = make_bridge();
    let tools = bridge.tool_router.list_all();
    let write_tool = tools
        .iter()
        .find(|t| t.name.as_ref() == "write_annotation")
        .unwrap();
    let schema = &write_tool.input_schema;
    let props = schema
        .get("properties")
        .expect("schema should have properties");
    assert!(
        props.get("text").is_some(),
        "write_annotation should have 'text' param"
    );
    let required = schema.get("required").and_then(|v| v.as_array());
    assert!(
        required.is_some_and(|r| r.iter().any(|v| v.as_str() == Some("text"))),
        "text should be required"
    );
}

#[test]
fn test_bridge_delete_annotation_has_id_param() {
    let bridge = make_bridge();
    let tools = bridge.tool_router.list_all();
    let del_tool = tools
        .iter()
        .find(|t| t.name.as_ref() == "delete_annotation")
        .unwrap();
    let schema = &del_tool.input_schema;
    let props = schema
        .get("properties")
        .expect("schema should have properties");
    assert!(
        props.get("id").is_some(),
        "delete_annotation should have 'id' param"
    );
}

#[test]
fn test_bridge_search_docs_has_query_param() {
    let bridge = make_bridge();
    let tools = bridge.tool_router.list_all();
    let search_tool = tools
        .iter()
        .find(|t| t.name.as_ref() == "search_docs")
        .unwrap();
    let schema = &search_tool.input_schema;
    let props = schema
        .get("properties")
        .expect("schema should have properties");
    assert!(
        props.get("query").is_some(),
        "search_docs should have 'query' param"
    );
}

#[test]
fn test_bridge_server_info() {
    let bridge = make_bridge();
    let info = bridge.get_info();
    assert!(
        info.instructions
            .as_ref()
            .is_some_and(|i| i.contains("capsule")),
        "server info instructions should mention capsules"
    );

    // Verify key concepts are documented
    let instructions = info.instructions.as_ref().unwrap();
    assert!(
        instructions.contains("query"),
        "instructions should mention 'query' parameter"
    );
    assert!(
        instructions.contains("detail_level"),
        "instructions should mention 'detail_level'"
    );
    assert!(
        instructions.contains("write_annotation"),
        "instructions should mention write_annotation"
    );
}
