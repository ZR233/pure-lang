use std::{fs, path::PathBuf};

use anyhow::{Context, Result, bail, ensure};
use clap::Parser;
use pl_provider_fixture::{
    FixtureServer, GUI_SCENARIOS, ReadyFile, gui_history_fault_script, gui_history_lock_script,
    gui_realtime_script, gui_script, gui_statistics_script, gui_stress_body_large_script,
    gui_stress_body_script, gui_stress_script,
};

#[derive(Parser)]
struct Args {
    #[arg(long)]
    scenario: String,
    #[arg(long)]
    ready_file: PathBuf,
    #[arg(long)]
    requests_file: Option<PathBuf>,
    #[arg(long)]
    report_file: Option<PathBuf>,
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();
    let scenario = args.scenario.as_str();
    ensure!(
        GUI_SCENARIOS.contains(&scenario),
        "unknown fixture scenario: {scenario}"
    );
    let steps = match scenario {
        "gui" => gui_script(),
        "stress" => gui_stress_script(),
        "stress-body" => gui_stress_body_script(),
        "stress-body-large" => gui_stress_body_large_script(),
        "statistics" => gui_statistics_script(),
        "realtime" => gui_realtime_script(),
        "history-lock" => gui_history_lock_script(),
        "history-fault" => gui_history_fault_script(),
        value => bail!("unknown fixture scenario: {value}"),
    };
    let fixture = FixtureServer::start(steps).await?;
    let ready = ReadyFile {
        base_url: fixture.base_url(),
        ws_url: fixture.ws_url(),
        scenario: args.scenario,
    };
    write_json(&args.ready_file, &ready)?;
    #[cfg(unix)]
    {
        let mut terminate =
            tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())?;
        tokio::select! { result = tokio::signal::ctrl_c() => result?, _ = terminate.recv() => {} }
    }
    #[cfg(windows)]
    {
        let mut break_signal = tokio::signal::windows::ctrl_break()?;
        tokio::select! { result = tokio::signal::ctrl_c() => result?, _ = break_signal.recv() => {} }
    }
    #[cfg(not(any(unix, windows)))]
    tokio::signal::ctrl_c().await?;
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
