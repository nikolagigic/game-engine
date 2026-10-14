//! History tools (GE-022): `history.undo` and `history.redo`, on the
//! engine's [`History`] resource.

use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::json;

use super::History;
use crate::headless::Headless;
use crate::tool::{ToolOutput, ToolRegistry, TypedTool};

/// Registers `history.undo` and `history.redo`.
pub fn register_tools(reg: &mut ToolRegistry) {
    reg.register(TypedTool::new(
        "history.undo",
        "Undoes the most recent edit recorded in the command history. Returns whether \
         anything was undone (`ok`) and how many edits are left to undo (`remaining`).",
        history_undo,
    ));
    reg.register(TypedTool::new(
        "history.redo",
        "Redoes the most recently undone edit. Returns whether anything was redone (`ok`) \
         and how many undone edits are left to redo (`remaining`).",
        history_redo,
    ));
}

/// `history.undo` and `history.redo` take no arguments.
#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct NoArgs {}

fn history_undo(engine: &mut Headless, _: NoArgs) -> anyhow::Result<ToolOutput> {
    let ok = super::undo(engine)?;
    let remaining = engine
        .resources
        .get::<History>()
        .map_or(0, History::undo_len);
    Ok(ToolOutput::Json(
        json!({ "ok": ok, "remaining": remaining }),
    ))
}

fn history_redo(engine: &mut Headless, _: NoArgs) -> anyhow::Result<ToolOutput> {
    let ok = super::redo(engine)?;
    let remaining = engine
        .resources
        .get::<History>()
        .map_or(0, History::redo_len);
    Ok(ToolOutput::Json(
        json!({ "ok": ok, "remaining": remaining }),
    ))
}
