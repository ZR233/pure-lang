use std::{fs, path::PathBuf};

use anyhow::{Context, Result, bail};
use clap::Parser;
use pl_provider_fixture::{
    FixtureServer, ReadyFile, gui_script, gui_statistics_script, gui_stress_script,
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
    let steps = match args.scenario.as_str() {
        "gui" => gui_script(),
        "stress" => gui_stress_script(),
        "statistics" => gui_statistics_script(),
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
    let report = fixture
        .shutdown()
        .await
        .context("fixture shutdown failed")?;
    if let Some(path) = args.requests_file.as_ref() {
        write_json(path, &report.requests)?;
    }
    if let Some(path) = args.report_file.as_ref() {
        write_json(path, &report.stress)?;
    }
    report.verify().context("fixture scenario failed")?;
    println!(
        "fixture accepted {} requests",
        report
            .requests
            .iter()
            .filter(|request| request.accepted)
            .count()
    );
    Ok(())
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
