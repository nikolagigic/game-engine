//! `engine-mcp`: an MCP server over stdio that exposes every tool in
//! `engine::tool::ToolRegistry`. Owned by GE-014.
//!
//! Stub: filled in by its ticket. The crate exists now so the dependency
//! graph (rmcp, tokio) is resolved and locked from day one.

fn main() {
    env_logger::init();
    log::info!("engine-mcp stub: implemented by GE-014");
}
