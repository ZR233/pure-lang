//! Scenario-owned stdio MCP server used by the shutdown acceptance.
//!
//! The product starts enabled `[mcp.servers.<id>]` entries through its own
//! supervised worker and must reclaim that whole subtree on exit. This server
//! mimics a real stdio MCP peer (the legacy `initialize` handshake plus
//! `tools/list`) and, in addition, spawns a grandchild that ignores `SIGTERM`
//! and lives in its own process group. That grandchild can only be reclaimed by
//! the product's real descendant supervision, so the acceptance can tell a
//! self-supervised service apart from a leaked one.
//!
//! The server records its own pid and the grandchild's pid (with `/proc` start
//! times where available) into a coordination file so the coordinator can
//! OS-poll the real business PIDs. It never kills the grandchild itself: the
//! whole point is that the product's supervisor must do it.

use anyhow::{Context, Result};
use serde_json::{Value, json};
use std::fs;
use std::io::{BufRead, Write};
use std::path::{Path, PathBuf};

use crate::stdio_peer::{proc_start_ticks, spawn_tracked_grandchild};

/// Options for [`run_mcp_stdio`].
pub struct McpStdioOptions {
    /// File the server writes its own and the grandchild's pid/starttime into.
    pub coord_file: PathBuf,
}

/// The protocol versions the fixture accepts, newest first. Kept in step with
/// `pl_tool`'s MCP client so the legacy `initialize` negotiation succeeds.
const ACCEPTED_PROTOCOL_VERSIONS: [&str; 5] = [
    "2026-07-28",
    "2025-11-25",
    "2025-06-18",
    "2025-03-26",
    "2024-11-05",
];

/// Runs the scenario-owned stdio MCP server until the supervisor closes stdin.
pub fn run_mcp_stdio(options: McpStdioOptions) -> Result<()> {
    // Fail loudly rather than recording a pid that is already gone: a missing
    // `sleep` (or any immediate exec failure) must not turn into a vacuous
    // "reclamation" pass. The helper keeps the grandchild handle owned so it
    // cannot become a zombie and never kills it here.
    let grandchild_pid = spawn_tracked_grandchild()?;
    let mcp_pid = std::process::id();
    write_coord_record(&options.coord_file, mcp_pid, grandchild_pid)?;
    serve()
}

/// Writes the server and grandchild identities so the coordinator can OS-poll
/// the real business PIDs with a pid-reuse guard.
fn write_coord_record(coord_file: &Path, mcp_pid: u32, grandchild_pid: u32) -> Result<()> {
    let record = json!({
        "schema": "anywork-mcp-stdio/1",
        "mcpPid": mcp_pid,
        "mcpStartTicks": proc_start_ticks(mcp_pid),
        "grandchildPid": grandchild_pid,
        "grandchildStartTicks": proc_start_ticks(grandchild_pid),
    });
    if let Some(parent) = coord_file.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create {}", parent.display()))?;
    }
    let mut temporary = tempfile::NamedTempFile::new_in(
        coord_file
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
            .unwrap_or_else(|| Path::new(".")),
    )?;
    serde_json::to_writer_pretty(&mut temporary, &record)?;
    temporary.persist(coord_file).map_err(|error| error.error)?;
    Ok(())
}

/// Serves newline-delimited JSON-RPC until the supervisor closes stdin.
fn serve() -> Result<()> {
    let stdin = std::io::stdin();
    let stdout = std::io::stdout();
    let mut out = stdout.lock();
    for line in stdin.lock().lines() {
        let line = line.context("failed to read an MCP request line")?;
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let Ok(request) = serde_json::from_str::<Value>(trimmed) else {
            continue;
        };
        // Notifications (no id) get no reply per JSON-RPC; `initialize` always
        // carries an id, so this only skips `notifications/initialized`.
        let Some(id) = request.get("id").cloned() else {
            continue;
        };
        if id.is_null() {
            continue;
        }
        let method = request["method"].as_str().unwrap_or_default();
        if let Some(response) = respond(method, &request, &id) {
            serde_json::to_writer(&mut out, &response)?;
            out.write_all(b"\n")?;
            out.flush()?;
        }
    }
    Ok(())
}

/// Builds the JSON-RPC reply for one request, or `None` for `server/discover`
/// (method-not-found) to make the client fall back to legacy `initialize`.
fn respond(method: &str, request: &Value, id: &Value) -> Option<Value> {
    match method {
        "server/discover" => Some(json!({
            "jsonrpc": "2.0",
            "id": id,
            "error": {"code": -32601, "message": "method not found"},
        })),
        "initialize" => {
            let requested = request["params"]["protocolVersion"]
                .as_str()
                .unwrap_or_default();
            let version = if ACCEPTED_PROTOCOL_VERSIONS.contains(&requested) {
                requested
            } else {
                "2024-11-05"
            };
            Some(json!({
                "jsonrpc": "2.0",
                "id": id,
                "result": {
                    "protocolVersion": version,
                    "capabilities": {"tools": {"listChanged": false}},
                    "serverInfo": {
                        "name": "anywork-shutdown-fixture",
                        "version": env!("CARGO_PKG_VERSION"),
                    },
                },
            }))
        }
        "tools/list" => Some(json!({
            "jsonrpc": "2.0",
            "id": id,
            "result": {
                "tools": [{
                    "name": "shutdown_fixture_echo",
                    "description": "scenario-owned echo tool for shutdown acceptance",
                    "inputSchema": {
                        "type": "object",
                        "properties": {"text": {"type": "string"}},
                    },
                }],
            },
        })),
        "tools/call" => Some(json!({
            "jsonrpc": "2.0",
            "id": id,
            "result": {
                "content": [{"type": "text", "text": "shutdown fixture tool ok"}],
                "isError": false,
            },
        })),
        "resources/list" => Some(json!({
            "jsonrpc": "2.0",
            "id": id,
            "result": {"resources": []},
        })),
        "prompts/list" => Some(json!({
            "jsonrpc": "2.0",
            "id": id,
            "result": {"prompts": []},
        })),
        "ping" => Some(json!({"jsonrpc": "2.0", "id": id, "result": {}})),
        _ => Some(json!({
            "jsonrpc": "2.0",
            "id": id,
            "error": {"code": -32601, "message": "method not found"},
        })),
    }
}
