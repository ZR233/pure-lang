use std::{fs, path::PathBuf, sync::Arc};

use anyhow::{Context, Result, bail, ensure};
use clap::Parser;
use pl_provider_fixture::{
    FixtureLiveStatus, FixtureOptions, FixtureServer, GUI_SCENARIOS, ReadyFile, WebSearchFault,
    WebSearchOptions, gui_history_fault_script, gui_history_lock_script, gui_plan_recovery_script,
    gui_realtime_script, gui_script, gui_shutdown_script, gui_statistics_script,
    gui_stress_body_large_script, gui_stress_body_script, gui_stress_script,
    gui_tool_scroll_script, gui_web_search_script, gui_websocket_recovery_script,
};
use tokio_util::sync::CancellationToken;

#[derive(Parser)]
struct Args {
    #[arg(long)]
    scenario: Option<String>,
    #[arg(long)]
    ready_file: Option<PathBuf>,
    #[arg(long)]
    requests_file: Option<PathBuf>,
    #[arg(long)]
    report_file: Option<PathBuf>,
    /// Live strict-script counters, rewritten whenever they change so a
    /// coordinator can prove a window without provider traffic mid-run.
    #[arg(long)]
    status_file: Option<PathBuf>,
    /// Optional web-search fault so the operator can observe an error path.
    #[arg(long, value_name = "NAME")]
    fault: Option<String>,
    /// Run the scenario-owned stdio MCP server instead of the HTTP fixture.
    ///
    /// The product starts this through its supervised worker; the server mimics
    /// a real MCP peer and spawns a SIGTERM-ignoring grandchild so the shutdown
    /// acceptance can prove the product reclaims its own subprocess subtree.
    #[arg(long)]
    mcp_stdio: bool,
    /// Run the scenario-owned fake LSP server instead of the HTTP fixture.
    ///
    /// The product starts this through its supervised worker when a matching
    /// `[lsp.servers.*]` entry is queried; the server speaks the LSP base
    /// protocol and spawns a SIGTERM-ignoring grandchild so the shutdown
    /// acceptance can prove the product reclaims its own subprocess subtree.
    #[arg(long)]
    lsp_stdio: bool,
    /// Run the scenario-owned background tool instead of the HTTP fixture.
    ///
    /// The product starts this through the real supervised tool worker when the
    /// scripted model issues the acceptance `exec` call; the peer spawns a
    /// SIGTERM-ignoring grandchild and keeps running so the shutdown acceptance
    /// can prove the product reclaims its own tool subtree.
    #[arg(long)]
    tool_peer: bool,
    /// Coordination file the stdio peer records its pids into.
    #[arg(long, value_name = "PATH")]
    coord_file: Option<PathBuf>,
}

#[tokio::main]
async fn main() -> Result<()> {
    // The catalog probe runs exactly `<program> --version`; answer it before clap
    // (which would reject the unknown flag) so the configured server is marked
    // available and a real language-service query can start it.
    if std::env::args().skip(1).any(|arg| arg == "--version") {
        println!("anywork-shutdown-lsp {}", env!("CARGO_PKG_VERSION"));
        return Ok(());
    }
    let args = Args::parse();
    if args.mcp_stdio {
        let coord_file = args
            .coord_file
            .clone()
            .context("--coord-file is required with --mcp-stdio")?;
        return pl_provider_fixture::run_mcp_stdio(pl_provider_fixture::McpStdioOptions {
            coord_file,
        });
    }
    if args.lsp_stdio {
        let coord_file = args
            .coord_file
            .clone()
            .context("--coord-file is required with --lsp-stdio")?;
        return pl_provider_fixture::run_lsp_stdio(pl_provider_fixture::LspStdioOptions {
            coord_file,
        });
    }
    if args.tool_peer {
        let coord_file = args
            .coord_file
            .clone()
            .context("--coord-file is required with --tool-peer")?;
        return pl_provider_fixture::run_tool_peer(pl_provider_fixture::ToolPeerOptions {
            coord_file,
        });
    }
    let scenario = args
        .scenario
        .as_deref()
        .context("--scenario is required without --mcp-stdio/--lsp-stdio/--tool-peer")?;
    ensure!(
        GUI_SCENARIOS.contains(&scenario),
        "unknown fixture scenario: {scenario}"
    );
    let steps = match scenario {
        "gui" => gui_script(),
        "storage-compaction" => pl_provider_fixture::gui_storage_compaction_script(),
        "tool-scroll" => gui_tool_scroll_script(),
        "stress" => gui_stress_script(),
        "stress-body" => gui_stress_body_script(),
        "stress-body-large" => gui_stress_body_large_script(),
        "statistics" => gui_statistics_script(),
        "realtime" => gui_realtime_script(),
        "history-lock" => gui_history_lock_script(),
        "history-fault" => gui_history_fault_script(),
        "plan-recovery" => gui_plan_recovery_script(),
        "context-replay-recovery" => pl_provider_fixture::gui_context_replay_recovery_script(),
        "websocket-recovery" => gui_websocket_recovery_script(),
        "call-lifecycle-recovery" => pl_provider_fixture::gui_call_lifecycle_recovery_script(),
        "web-search" => gui_web_search_script(),
        "shutdown" => gui_shutdown_script(),
        value => bail!("unknown fixture scenario: {value}"),
    };
    let fault = match args.fault.as_deref() {
        None => None,
        Some(value) => Some(
            WebSearchFault::parse(value)
                .ok_or_else(|| anyhow::anyhow!("unknown fixture fault: {value}"))?,
        ),
    };
    let options = FixtureOptions {
        web_search: (scenario == "web-search").then_some(WebSearchOptions { fault }),
    };
    let fixture = Arc::new(FixtureServer::start_with_options(steps, options).await?);
    // The status watcher is a plain reader of the same script state the strict
    // match writes, so it can never reorder or mask a request; it only rewrites
    // the counters file when they change.
    let mut status_watch: Option<StatusWatch> = None;
    if let Some(path) = args.status_file.clone() {
        let fixture = Arc::clone(&fixture);
        let stop = CancellationToken::new();
        let token = stop.clone();
        status_watch = Some((
            stop,
            tokio::spawn(async move {
                let mut written: Option<FixtureLiveStatus> = None;
                write_status(&path, &mut written, fixture.live_status())?;
                loop {
                    tokio::select! {
                        () = token.cancelled() => break,
                        () = tokio::time::sleep(std::time::Duration::from_millis(200)) => {
                            write_status(&path, &mut written, fixture.live_status())?;
                        }
                    }
                }
                Ok(())
            }),
        ));
    }
    let ready = ReadyFile {
        base_url: fixture.base_url(),
        ws_url: fixture.ws_url(),
        scenario: scenario.to_owned(),
    };
    let ready_file = args
        .ready_file
        .as_ref()
        .context("--ready-file is required without --mcp-stdio")?;
    write_json(ready_file, &ready)?;
    wait_for_stop(&mut status_watch).await?;
    // Stop the read-only watcher first so the server can be unwrapped from its
    // Arc and shut down exactly once.
    if let Some((stop, watcher)) = status_watch {
        stop.cancel();
        let joined = watcher.await.context("fixture status watcher failed")?;
        joined?;
    }
    let fixture = Arc::try_unwrap(fixture)
        .map_err(|_| anyhow::anyhow!("fixture status watcher still holds the server"))?;
    let report = match fixture.shutdown().await {
        Ok(report) => report,
        Err(error) => {
            // The failure is printed on its own `fixture_` line so the
            // coordinator keeps the reason even when the process exit status is
            // the only other signal.
            println!("fixture_failure=shutdown error: {}", one_line(&error));
            return Err(error).context("fixture shutdown failed");
        }
    };
    if let Some(path) = args.requests_file.as_ref() {
        write_json(path, &report.requests)?;
    }
    if let Some(path) = args.report_file.as_ref() {
        write_json(path, &report.stress)?;
    }
    // Print the strict-match facts first so the coordinator can persist a
    // reviewable reason even when the scenario fails part way through.
    for line in report.diagnostic_lines() {
        println!("{line}");
    }
    // No extra context is attached: the verification message already names the
    // rejected request and the step it expected, and it has to stay the
    // outermost reason the coordinator records.
    report.verify()?;
    Ok(())
}

/// The owned status watcher handle; it is never detached, and its `Ok(())`
/// only happens after the stop token fired with every counter change written.
type StatusWatch = (
    CancellationToken,
    tokio::task::JoinHandle<anyhow::Result<()>>,
);

/// Maps the joined watcher outcome: success is only a cancelled watcher that
/// durably wrote every counter change.
fn status_watch_failed(joined: Result<anyhow::Result<()>, tokio::task::JoinError>) -> Result<()> {
    match joined {
        Ok(Ok(())) => Ok(()),
        Ok(Err(error)) => Err(error.context("fixture status file cannot be kept fresh")),
        Err(error) => Err(anyhow::Error::new(error).context("fixture status watcher crashed")),
    }
}

/// Waits for the platform stop signal, or fails the run the moment the status
/// watcher cannot keep its evidence fresh: the status file is required
/// evidence, so a stale file must end the fixture with a non-zero exit
/// instead of surviving until a coordinator trusts an old value.
async fn wait_for_stop(status_watch: &mut Option<StatusWatch>) -> Result<()> {
    let watcher = async {
        match status_watch.as_mut() {
            Some((_, handle)) => handle.await,
            None => {
                std::future::pending::<Result<anyhow::Result<()>, tokio::task::JoinError>>().await
            }
        }
    };
    #[cfg(unix)]
    {
        let mut terminate =
            tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())?;
        tokio::select! {
            result = tokio::signal::ctrl_c() => result.context("failed to listen for ctrl-c"),
            _ = terminate.recv() => Ok(()),
            joined = watcher => status_watch_failed(joined),
        }
    }
    #[cfg(windows)]
    {
        let mut break_signal = tokio::signal::windows::ctrl_break()?;
        tokio::select! {
            result = tokio::signal::ctrl_c() => result.context("failed to listen for ctrl-c"),
            _ = break_signal.recv() => Ok(()),
            joined = watcher => status_watch_failed(joined),
        }
    }
    #[cfg(not(any(unix, windows)))]
    {
        tokio::select! {
            result = tokio::signal::ctrl_c() => result.context("failed to listen for ctrl-c"),
            joined = watcher => status_watch_failed(joined),
        }
    }
}

/// Writes the live counters whenever they changed.
///
/// The `written` marker only advances after the file is durably rewritten, and
/// a failed write is an error rather than a log line: the status file is
/// required evidence, so the fixture must stop with a non-zero exit instead of
/// letting a coordinator trust a stale value.
fn write_status(
    path: &std::path::Path,
    written: &mut Option<FixtureLiveStatus>,
    status: FixtureLiveStatus,
) -> Result<()> {
    if written.is_some_and(|previous| previous == status) {
        return Ok(());
    }
    write_json(path, &status)?;
    *written = Some(status);
    Ok(())
}

/// Flattens one failure reason so it occupies exactly one log line.
fn one_line(error: &anyhow::Error) -> String {
    error.to_string().replace(['\n', '\r'], " ")
}

fn write_json(path: &std::path::Path, value: &impl serde::Serialize) -> Result<()> {
    let parent = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| std::path::Path::new("."));
    fs::create_dir_all(parent)?;
    let mut temporary = tempfile::NamedTempFile::new_in(parent)?;
    serde_json::to_writer_pretty(&mut temporary, value)?;
    temporary.persist(path).map_err(|error| error.error)?;
    Ok(())
}
