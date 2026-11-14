//! The configuration library of tool settings (M31b): save, replace,
//! delete, and share as a file.

#![allow(clippy::unwrap_used)] // test helpers: panicking is the failure report

use chosproxy_app::{Engine, ToolConfig};
use chosproxy_core::paths::AppPaths;

fn config(tool: &str, name: &str, json: &str) -> ToolConfig {
    ToolConfig {
        tool: tool.into(),
        name: name.into(),
        json: json.into(),
    }
}

#[test]
fn settings_are_saved_per_tool_and_shared_as_files() {
    let dir = tempfile::tempdir().unwrap();
    let engine = Engine::new(AppPaths::under(dir.path())).unwrap();
    assert!(engine.tool_configs("crawler").is_empty());
    engine
        .save_tool_config(config("crawler", " Slow ", r#"{"requestsPerSecond":2}"#))
        .unwrap();
    engine
        .save_tool_config(config("fuzzer", "Slow", r#"{"concurrency":1}"#))
        .unwrap();
    // The same name again replaces it (names ignore case).
    let list = engine
        .save_tool_config(config("crawler", "slow", r#"{"requestsPerSecond":1}"#))
        .unwrap();
    assert_eq!(list.len(), 1);
    assert_eq!(list[0].json, r#"{"requestsPerSecond":1}"#);
    assert_eq!(engine.tool_configs("fuzzer").len(), 1);

    // What is not a configuration is refused.
    for bad in [
        config("repeater", "x", "{}"),
        config("crawler", "", "{}"),
        config("crawler", "x", "[1]"),
        config("crawler", "x", "not json"),
    ] {
        assert!(engine.save_tool_config(bad).is_err());
    }

    // Shared as a file: only that tool's, and names in use get a suffix.
    let file = dir.path().join("crawler.json");
    assert_eq!(engine.export_tool_configs(&file, "crawler").unwrap(), 1);
    let text = std::fs::read_to_string(&file).unwrap();
    assert!(text.contains("chosproxy-tool-configurations") && !text.contains("concurrency"));
    let list = engine.import_tool_configs(&file, "crawler").unwrap();
    let names: Vec<&str> = list.iter().map(|c| c.name.as_str()).collect();
    assert_eq!(names, ["slow", "slow (2)"]);
    assert!(engine.import_tool_configs(&file, "fuzzer").is_err());
    std::fs::write(&file, "{}").unwrap();
    assert!(engine.import_tool_configs(&file, "crawler").is_err());

    assert_eq!(
        engine.delete_tool_config("crawler", "SLOW").unwrap().len(),
        1
    );
    assert!(engine.delete_tool_config("crawler", "nope").is_err());
    // It survives a restart.
    let again = Engine::new(AppPaths::under(dir.path())).unwrap();
    assert_eq!(again.tool_configs("crawler")[0].name, "slow (2)");
}
