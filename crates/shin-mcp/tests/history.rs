//! GE-027 (from GE-022): tool calls over MCP go through the command
//! history, so `history.undo` reverts an `entity.spawn` made by an agent.

mod common;

use serde_json::{Value, json};

use common::Server;

/// The number of entities `entity.list` reports.
fn entity_count(server: &mut Server) -> usize {
    let list: Value = server.call_json("entity.list", json!({}));
    list.as_array()
        .unwrap_or_else(|| panic!("entity.list is not an array: {list}"))
        .len()
}

#[test]
fn undo_reverts_a_spawn_made_over_mcp() {
    let mut server = Server::spawn(&["--width", "32", "--height", "18"]);
    server.handshake("history");

    let cube = server.call_json("mesh.builtin", json!({ "kind": "cube" }));
    let start = entity_count(&mut server);

    server.call_json(
        "entity.spawn",
        json!({ "mesh": cube["handle"], "name": "agent-cube" }),
    );
    assert_eq!(entity_count(&mut server), start + 1);

    let undo = server.call_json("history.undo", json!({}));
    assert_eq!(undo["ok"], json!(true), "{undo}");
    assert_eq!(entity_count(&mut server), start, "undo removed the spawn");

    let redo = server.call_json("history.redo", json!({}));
    assert_eq!(redo["ok"], json!(true), "{redo}");
    assert_eq!(entity_count(&mut server), start + 1, "redo restored it");

    server.shutdown();
}
