//! GE-013: the tool registry, its built-ins, and its argument errors.

use serde_json::{Value, json};
use shin::headless::Headless;
use shin::tool::{ToolError, ToolOutput, ToolRegistry, TypedTool};

fn json_output(output: ToolOutput) -> Value {
    match output {
        ToolOutput::Json(value) => value,
        other => panic!("expected JSON output, got {other:?}"),
    }
}

#[test]
fn builtins_are_listed_with_object_schemas() {
    let tools = ToolRegistry::with_builtins().list();
    for name in ["engine.info", "engine.list_tools"] {
        let info = tools
            .iter()
            .find(|t| t.name == name)
            .unwrap_or_else(|| panic!("`{name}` not listed"));
        assert!(!info.description.is_empty());
        let schema = info.input_schema.as_object().expect("schema is an object");
        assert!(!schema.is_empty(), "`{name}` schema is empty");
        assert_eq!(schema.get("type"), Some(&json!("object")), "`{name}`");
    }
}

#[test]
fn engine_info_reports_an_empty_world() -> anyhow::Result<()> {
    let registry = ToolRegistry::with_builtins();
    let mut engine = Headless::new(32, 16)?;
    let info = json_output(registry.call(&mut engine, "engine.info", json!({}))?);
    eprintln!("engine.info: {info}");
    assert_eq!(info["entities"], 0);
    assert_eq!(info["target"], json!({ "width": 32, "height": 16 }));
    assert_eq!(info["fixed_dt"], 1.0 / 60.0);
    assert_eq!(info["version"], env!("CARGO_PKG_VERSION"));
    assert!(info["adapter_name"].is_string());
    assert!(info["backend"].is_string());

    // `null` arguments mean "no arguments".
    registry.call(&mut engine, "engine.info", Value::Null)?;
    Ok(())
}

#[test]
fn list_tools_includes_later_registrations() -> anyhow::Result<()> {
    let mut registry = ToolRegistry::with_builtins();
    registry.register(TypedTool::new(
        "test.noop",
        "Does nothing.",
        |_: &mut Headless, _: serde_json::Map<String, Value>| Ok(ToolOutput::Text(String::new())),
    ));
    let mut engine = Headless::new(4, 4)?;
    let listed = json_output(registry.call(&mut engine, "engine.list_tools", json!({}))?);
    let names: Vec<_> = listed
        .as_array()
        .expect("array")
        .iter()
        .map(|t| t["name"].as_str().expect("name"))
        .collect();
    assert_eq!(names, ["engine.info", "engine.list_tools", "test.noop"]);
    assert!(listed[0]["input_schema"].is_object());
    Ok(())
}

#[test]
fn unknown_tool_is_reported() -> anyhow::Result<()> {
    let mut engine = Headless::new(4, 4)?;
    let err = ToolRegistry::with_builtins()
        .call(&mut engine, "engine.nope", json!({}))
        .unwrap_err();
    assert!(
        matches!(&err, ToolError::Unknown(name) if name == "engine.nope"),
        "{err:?}"
    );
    Ok(())
}

#[derive(serde::Deserialize, schemars::JsonSchema)]
struct StepArgs {
    /// Fixed steps to run.
    steps: u32,
}

#[test]
fn wrong_argument_type_names_the_field() -> anyhow::Result<()> {
    let mut registry = ToolRegistry::with_builtins();
    registry.register(TypedTool::new(
        "sim.step",
        "Runs fixed steps.",
        |engine: &mut Headless, args: StepArgs| {
            engine.step(args.steps);
            Ok(ToolOutput::Text(format!("ran {}", args.steps)))
        },
    ));
    let mut engine = Headless::new(4, 4)?;

    let err = registry
        .call(&mut engine, "sim.step", json!({ "steps": "three" }))
        .unwrap_err();
    eprintln!("{err}");
    match &err {
        ToolError::InvalidArgs { tool, message } => {
            assert_eq!(tool, "sim.step");
            assert!(message.contains("`steps`"), "{message}");
        }
        other => panic!("expected InvalidArgs, got {other:?}"),
    }

    // A missing field is also InvalidArgs and also named (by serde).
    let err = registry
        .call(&mut engine, "sim.step", json!({}))
        .unwrap_err();
    assert!(
        matches!(&err, ToolError::InvalidArgs { message, .. } if message.contains("steps")),
        "{err:?}"
    );

    // Built-ins take no arguments and reject unknown ones.
    let err = registry
        .call(&mut engine, "engine.info", json!({ "verbose": true }))
        .unwrap_err();
    assert!(
        matches!(&err, ToolError::InvalidArgs { message, .. } if message.contains("verbose")),
        "{err:?}"
    );

    // Valid arguments go through.
    registry.call(&mut engine, "sim.step", json!({ "steps": 2 }))?;
    assert_eq!(engine.resources.expect::<shin::ecs::Time>().fixed_steps, 2);
    Ok(())
}
