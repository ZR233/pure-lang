//! Scenario-owned fake LSP server used by the shutdown acceptance.
//!
//! The product starts a matching `[lsp.servers.<id>]` entry through its own
//! supervised worker when a language-service query is issued, and must reclaim
//! that whole subtree on exit. This server speaks the real LSP base protocol
//! (Content-Length framed `initialize`/`initialized`/`shutdown`/`exit`) so the
//! client's handshake succeeds, and in addition spawns a grandchild that ignores
//! `SIGTERM` and lives in its own process group. That grandchild can only be
//! reclaimed by the product's real descendant supervision, so the acceptance can
//! tell a self-supervised language server apart from a leaked one.
//!
//! The server records its own pid and the grandchild's pid (with `/proc` start
//! times where available) into a coordination file so the coordinator can
//! OS-poll the real business PIDs. It never kills the grandchild itself.
//!
//! The same binary also answers `<program> --version` (see `main.rs`) because the
//! catalog probe runs exactly that before the server is considered available.

use anyhow::{Context, Result};
use serde_json::{Value, json};
use std::fs;
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};

use crate::stdio_peer::{proc_start_ticks, spawn_tracked_grandchild};

/// Options for [`run_lsp_stdio`].
pub struct LspStdioOptions {
    /// File the server writes its own and the grandchild's pid/starttime into.
    pub coord_file: PathBuf,
}

/// Runs the scenario-owned LSP server until the client sends `exit`.
pub fn run_lsp_stdio(options: LspStdioOptions) -> Result<()> {
    let grandchild_pid = spawn_tracked_grandchild()?;
    let lsp_pid = std::process::id();
    write_coord_record(&options.coord_file, lsp_pid, grandchild_pid)?;
    serve()
}

/// Writes the server and grandchild identities so the coordinator can OS-poll
/// the real business PIDs with a pid-reuse guard.
fn write_coord_record(coord_file: &Path, lsp_pid: u32, grandchild_pid: u32) -> Result<()> {
    let record = json!({
        "schema": "anywork-lsp-stdio/1",
        "lspPid": lsp_pid,
        "lspStartTicks": proc_start_ticks(lsp_pid),
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

/// Serves the LSP base protocol until `exit` (or the client closes stdin).
fn serve() -> Result<()> {
    let stdin = std::io::stdin();
    let mut reader = BufReader::new(stdin.lock());
    let stdout = std::io::stdout();
    let mut out = stdout.lock();
    while let Some(message) = read_message(&mut reader)? {
        let method = message["method"].as_str().unwrap_or_default().to_string();
        let id = message.get("id").cloned();
        match method.as_str() {
            // `exit` is a notification: the server terminates after replying to
            // the preceding `shutdown` request.
            "exit" => return Ok(()),
            "initialize" => reply(&mut out, id.as_ref(), initialize_result())?,
            "shutdown" => reply(&mut out, id.as_ref(), Value::Null)?,
            // Notifications (`initialized`, `textDocument/didOpen`, ...) get no
            // reply; `reply` already ignores a missing id, but name them so an
            // unknown notification is never mistaken for a request.
            "initialized"
            | "textDocument/didOpen"
            | "textDocument/didChange"
            | "textDocument/didClose"
            | "textDocument/didSave" => {}
            _ => reply(&mut out, id.as_ref(), default_result(&method))?,
        }
    }
    Ok(())
}

/// Reads one Content-Length framed JSON-RPC message, or `None` at EOF.
fn read_message(reader: &mut impl BufRead) -> Result<Option<Value>> {
    let mut content_length: Option<usize> = None;
    loop {
        let mut line = String::new();
        if reader.read_line(&mut line)? == 0 {
            return Ok(None);
        }
        let trimmed = line.trim_end_matches(['\r', '\n']);
        if trimmed.is_empty() {
            break;
        }
        if let Some(value) = trimmed.strip_prefix("Content-Length:") {
            content_length = value.trim().parse::<usize>().ok();
        }
    }
    let Some(length) = content_length else {
        return Ok(None);
    };
    let mut buffer = vec![0u8; length];
    reader
        .read_exact(&mut buffer)
        .context("failed to read an LSP message body")?;
    Ok(Some(
        serde_json::from_slice(&buffer).context("failed to decode an LSP message body")?,
    ))
}

/// Writes one Content-Length framed JSON-RPC response.
///
/// A missing id (a notification) is silently skipped; the fixture never answers
/// notifications, matching a real language server.
fn reply(out: &mut impl Write, id: Option<&Value>, result: Value) -> Result<()> {
    let Some(id) = id else {
        return Ok(());
    };
    let body = serde_json::to_vec(&json!({"jsonrpc": "2.0", "id": id, "result": result}))?;
    write!(out, "Content-Length: {}\r\n\r\n", body.len())?;
    out.write_all(&body)?;
    out.flush()?;
    Ok(())
}

/// The `initialize` result; the client only checks that the request succeeded,
/// but the capabilities keep the payload a truthful LSP server description.
fn initialize_result() -> Value {
    json!({
        "capabilities": {
            "textDocumentSync": 1,
            "hoverProvider": true,
            "definitionProvider": true,
            "referencesProvider": true,
            "documentSymbolProvider": true,
            "workspaceSymbolProvider": true,
        },
        "serverInfo": {
            "name": "anywork-shutdown-lsp-fixture",
            "version": env!("CARGO_PKG_VERSION"),
        },
    })
}

/// A minimal well-formed result for the queries the acceptance issues.
///
/// The committed tool output must contain the coordination marker, so the hover
/// body carries scenario-owned text the fixture script matches.
fn default_result(method: &str) -> Value {
    match method {
        "textDocument/hover" => json!({
            "contents": {"kind": "plaintext", "value": "shutdown lsp fixture hover"},
        }),
        _ => Value::Null,
    }
}
