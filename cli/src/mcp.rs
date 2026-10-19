//! `argus mcp`: the tool layer ([`argus_engine::tools`]) as an MCP server over stdio, for
//! Claude Desktop, Claude Code and other MCP clients (ADR 0003). A thin transport: it lists
//! the engine's tool specs and forwards each call. stdout carries the protocol, so logs go
//! to stderr.

use argus_engine::tools::{Part, Tools};
use base64::Engine as _;
use rmcp::model::{
    CallToolRequestParams, CallToolResponse, CallToolResult, ContentBlock, Implementation,
    ListToolsResult, PaginatedRequestParams, ServerCapabilities, ServerConfig, Tool,
    ToolAnnotations,
};
use rmcp::service::RequestContext;
use rmcp::{ErrorData, RoleServer, ServerHandler, ServiceExt};
use std::path::Path;
use std::sync::Arc;

const INSTRUCTIONS: &str = "Argus is a photo Library on this Mac. Photos are grouped into \
Shoots (one per Import). A Verdict is a Lightroom-style flag: pick, reject or unflagged. \
Stacks group bursts of the same moment, best frame first. Before judging any Photo, look at \
its Shoot's Contact sheet (get_contact_sheet) so you know the whole outing. The person's own \
flags win over yours. Originals are never modified.";

struct Server {
    tools: Arc<Tools>,
}

impl ServerHandler for Server {
    fn get_info(&self) -> ServerConfig {
        ServerConfig::new(ServerCapabilities::builder().enable_tools().build())
            .with_server_info(Implementation::new("argus", env!("CARGO_PKG_VERSION")))
            .with_instructions(INSTRUCTIONS)
    }

    async fn list_tools(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> Result<ListToolsResult, ErrorData> {
        let tools = Tools::specs()
            .into_iter()
            .map(|s| {
                let schema = match s.input_schema {
                    serde_json::Value::Object(m) => m,
                    _ => unreachable!("tool schemas are objects"),
                };
                Tool::new(s.name, s.description, Arc::new(schema)).with_annotations(
                    ToolAnnotations::new()
                        .read_only(s.read_only)
                        .destructive(false),
                )
            })
            .collect();
        Ok(ListToolsResult {
            tools,
            ..Default::default()
        })
    }

    async fn call_tool(
        &self,
        request: CallToolRequestParams,
        _context: RequestContext<RoleServer>,
    ) -> Result<CallToolResponse, ErrorData> {
        let tools = self.tools.clone();
        let name = request.name.to_string();
        let args = request
            .arguments
            .map(serde_json::Value::Object)
            .unwrap_or_default();
        // Tools hit SQLite and decode images: keep them off the async workers.
        let out = tokio::task::spawn_blocking(move || tools.call(&name, args))
            .await
            .map_err(|e| ErrorData::internal_error(e.to_string(), None))?;
        Ok(match out {
            Ok(parts) => CallToolResult::success(parts.into_iter().map(content).collect()),
            Err(e) => CallToolResult::error(vec![ContentBlock::text(e.to_string())]),
        }
        .into())
    }
}

fn content(part: Part) -> ContentBlock {
    match part {
        Part::Json(v) => ContentBlock::text(v.to_string()),
        Part::Jpeg(bytes) => ContentBlock::image(
            base64::engine::general_purpose::STANDARD.encode(bytes),
            "image/jpeg",
        ),
    }
}

pub fn run(library: &Path) -> Result<(), Box<dyn std::error::Error>> {
    // Fail fast on a bad Library path rather than on the first tool call.
    argus_engine::library::Library::open(library)?;
    let server = Server {
        tools: Arc::new(Tools::new(library)),
    };
    eprintln!("argus mcp: serving Library {} on stdio", library.display());
    // Queued work (Suggestions, previews) runs while a client is connected, as in the app.
    let root = library.to_path_buf();
    let _queue = (std::env::var("ARGUS_QUEUE").as_deref() != Ok("off"))
        .then(|| argus_engine::jobs::Worker::start(move || Some(root.clone()), Tools::new));
    tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?
        .block_on(async {
            server
                .serve(rmcp::transport::stdio())
                .await?
                .waiting()
                .await?;
            Ok(())
        })
}
