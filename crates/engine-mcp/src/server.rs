//! The rmcp [`ServerHandler`]: lists the registry's tools and forwards calls
//! to the engine thread, mapping engine types to MCP types.

use std::sync::Arc;

use base64::Engine as _;
use engine::tool::{ToolError, ToolInfo, ToolOutput};
use rmcp::model::{
    CallToolRequestParams, CallToolResponse, CallToolResult, ContentBlock, Implementation,
    ListToolsResult, PaginatedRequestParams, ServerCapabilities, ServerConfig, Tool,
};
use rmcp::service::RequestContext;
use rmcp::{ErrorData, RoleServer, ServerHandler};
use serde_json::Value;

use crate::engine_thread::EngineHandle;

/// MCP server exposing every tool in an `engine::tool::ToolRegistry`.
#[derive(Clone)]
pub struct EngineServer {
    engine: EngineHandle,
    /// The registry's tools as MCP tools. The registry is fixed at startup,
    /// so this is computed once.
    tools: Arc<Vec<Tool>>,
}

impl EngineServer {
    /// Creates a server that lists `tools` and runs calls on `engine`.
    pub fn new(engine: EngineHandle, tools: &[ToolInfo]) -> Self {
        Self {
            engine,
            tools: Arc::new(tools.iter().map(to_mcp_tool).collect()),
        }
    }
}

impl ServerHandler for EngineServer {
    fn get_info(&self) -> ServerConfig {
        ServerConfig::new(ServerCapabilities::builder().enable_tools().build())
            .with_server_info(Implementation::new(
                env!("CARGO_PKG_NAME"),
                env!("CARGO_PKG_VERSION"),
            ))
            .with_instructions(
                "Drives a headless instance of the engine. Call `engine.info` for \
                 its state and `engine.list_tools` for the available tools.",
            )
    }

    async fn list_tools(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> Result<ListToolsResult, ErrorData> {
        Ok(ListToolsResult::with_all_items(self.tools.as_ref().clone()))
    }

    async fn call_tool(
        &self,
        request: CallToolRequestParams,
        _context: RequestContext<RoleServer>,
    ) -> Result<CallToolResponse, ErrorData> {
        let args = Value::Object(request.arguments.unwrap_or_default());
        let output = self
            .engine
            .call(request.name.into_owned(), args)
            .await
            .map_err(to_mcp_error)?;
        Ok(CallToolResult::success(vec![to_content(output)?]).into())
    }
}

/// Maps a registry entry to an MCP tool definition.
fn to_mcp_tool(info: &ToolInfo) -> Tool {
    let schema = match &info.input_schema {
        Value::Object(map) => map.clone(),
        // Registry schemas are always objects; never expose a broken one.
        other => {
            log::warn!("tool `{}` has a non-object schema: {other}", info.name);
            serde_json::Map::from_iter([("type".to_string(), Value::from("object"))])
        }
    };
    Tool::new(info.name.clone(), info.description.clone(), schema)
}

/// Maps a tool's output to MCP content.
fn to_content(output: ToolOutput) -> Result<ContentBlock, ErrorData> {
    Ok(match output {
        ToolOutput::Text(text) => ContentBlock::text(text),
        ToolOutput::Json(value) => {
            let text = serde_json::to_string_pretty(&value)
                .map_err(|err| ErrorData::internal_error(err.to_string(), None))?;
            ContentBlock::text(text)
        }
        ToolOutput::Image { png, .. } => ContentBlock::image(
            base64::engine::general_purpose::STANDARD.encode(png),
            "image/png",
        ),
    })
}

/// Maps a tool error to a JSON-RPC error, preserving its message.
/// Invalid arguments are invalid-params; everything else is internal.
fn to_mcp_error(err: ToolError) -> ErrorData {
    let message = format!("{err:#}");
    match err {
        ToolError::InvalidArgs { .. } => ErrorData::invalid_params(message, None),
        ToolError::Unknown(_) | ToolError::Failed(_) => ErrorData::internal_error(message, None),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rmcp::model::ErrorCode;
    use serde_json::json;

    #[test]
    fn json_output_is_pretty_text() {
        let content = to_content(ToolOutput::Json(json!({ "entities": 0 }))).unwrap();
        let ContentBlock::Text(text) = content else {
            panic!("expected text, got {content:?}");
        };
        assert_eq!(text.text, "{\n  \"entities\": 0\n}");
    }

    #[test]
    fn image_output_is_base64_png() {
        let content = to_content(ToolOutput::Image {
            png: vec![1, 2, 3],
            width: 1,
            height: 1,
        })
        .unwrap();
        let ContentBlock::Image(image) = content else {
            panic!("expected image, got {content:?}");
        };
        assert_eq!(image.data, "AQID");
        assert_eq!(image.mime_type, "image/png");
    }

    #[test]
    fn errors_keep_their_message_and_kind() {
        let invalid = to_mcp_error(ToolError::InvalidArgs {
            tool: "a.b".into(),
            message: "field `x`".into(),
        });
        assert_eq!(invalid.code, ErrorCode::INVALID_PARAMS);
        assert!(invalid.message.contains("field `x`"));

        let unknown = to_mcp_error(ToolError::Unknown("nope".into()));
        assert_eq!(unknown.code, ErrorCode::INTERNAL_ERROR);
        assert!(unknown.message.contains("nope"));
    }
}
