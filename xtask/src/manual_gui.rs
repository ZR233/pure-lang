use crate::cli::ManualGuiOptions;
use crate::paths;
use crate::process;
use anyhow::{Context, Result, bail, ensure};
use pl_model::config::{ProviderConfig, ProviderId};
use pl_model::model::{ModelInfo, ModelTransportProfile};
use pl_model::provider::ProviderEndpoint;
use pl_studio_runtime::config::StudioConfig;
use sea_orm::sqlx::{
    Connection as _,
    sqlite::{SqliteConnectOptions, SqliteConnection},
};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::fs::{self, File};
use std::io::{self, IsTerminal, Write};
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

#[derive(Deserialize)]
struct FixtureReady {
    base_url: String,
    ws_url: String,
    scenario: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct StressSessionsReport {
    original_thread_id: String,
    original_reopened: bool,
    original_window_items: usize,
    session_count: usize,
    directory_count: usize,
    sessions: Vec<StressSessionReport>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct StressSessionReport {
    ordinal: usize,
    thread_id: String,
    window_items: usize,
    previewed_bodies: usize,
    pending_bodies: usize,
    assistant_characters: Option<usize>,
}

/// The stress probe's window-paging evidence.
///
/// Proves the production `load-older` command really moved the typed content
/// window (`timelineWindow`) toward older content while retaining identity,
/// instead of a version counter changing on its own. `capacity` is reported by
/// the probe itself (the native `ChatView` window bound in `pl-core/src/chat.rs`)
/// so the multi-session bound below never hard-codes the number twice.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ProbePagingReport {
    capacity: usize,
    paged: bool,
    page_count: usize,
    final_items: Option<usize>,
    saturated: bool,
    moved_older: bool,
    identity_retained: bool,
    anchor_retained: bool,
    all_within_capacity: bool,
    all_unique: bool,
}

#[derive(Deserialize, Serialize, PartialEq, Eq, Debug)]
#[serde(rename_all = "camelCase")]
struct StatisticsDbCounts {
    model_calls: usize,
    committed_calls: usize,
    performance_samples: usize,
}

#[derive(Serialize)]
struct ManualOperation {
    at_unix_ms: u128,
    action: &'static str,
}

enum InputEvent {
    Action(ManualOperation),
    Done,
    Eof,
}

struct OwnedProcess {
    child: Child,
    stopped: bool,
}

/// One exclusive external SQLite write transaction held for an acceptance window.
///
/// It opens the real database file (never a copy) and takes `BEGIN IMMEDIATE`,
/// so the runtime writer for that exact library cannot commit while the
/// transaction is held. It gates every accepted scenario the same way and is
/// released from [`Drop`], so a failed or cancelled run can never leave the lock
/// behind.
struct SqliteWriteLock {
    release: mpsc::Sender<()>,
    worker: Option<thread::JoinHandle<Result<()>>>,
}

struct StatisticsScenario<'a> {
    workspace: &'a Path,
    app_dir: &'a Path,
    home: &'a Path,
    working: &'a Path,
    output: &'a Path,
    fixture_log: &'a Path,
    requests_file: &'a Path,
    report_file: &'a Path,
    interrupt: &'a mpsc::Receiver<()>,
}

struct RealtimeScenario<'a> {
    workspace: &'a Path,
    app_dir: &'a Path,
    home: &'a Path,
    working: &'a Path,
    output: &'a Path,
    fixture_log: &'a Path,
    requests_file: &'a Path,
    interrupt: &'a mpsc::Receiver<()>,
}

struct StressBodyScenario<'a> {
    /// `stress-body` or `stress-body-large`; selects the fixture script and the
    /// Driver's expected body. Both drive the same journey.
    scenario: &'a str,
    workspace: &'a Path,
    app_dir: &'a Path,
    home: &'a Path,
    working: &'a Path,
    output: &'a Path,
    fixture_log: &'a Path,
    requests_file: &'a Path,
    interrupt: &'a mpsc::Receiver<()>,
}

struct HistoryLockScenario<'a> {
    workspace: &'a Path,
    app_dir: &'a Path,
    home: &'a Path,
    working: &'a Path,
    output: &'a Path,
    fixture_log: &'a Path,
    requests_file: &'a Path,
    interrupt: &'a mpsc::Receiver<()>,
}

struct HistoryFaultScenario<'a> {
    workspace: &'a Path,
    app_dir: &'a Path,
    home: &'a Path,
    working: &'a Path,
    output: &'a Path,
    fixture_log: &'a Path,
    requests_file: &'a Path,
    interrupt: &'a mpsc::Receiver<()>,
}

impl SqliteWriteLock {
    /// Acquires `BEGIN IMMEDIATE` on [database], naming it [label] in errors.
    ///
    /// [hold_timeout] bounds how long the holder waits for the coordinator to
    /// release it; the worker commits and closes on release, and [`Drop`] is the
    /// backstop, so a failed or cancelled run can never leave the lock behind.
    fn acquire(database: &Path, label: &str, hold_timeout: Duration) -> Result<Self> {
        let (acquired_tx, acquired_rx) = mpsc::sync_channel(1);
        let (release, release_rx) = mpsc::channel();
        let path = database.to_owned();
        let label = label.to_owned();
        let worker_label = label.clone();
        let worker = thread::spawn(move || -> Result<()> {
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()?;
            runtime.block_on(async move {
                let options = SqliteConnectOptions::new()
                    .filename(path)
                    .create_if_missing(false);
                let mut connection = SqliteConnection::connect_with(&options).await?;
                sea_orm::sqlx::query("BEGIN IMMEDIATE")
                    .execute(&mut connection)
                    .await?;
                acquired_tx.send(()).ok();
                release_rx
                    .recv_timeout(hold_timeout)
                    .with_context(|| format!("{worker_label} SQLite lock release timed out"))?;
                sea_orm::sqlx::query("COMMIT")
                    .execute(&mut connection)
                    .await?;
                Ok(())
            })
        });
        let mut lock = Self {
            release,
            worker: Some(worker),
        };
        if acquired_rx.recv_timeout(Duration::from_secs(8)).is_err() {
            let result = lock.finish();
            result?;
            bail!("{label} BEGIN IMMEDIATE was not acquired");
        }
        Ok(lock)
    }

    fn finish(&mut self) -> Result<()> {
        self.release.send(()).ok();
        if let Some(worker) = self.worker.take() {
            worker
                .join()
                .map_err(|_| anyhow::anyhow!("SQLite write lock holder panicked"))??;
        }
        Ok(())
    }
}

impl Drop for SqliteWriteLock {
    fn drop(&mut self) {
        let _ = self.finish();
    }
}

impl OwnedProcess {
    fn start(command: &mut Command, fixture: bool) -> Result<Self> {
        #[cfg(unix)]
        {
            use std::os::unix::process::CommandExt;
            command.process_group(0);
        }
        #[cfg(windows)]
        if fixture {
            use std::os::windows::process::CommandExt;
            use windows_sys::Win32::System::Threading::CREATE_NEW_PROCESS_GROUP;
            command.creation_flags(CREATE_NEW_PROCESS_GROUP);
        } else {
            process::configure_background_command(command);
        }
        #[cfg(not(windows))]
        let _ = fixture;
        #[cfg(not(windows))]
        process::configure_background_command(command);
        Ok(Self {
            child: command.spawn().context("failed to start child process")?,
            stopped: false,
        })
    }

    fn exited(&mut self) -> Result<bool> {
        Ok(self.child.try_wait()?.is_some())
    }

    fn stop(&mut self, report: &Path) -> Result<std::process::ExitStatus> {
        #[cfg(unix)]
        {
            let status = Command::new("kill")
                .arg("-TERM")
                .arg("--")
                .arg(format!("-{}", self.child.id()))
                .status()?;
            ensure!(status.success(), "failed to signal process group: {status}");
        }
        #[cfg(windows)]
        {
            use windows_sys::Win32::System::Console::{CTRL_BREAK_EVENT, GenerateConsoleCtrlEvent};
            ensure!(
                unsafe { GenerateConsoleCtrlEvent(CTRL_BREAK_EVENT, self.child.id()) } != 0,
                "failed to signal fixture process group: {}",
                std::io::Error::last_os_error()
            );
        }
        let deadline = Instant::now() + Duration::from_secs(10);
        let mut status = None;
        while Instant::now() < deadline {
            if status.is_none() {
                status = self.child.try_wait()?;
            }
            if report.is_file()
                && let Some(status) = status
            {
                self.stopped = true;
                return Ok(status);
            }
            thread::sleep(Duration::from_millis(100));
        }
        status.context("fixture did not write requests-file after graceful signal")
    }
}

impl Drop for OwnedProcess {
    fn drop(&mut self) {
        if self.stopped {
            return;
        }
        #[cfg(unix)]
        {
            // Terminate descendants even when the group leader has exited.
            let _ = Command::new("kill")
                .arg("-TERM")
                .arg("--")
                .arg(format!("-{}", self.child.id()))
                .stderr(Stdio::null())
                .status();
            thread::sleep(Duration::from_millis(200));
            let _ = Command::new("kill")
                .arg("-KILL")
                .arg("--")
                .arg(format!("-{}", self.child.id()))
                .stderr(Stdio::null())
                .status();
        }
        #[cfg(windows)]
        {
            let _ = Command::new("taskkill")
                .args(["/T", "/F", "/PID"])
                .arg(self.child.id().to_string())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status();
        }
        let _ = self.child.wait();
    }
}

pub(crate) fn run(options: ManualGuiOptions) -> Result<()> {
    // The statistics, realtime and history-writer journeys are fully driven by
    // Flutter Driver and reviewed from captured evidence, so they do not require
    // an interactive stdin.
    if !matches!(
        options.scenario.as_str(),
        "statistics"
            | "realtime"
            | "stress-body"
            | "stress-body-large"
            | "history-lock"
            | "history-fault"
    ) {
        ensure!(
            io::stdin().is_terminal(),
            "manual-gui requires an interactive terminal"
        );
    }
    let (interrupt_tx, interrupt_rx) = mpsc::channel();
    thread::spawn(move || {
        if let Ok(runtime) = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
        {
            let _ = runtime.block_on(tokio::signal::ctrl_c());
            let _ = interrupt_tx.send(());
        }
    });
    let workspace = paths::workspace_root()?;
    let app_dir = paths::studio_app_dir(&workspace);
    let stamp = SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs();
    let output = options.output.unwrap_or_else(|| {
        workspace
            .join("target/manual-gui")
            .join(format!("{stamp}-{}", std::process::id()))
    });
    ensure!(
        !output.exists(),
        "evidence directory already exists: {}",
        output.display()
    );
    fs::create_dir_all(&output)?;
    let output = output.canonicalize()?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&output, fs::Permissions::from_mode(0o700))?;
    }
    fs::write(
        output.join("verdict.json"),
        "{\"human_verdict\":\"pending\"}\n",
    )?;
    fs::write(output.join("operations.json"), "[]\n")?;

    let working = tempfile::tempdir().context("failed to create isolated GUI home")?;
    let home = working.path().join("home");
    fs::create_dir(&home)?;
    let ready_file = working.path().join("fixture-ready.json");
    let requests_file = working.path().join("requests.json");
    let stress_report_file = working.path().join("stress-report.json");
    let probe_stop = working.path().join("probe-stop");
    let probe_report = working.path().join("probe-report.json");
    // The probe writes its window-paging evidence as a sibling of the sample
    // report; keep the exact name in sync with `_pagingEvidenceFile`.
    let probe_paging = working.path().join("probe-report.paging.json");
    let probe_frames = working.path().join("probe-frames.json");
    let stress_stage = working.path().join("stress-stage");
    let stress_sessions_stage = working.path().join("stress-sessions-stage");
    let stress_sessions_report = working.path().join("stress-sessions.json");
    let probe_ready = working.path().join("probe-ready");
    let probe_finished = working.path().join("probe-finished");
    let fixture_log_path = working.path().join("fixture.log");
    let fixture_log = File::create(&fixture_log_path)?;
    let mut fixture_command = Command::new("cargo");
    fixture_command
        .current_dir(&workspace)
        .args([
            "run",
            "-p",
            "pl-provider-fixture",
            "--",
            "--scenario",
            &options.scenario,
            "--ready-file",
        ])
        .arg(&ready_file)
        .arg("--requests-file")
        .arg(&requests_file)
        .arg("--report-file")
        .arg(&stress_report_file)
        .stdout(Stdio::from(fixture_log.try_clone()?))
        .stderr(Stdio::from(fixture_log));
    #[cfg(windows)]
    process::own_current_process_tree()?;
    let mut fixture = match OwnedProcess::start(&mut fixture_command, true) {
        Ok(fixture) => fixture,
        Err(error) => {
            write_fixture_log(&fixture_log_path, &output.join("fixture.log"))?;
            return Err(error);
        }
    };
    let ready = match wait_for_ready(&ready_file, &mut fixture, &interrupt_rx) {
        Ok(ready) => ready,
        Err(error) => {
            let _ = fixture.stop(&requests_file);
            drop(fixture);
            write_fixture_log(&fixture_log_path, &output.join("fixture.log"))?;
            if requests_file.exists() {
                sanitize_requests(&requests_file, &output.join("requests.json"))?;
            }
            return Err(error);
        }
    };
    ensure!(
        ready.scenario == options.scenario,
        "fixture scenario does not match request"
    );
    if let Err(error) = write_config(&home, &ready) {
        let _ = fixture.stop(&requests_file);
        drop(fixture);
        write_fixture_log(&fixture_log_path, &output.join("fixture.log"))?;
        return Err(error);
    }

    if options.scenario == "statistics" {
        return run_statistics_scenario(
            &StatisticsScenario {
                workspace: &workspace,
                app_dir: &app_dir,
                home: &home,
                working: working.path(),
                output: &output,
                fixture_log: &fixture_log_path,
                requests_file: &requests_file,
                report_file: &stress_report_file,
                interrupt: &interrupt_rx,
            },
            fixture,
        );
    }

    if options.scenario == "realtime" {
        return run_realtime_scenario(
            &RealtimeScenario {
                workspace: &workspace,
                app_dir: &app_dir,
                home: &home,
                working: working.path(),
                output: &output,
                fixture_log: &fixture_log_path,
                requests_file: &requests_file,
                interrupt: &interrupt_rx,
            },
            fixture,
        );
    }

    if matches!(
        options.scenario.as_str(),
        "stress-body" | "stress-body-large"
    ) {
        return run_stress_body_scenario(
            &StressBodyScenario {
                scenario: options.scenario.as_str(),
                workspace: &workspace,
                app_dir: &app_dir,
                home: &home,
                working: working.path(),
                output: &output,
                fixture_log: &fixture_log_path,
                requests_file: &requests_file,
                interrupt: &interrupt_rx,
            },
            fixture,
        );
    }

    if options.scenario == "history-lock" {
        return run_history_lock_scenario(
            &HistoryLockScenario {
                workspace: &workspace,
                app_dir: &app_dir,
                home: &home,
                working: working.path(),
                output: &output,
                fixture_log: &fixture_log_path,
                requests_file: &requests_file,
                interrupt: &interrupt_rx,
            },
            fixture,
        );
    }

    if options.scenario == "history-fault" {
        return run_history_fault_scenario(
            &HistoryFaultScenario {
                workspace: &workspace,
                app_dir: &app_dir,
                home: &home,
                working: working.path(),
                output: &output,
                fixture_log: &fixture_log_path,
                requests_file: &requests_file,
                interrupt: &interrupt_rx,
            },
            fixture,
        );
    }

    let gui_log_path = working.path().join("gui.log");
    let gui_log = File::create(&gui_log_path)?;
    let mut gui_command = Command::new("cargo");
    gui_command
        .current_dir(&workspace)
        .args(["xtask", "run-gui", "--driver"])
        .env("ANYWORK_HOME", &home)
        .stdout(Stdio::from(gui_log.try_clone()?))
        .stderr(Stdio::from(gui_log));
    if options.scenario == "stress" {
        gui_command.arg("--profile");
    }
    let mut gui = match OwnedProcess::start(&mut gui_command, false) {
        Ok(gui) => gui,
        Err(error) => {
            let _ = fixture.stop(&requests_file);
            drop(fixture);
            write_fixture_log(&fixture_log_path, &output.join("fixture.log"))?;
            write_sanitized_log(&gui_log_path, &output.join("gui.log"))?;
            return Err(error);
        }
    };
    let session = (|| -> Result<()> {
        let vm_url = wait_for_vm(&gui_log_path, &mut gui, &mut fixture, &interrupt_rx)?;
        let mut probe = None;
        println!("Native GUI is ready. Evidence: {}", output.display());
        if options.scenario == "stress" {
            let project = working.path().join("stress-project");
            fs::create_dir(&project)?;
            let git = Command::new("git")
                .args(["init", "-q"])
                .arg(&project)
                .status()?;
            ensure!(
                git.success(),
                "failed to initialize isolated stress project"
            );
            let mut command = Command::new("dart");
            command
                .current_dir(&app_dir)
                .args(["run", "test_driver/stress_probe.dart", &vm_url])
                .arg(&probe_stop)
                .arg(&probe_report)
                .arg(&probe_ready)
                .arg(&probe_frames)
                .arg(&probe_finished)
                .stdout(Stdio::null())
                .stderr(Stdio::null());
            probe = Some(OwnedProcess::start(&mut command, false)?);
            let deadline = Instant::now() + Duration::from_secs(30);
            while !probe_ready.exists() {
                if let Some(process) = probe.as_mut() {
                    ensure!(
                        !process.exited()?,
                        "GUI stress probe exited before attaching"
                    );
                }
                ensure!(
                    interrupt_rx.try_recv().is_err(),
                    "GUI stress probe cancelled"
                );
                ensure!(Instant::now() < deadline, "GUI stress probe did not attach");
                thread::sleep(Duration::from_millis(100));
            }
            start_stress_turn(
                &app_dir,
                &home,
                &vm_url,
                &project,
                pl_provider_fixture::GUI_STRESS_PROMPT,
                &stress_stage,
                &interrupt_rx,
            )?;
            println!(
                "Stress prompt submitted through Flutter Driver; inspect GUI and type done after completion."
            );
        }
        println!(
            "Record actions one per line: open-project, create-thread, select-model, send-prompt, inspect-response, stop-turn, open-settings, edit-settings, save-settings, other. Type done to capture and shut down; Ctrl-C cancels. Only action codes are saved."
        );
        let (input_tx, input_rx) = mpsc::channel();
        thread::spawn(move || {
            loop {
                let mut input = String::new();
                match io::stdin().read_line(&mut input) {
                    Ok(0) | Err(_) => {
                        let _ = input_tx.send(InputEvent::Eof);
                        break;
                    }
                    Ok(_) if input.trim() == "done" => {
                        let _ = input_tx.send(InputEvent::Done);
                        break;
                    }
                    Ok(_) => {
                        let action = safe_action(&input);
                        let at_unix_ms = SystemTime::now()
                            .duration_since(UNIX_EPOCH)
                            .map_or(0, |time| time.as_millis());
                        if input_tx
                            .send(InputEvent::Action(ManualOperation { at_unix_ms, action }))
                            .is_err()
                        {
                            break;
                        }
                    }
                }
            }
        });
        let mut operations = Vec::new();
        loop {
            match input_rx.try_recv() {
                Ok(InputEvent::Action(operation)) => {
                    operations.push(operation);
                    fs::write(
                        output.join("operations.json"),
                        serde_json::to_vec_pretty(&operations)?,
                    )?;
                    continue;
                }
                Ok(InputEvent::Done) => {
                    if let Some(probe) = probe.as_mut() {
                        let deadline = Instant::now() + Duration::from_secs(120);
                        while !probe_finished.exists() {
                            ensure!(Instant::now() < deadline, "GUI stress turn did not finish");
                            ensure!(
                                !probe.exited()?,
                                "GUI stress probe exited before completion"
                            );
                            ensure!(!gui.exited()?, "GUI exited before stress completion");
                            ensure!(!fixture.exited()?, "provider fixture exited during stress");
                            ensure!(
                                interrupt_rx.try_recv().is_err(),
                                "GUI stress wait cancelled"
                            );
                            thread::sleep(Duration::from_millis(100));
                        }
                        run_stress_sessions(
                            &app_dir,
                            &vm_url,
                            &stress_sessions_report,
                            &stress_sessions_stage,
                            &interrupt_rx,
                        )?;
                        thread::sleep(Duration::from_secs(1));
                        fs::write(&probe_stop, "stop")?;
                        let deadline = Instant::now() + Duration::from_secs(25);
                        while !probe.exited()? {
                            ensure!(Instant::now() < deadline, "GUI stress probe did not stop");
                            thread::sleep(Duration::from_millis(100));
                        }
                        ensure!(probe.child.wait()?.success(), "GUI stress probe failed");
                    }
                    capture(&app_dir, &home, &vm_url, &output, &interrupt_rx)?;
                    return Ok(());
                }
                Ok(InputEvent::Eof) => bail!("manual GUI input closed before done"),
                Err(mpsc::TryRecvError::Disconnected) => bail!("manual GUI input unavailable"),
                Err(mpsc::TryRecvError::Empty) => {}
            }
            if interrupt_rx.try_recv().is_ok() {
                bail!("manual GUI cancelled; human verdict remains pending");
            }
            ensure!(!gui.exited()?, "GUI exited before manual review completed");
            ensure!(
                !fixture.exited()?,
                "provider fixture exited during manual review"
            );
            thread::sleep(Duration::from_millis(200));
        }
    })();
    let gui_log_result = write_sanitized_log(&gui_log_path, &output.join("gui.log"));
    if stress_stage.exists() {
        fs::copy(&stress_stage, output.join("stress-stage.txt"))?;
    }
    if stress_sessions_stage.exists() {
        fs::copy(
            &stress_sessions_stage,
            output.join("stress-sessions-stage.txt"),
        )?;
    }
    let sessions_log = stress_sessions_stage.with_extension("log");
    if sessions_log.exists() {
        fs::copy(sessions_log, output.join("stress-sessions.log"))?;
    }
    if stress_sessions_report.exists() {
        fs::copy(&stress_sessions_report, output.join("stress-sessions.json"))?;
    }
    let stress_screenshot = stress_stage.with_extension("png");
    if stress_screenshot.exists() {
        fs::copy(stress_screenshot, output.join("stress-start.png"))?;
    }
    let stress_tree = stress_stage.with_extension("tree");
    if stress_tree.exists() {
        fs::copy(stress_tree, output.join("stress-start.tree"))?;
    }
    if probe_report.exists() {
        fs::copy(&probe_report, output.join("probe-report.json"))?;
    }
    if probe_paging.exists() {
        fs::copy(&probe_paging, output.join("probe-paging.json"))?;
    }
    if probe_frames.exists() {
        fs::copy(&probe_frames, output.join("probe-frames.json"))?;
    }
    drop(gui);
    let fixture_result = fixture.stop(&requests_file);
    drop(fixture);
    write_fixture_log(&fixture_log_path, &output.join("fixture.log"))?;
    let stress_report: Option<pl_provider_fixture::StressReport> = if stress_report_file.exists() {
        let report = serde_json::from_slice(&fs::read(&stress_report_file)?)?;
        fs::write(
            output.join("stress-report.json"),
            serde_json::to_vec_pretty(&report)?,
        )?;
        report
    } else {
        None
    };
    let requests = if requests_file.exists() {
        sanitize_requests(&requests_file, &output.join("requests.json"))?;
        Some(serde_json::from_slice::<Vec<serde_json::Value>>(
            &fs::read(&requests_file)?,
        )?)
    } else {
        None
    };
    let accepted = requests.as_ref().map_or(0, |rows| {
        rows.iter()
            .filter(|row| row.get("accepted").and_then(|value| value.as_bool()) == Some(true))
            .count()
    });
    let rejected = requests.as_ref().map_or(0, |rows| {
        rows.iter()
            .filter(|row| row.get("accepted").and_then(|value| value.as_bool()) == Some(false))
            .count()
    });
    let fixture_complete = fixture_result.as_ref().is_ok_and(|status| status.success());
    let fixture_state = if requests.is_none() {
        "error"
    } else if rejected > 0 {
        "rejected"
    } else if fixture_complete {
        "completed"
    } else if accepted == 0 {
        "pending"
    } else {
        "error"
    };
    fs::write(
        output.join("fixture-status.json"),
        serde_json::to_vec_pretty(&serde_json::json!({
            "scenario": fixture_state, "acceptedRequests": accepted, "rejectedRequests": rejected,
            "emittedEvents": stress_report.as_ref().map(|report| report.emitted_events),
            "elapsedMillis": stress_report.as_ref().map(|report| report.elapsed_millis),
        }))?,
    )?;
    println!("Evidence: {} (human verdict pending)", output.display());
    gui_log_result?;
    if fixture_state == "pending" {
        session?;
        println!("Fixture main step was not exercised; review remains pending.");
        return Ok(());
    }
    session?;
    ensure!(
        fixture_state == "completed",
        "fixture {fixture_state}; verdict remains pending"
    );
    if options.scenario == "stress" {
        // The probe must have really moved the typed content window toward older
        // content and saturated it at its capacity. Its own reported `capacity`
        // is the shared bound for the per-session window checks below.
        let paging: ProbePagingReport = serde_json::from_slice(
            &fs::read(&probe_paging)
                .context("GUI stress probe did not write its window-paging evidence")?,
        )?;
        ensure!(
            paging.paged
                && paging.page_count >= 1
                && paging.saturated
                && paging.final_items == Some(paging.capacity)
                && paging.moved_older
                && paging.identity_retained
                && paging.anchor_retained
                && paging.all_within_capacity
                && paging.all_unique,
            "GUI stress window never moved to an older page: {}",
            serde_json::json!({
                "capacity": paging.capacity,
                "paged": paging.paged,
                "pageCount": paging.page_count,
                "finalItems": paging.final_items,
                "saturated": paging.saturated,
                "movedOlder": paging.moved_older,
                "identityRetained": paging.identity_retained,
                "anchorRetained": paging.anchor_retained,
                "allWithinCapacity": paging.all_within_capacity,
                "allUnique": paging.all_unique,
            })
        );
        let sessions: StressSessionsReport = serde_json::from_slice(
            &fs::read(&stress_sessions_report)
                .context("Flutter Driver did not complete the multi-session journey")?,
        )?;
        ensure!(
            sessions.original_reopened
                && !sessions.original_thread_id.is_empty()
                && sessions.original_window_items <= paging.capacity
                && sessions.session_count == pl_provider_fixture::GUI_STRESS_SESSION_COUNT
                && sessions.sessions.len() == sessions.session_count
                && sessions.directory_count > sessions.session_count,
            "multi-session stress journey incomplete"
        );
        let mut ids = HashSet::from([sessions.original_thread_id]);
        for (index, session) in sessions.sessions.iter().enumerate() {
            ensure!(
                session.ordinal == index + 1
                    && session.window_items <= paging.capacity
                    && ids.insert(session.thread_id.clone()),
                "multi-session stress item {} invalid",
                index + 1
            );
        }
        // The large first-session assistant body must be delivered whole: it may
        // not fall back to a lazy preview or await a manual full-body load, and
        // its length must match the strict fixture. Truncating it to a bounded
        // preview was the removed Dart 8 KiB behaviour and is deliberately not a
        // passing condition any more.
        let large = &sessions.sessions[0];
        ensure!(
            large.previewed_bodies == 0 && large.pending_bodies == 0,
            "large assistant body must be delivered whole without a manual load, \
             but the first session reported {} previewed and {} pending items",
            large.previewed_bodies,
            large.pending_bodies
        );
        ensure!(
            large.assistant_characters
                == Some(pl_provider_fixture::GUI_STRESS_LARGE_SESSION_CHARACTERS),
            "large assistant body incomplete: {:?} of {} code units",
            large.assistant_characters,
            pl_provider_fixture::GUI_STRESS_LARGE_SESSION_CHARACTERS
        );
        let report = stress_report.context("stress fixture did not emit a report")?;
        let gui_errors = count_gui_errors(&gui_log_path)?;
        let debug_log = output.join("gui-debug.log");
        fs::copy(&gui_log_path, &debug_log)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&debug_log, fs::Permissions::from_mode(0o600))?;
        }
        fs::write(
            output.join("gui-health.json"),
            serde_json::to_vec_pretty(&serde_json::json!({
                "unhandledErrors": gui_errors, "humanVerdict": "pending"
            }))?,
        )?;
        ensure!(
            gui_errors == 0,
            "GUI reported {gui_errors} unhandled errors during stress"
        );
        ensure!(
            report.emitted_events == pl_provider_fixture::STRESS_EVENT_COUNT,
            "stress stream incomplete: {}/{} events",
            report.emitted_events,
            pl_provider_fixture::STRESS_EVENT_COUNT
        );
        ensure!(
            report.elapsed_millis >= 3_800,
            "stress stream ran faster than 5,000 nominal tokens/s"
        );
        ensure!(
            report.elapsed_millis <= 10_000,
            "stress stream fell behind the 5,000 token/s fixture: {}ms for {} items",
            report.elapsed_millis,
            report.emitted_events
        );
        let finished = report
            .finished_unix_millis
            .context("stress stream did not finish")?;
        let gui_finished: u128 = fs::read_to_string(&probe_finished)?
            .trim()
            .parse()
            .context("GUI stress probe did not report its completion time")?;
        let completion_lag = gui_finished.saturating_sub(finished);
        ensure!(
            completion_lag <= 15_000,
            "GUI stress result lagged provider completion by {completion_lag}ms"
        );
        let samples: Vec<serde_json::Value> = serde_json::from_slice(
            &fs::read(&probe_report).context("GUI stress probe did not write a report")?,
        )?;
        let overlapping: Vec<_> = samples
            .iter()
            .filter(|sample| {
                sample
                    .get("startedUnixMillis")
                    .and_then(serde_json::Value::as_u64)
                    .is_some_and(|time| {
                        u128::from(time) >= report.started_unix_millis
                            && u128::from(time) < finished
                    })
            })
            .collect();
        let failed = overlapping
            .iter()
            .filter(|sample| sample.get("ok") != Some(&serde_json::Value::Bool(true)))
            .count();
        let slowest = overlapping
            .iter()
            .filter_map(|sample| {
                sample
                    .get("elapsedMillis")
                    .and_then(serde_json::Value::as_u64)
            })
            .max()
            .unwrap_or(0);
        let frames: serde_json::Value = serde_json::from_slice(
            &fs::read(&probe_frames).context("GUI stress probe did not write frame timings")?,
        )?;
        let all_frames = frames
            .get("frames")
            .and_then(serde_json::Value::as_u64)
            .context("GUI total frame count is unavailable")?;
        let all_slow_frames = frames
            .get("over33Millis")
            .and_then(serde_json::Value::as_u64)
            .context("GUI total slow frame count is unavailable")?;
        let all_slowest_frame = frames
            .get("maxFrameMillis")
            .and_then(serde_json::Value::as_f64)
            .context("GUI longest frame is unavailable")?;
        let frame_samples = frames
            .get("samples")
            .and_then(serde_json::Value::as_array)
            .context("GUI frame timing samples are unavailable")?;
        let stream_frames: Vec<f64> = frame_samples
            .iter()
            .filter(|sample| {
                sample
                    .get("completedUnixMillis")
                    .and_then(serde_json::Value::as_u64)
                    .is_some_and(|time| {
                        u128::from(time) >= report.started_unix_millis
                            && u128::from(time) < finished.saturating_add(1_000)
                    })
            })
            .filter_map(|sample| {
                sample
                    .get("totalMillis")
                    .and_then(serde_json::Value::as_f64)
            })
            .collect();
        let slow_frames = stream_frames.iter().filter(|&&ms| ms > 33.333).count();
        let slowest_frame = stream_frames.iter().copied().fold(0.0_f64, f64::max);
        fs::write(
            output.join("responsiveness.json"),
            serde_json::to_vec_pretty(&serde_json::json!({
                "samplesDuringStream": overlapping.len(), "failed": failed,
                "slowestSnapshotMillis": slowest,
                "providerStreamMillis": report.elapsed_millis,
                "guiCompletionLagMillis": completion_lag,
                "streamAndDrainFrames": stream_frames.len(),
                "streamAndDrainOver33Millis": slow_frames,
                "streamAndDrainMaxFrameMillis": slowest_frame,
                "fullSessionFrameTimings": {
                    "frames": frames.get("frames"),
                    "over33Millis": frames.get("over33Millis"),
                    "maxFrameMillis": frames.get("maxFrameMillis"),
                },
                "verdict": "pending"
            }))?,
        )?;
        ensure!(
            stream_frames.len() >= 5,
            "GUI did not report enough frames during the stress stream"
        );
        ensure!(
            all_frames >= 20 && all_slowest_frame <= 100.0 && all_slow_frames <= all_frames / 20,
            "GUI full stress journey stalled: max {all_slowest_frame:.1}ms, {all_slow_frames}/{all_frames} over 33ms"
        );
        ensure!(
            slowest_frame <= 100.0 && slow_frames * 20 <= stream_frames.len(),
            "GUI stream frames stalled: max {slowest_frame:.1}ms, {slow_frames}/{} over 33ms",
            stream_frames.len()
        );
        ensure!(
            overlapping.len() >= 5,
            "GUI was not probed during stress stream"
        );
        ensure!(
            failed == 0,
            "GUI snapshot probe failed {failed} times during stress stream"
        );
        ensure!(
            slowest <= 2_000,
            "GUI snapshot stalled for {slowest}ms during stress stream"
        );
    }
    Ok(())
}

fn safe_action(line: &str) -> &'static str {
    match line.split_whitespace().next().unwrap_or("") {
        "open-project" => "open-project",
        "create-thread" => "create-thread",
        "select-model" => "select-model",
        "send-prompt" => "send-prompt",
        "inspect-response" => "inspect-response",
        "stop-turn" => "stop-turn",
        "open-settings" => "open-settings",
        "edit-settings" => "edit-settings",
        "save-settings" => "save-settings",
        _ => "other",
    }
}

fn wait_for_ready(
    path: &Path,
    fixture: &mut OwnedProcess,
    interrupt: &mpsc::Receiver<()>,
) -> Result<FixtureReady> {
    let deadline = Instant::now() + Duration::from_secs(90);
    loop {
        if path.is_file() {
            let ready: FixtureReady = serde_json::from_slice(&fs::read(path)?).context(
                "fixture ready-file must contain base_url, ws_url and scenario JSON fields",
            )?;
            validate_ready(&ready)?;
            return Ok(ready);
        }
        ensure!(
            !fixture.exited()?,
            "fixture exited before writing ready-file"
        );
        ensure!(
            interrupt.try_recv().is_err(),
            "manual GUI cancelled while starting fixture"
        );
        ensure!(Instant::now() < deadline, "fixture ready-file timed out");
        thread::sleep(Duration::from_millis(200));
    }
}

fn validate_ready(ready: &FixtureReady) -> Result<()> {
    let remainder = ready
        .base_url
        .strip_prefix("http://127.0.0.1:")
        .context("fixture base_url must use IPv4 loopback")?;
    let (port, path) = remainder
        .split_once('/')
        .context("fixture base_url must end with /v1")?;
    ensure!(
        port.parse::<u16>().is_ok_and(|port| port != 0) && path == "v1",
        "fixture base_url must be http://127.0.0.1:<port>/v1"
    );
    ensure!(
        pl_provider_fixture::GUI_SCENARIOS.contains(&ready.scenario.as_str()),
        "unsupported fixture scenario"
    );
    ensure!(
        ready.ws_url == ready.base_url.replacen("http://", "ws://", 1),
        "fixture ws_url must use the same loopback endpoint"
    );
    Ok(())
}

fn write_config(home: &Path, ready: &FixtureReady) -> Result<()> {
    let mut config = StudioConfig::default_config();
    let mut model = ModelInfo::compatible("fixture-model");
    model.display_name = "Local GUI fixture".into();
    model
        .binding
        .set_transport(ModelTransportProfile::responses_http());
    model.binding.request.api_model = None;
    let provider_id = ProviderId::new(format!("gui-fixture-{}", std::process::id()))?;
    let provider = ProviderConfig::from_explicit_models(
        ProviderEndpoint::compatible("Local GUI fixture", &ready.base_url),
        vec![model],
    );
    config.models.providers.clear();
    config
        .models
        .providers
        .insert(provider_id.clone(), provider);
    for route in config
        .models
        .routes
        .values_mut()
        .chain(config.mode_model_routes.values_mut())
    {
        route.provider = provider_id.clone();
        route.model = "fixture-model".into();
        route.effort = None;
    }
    config.skills.user_dir = home.join("skills").to_string_lossy().into_owned();
    config.validate()?;
    let file = home.join("config.toml");
    fs::write(&file, toml::to_string_pretty(&config)?)
        .with_context(|| format!("failed to create isolated config: {}", file.display()))
}

fn wait_for_vm(
    path: &Path,
    gui: &mut OwnedProcess,
    fixture: &mut OwnedProcess,
    interrupt: &mpsc::Receiver<()>,
) -> Result<String> {
    let deadline = Instant::now() + Duration::from_secs(600);
    loop {
        let log = fs::read_to_string(path)?;
        if let Some(url) = log
            .lines()
            .filter_map(|line| {
                line.split("available at: http://127.0.0.1:")
                    .nth(1)
                    .map(|tail| {
                        format!(
                            "http://127.0.0.1:{}",
                            tail.split_whitespace().next().unwrap_or("")
                        )
                    })
            })
            .next_back()
        {
            return Ok(url);
        }
        ensure!(!gui.exited()?, "GUI exited before publishing VM service");
        ensure!(
            !fixture.exited()?,
            "provider fixture exited while GUI started"
        );
        ensure!(
            interrupt.try_recv().is_err(),
            "manual GUI cancelled while starting GUI"
        );
        ensure!(
            Instant::now() < deadline,
            "GUI VM service startup timed out"
        );
        thread::sleep(Duration::from_millis(300));
    }
}

fn capture(
    app_dir: &Path,
    home: &Path,
    vm_url: &str,
    output: &Path,
    interrupt: &mpsc::Receiver<()>,
) -> Result<()> {
    let mut command = Command::new("dart");
    command
        .current_dir(app_dir)
        .args(["run", "test_driver/manual_capture.dart", vm_url])
        .arg(output)
        .env("ANYWORK_HOME", home)
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    let mut capture = OwnedProcess::start(&mut command, false)?;
    let deadline = Instant::now() + Duration::from_secs(120);
    loop {
        if let Some(status) = capture.child.try_wait()? {
            ensure!(
                status.success(),
                "Driver capture or shutdown failed; human verdict pending"
            );
            return Ok(());
        }
        ensure!(
            interrupt.try_recv().is_err(),
            "manual GUI cancelled while capturing evidence"
        );
        ensure!(
            Instant::now() < deadline,
            "Driver capture or shutdown timed out"
        );
        thread::sleep(Duration::from_millis(200));
    }
}

fn start_stress_turn(
    app_dir: &Path,
    home: &Path,
    vm_url: &str,
    project: &Path,
    prompt: &str,
    stage: &Path,
    interrupt: &mpsc::Receiver<()>,
) -> Result<()> {
    let mut command = Command::new("dart");
    command
        .current_dir(app_dir)
        .args(["run", "test_driver/stress_start.dart", vm_url])
        .arg(project)
        .arg(prompt)
        .arg(stage)
        .env("ANYWORK_HOME", home)
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    let mut drive = OwnedProcess::start(&mut command, false)?;
    let deadline = Instant::now() + Duration::from_secs(180);
    loop {
        if let Some(status) = drive.child.try_wait()? {
            ensure!(
                status.success(),
                "Flutter Driver could not submit the stress prompt (last stage: {})",
                fs::read_to_string(stage).unwrap_or_else(|_| "connect".to_owned())
            );
            drive.stopped = true;
            return Ok(());
        }
        ensure!(interrupt.try_recv().is_err(), "stress startup cancelled");
        ensure!(
            Instant::now() < deadline,
            "stress GUI actions timed out (last stage: {})",
            fs::read_to_string(stage).unwrap_or_else(|_| "connect".to_owned())
        );
        thread::sleep(Duration::from_millis(100));
    }
}

fn run_stress_sessions(
    app_dir: &Path,
    vm_url: &str,
    report: &Path,
    stage: &Path,
    interrupt: &mpsc::Receiver<()>,
) -> Result<()> {
    let mut command = Command::new("dart");
    command
        .current_dir(app_dir)
        .args(["run", "test_driver/stress_sessions.dart", vm_url])
        .arg(pl_provider_fixture::GUI_STRESS_SESSION_COUNT.to_string())
        .arg(pl_provider_fixture::GUI_STRESS_FOLLOWUP_PROMPT_PREFIX)
        .arg(report)
        .arg(stage)
        .stdout(Stdio::null())
        .stderr(Stdio::from(File::create(stage.with_extension("log"))?));
    let mut drive = OwnedProcess::start(&mut command, false)?;
    let deadline = Instant::now() + Duration::from_secs(600);
    loop {
        if let Some(status) = drive.child.try_wait()? {
            ensure!(
                status.success(),
                "Flutter Driver multi-session journey failed (last stage: {})",
                fs::read_to_string(stage).unwrap_or_else(|_| "connect".to_owned())
            );
            drive.stopped = true;
            return Ok(());
        }
        ensure!(interrupt.try_recv().is_err(), "stress sessions cancelled");
        ensure!(
            Instant::now() < deadline,
            "stress GUI sessions timed out (last stage: {})",
            fs::read_to_string(stage).unwrap_or_else(|_| "connect".to_owned())
        );
        thread::sleep(Duration::from_millis(100));
    }
}

fn run_statistics_scenario(
    context: &StatisticsScenario<'_>,
    mut fixture: OwnedProcess,
) -> Result<()> {
    let StatisticsScenario {
        workspace,
        home,
        working,
        output,
        fixture_log,
        requests_file,
        report_file,
        interrupt,
        ..
    } = context;
    let first_log = working.join("statistics-first-gui.log");
    let restart_log = working.join("statistics-restart-gui.log");
    let stage = working.join("statistics-stage");
    let project = working.join("statistics-project");
    let journey = (|| -> Result<()> {
        fs::create_dir(&project)?;
        ensure!(
            Command::new("git")
                .args(["init", "-q"])
                .arg(&project)
                .status()?
                .success(),
            "failed to initialize isolated statistics project"
        );
        {
            let mut gui = start_statistics_gui(workspace, home, &first_log)?;
            let vm_url = wait_for_vm(&first_log, &mut gui, &mut fixture, interrupt)?;
            let mut driver = start_statistics_driver(context, &vm_url, "first", &project, &stage)?;
            wait_for_statistics_stage(
                "ready_lock",
                &stage,
                &mut driver,
                &mut gui,
                &mut fixture,
                interrupt,
                Duration::from_secs(120),
            )?;
            let calls_db = home.join("v2/calls/calls.sqlite");
            ensure!(
                calls_db.is_file(),
                "GUI did not initialize isolated calls.sqlite"
            );
            let mut lock =
                SqliteWriteLock::acquire(&calls_db, "calls.sqlite", Duration::from_secs(90))?;
            fs::write(working.join("lock-acquired"), "acquired")?;
            wait_for_statistics_stage(
                "lock_observed",
                &stage,
                &mut driver,
                &mut gui,
                &mut fixture,
                interrupt,
                Duration::from_secs(60),
            )?;
            ensure!(
                working.join("fast-submitted").is_file(),
                "fast prompt was not submitted under SQLite lock"
            );
            lock.finish()?;
            fs::write(working.join("lock-released"), "released")?;
            wait_for_statistics_driver(
                "first",
                &stage,
                &mut driver,
                &mut gui,
                &mut fixture,
                interrupt,
            )?;
            // The Driver's native shutdown has completed; terminate and reap the
            // resident process group before reopening this exact isolated home.
        }
        let before = statistics_db_counts(home)?;
        ensure!(
            before.committed_calls >= 2 && before.performance_samples >= 1,
            "statistics calls.sqlite did not record both calls: {before:?}"
        );
        fs::write(
            output.join("statistics-db-before-restart.json"),
            serde_json::to_vec_pretty(&before)?,
        )?;
        {
            let mut gui = start_statistics_gui(workspace, home, &restart_log)?;
            let vm_url = wait_for_vm(&restart_log, &mut gui, &mut fixture, interrupt)?;
            let mut driver =
                start_statistics_driver(context, &vm_url, "restart", &project, &stage)?;
            wait_for_statistics_driver(
                "restart",
                &stage,
                &mut driver,
                &mut gui,
                &mut fixture,
                interrupt,
            )?;
        }
        let after = statistics_db_counts(home)?;
        fs::write(
            output.join("statistics-db-after-restart.json"),
            serde_json::to_vec_pretty(&after)?,
        )?;
        ensure!(
            before == after,
            "calls.sqlite changed across restart: {before:?} -> {after:?}"
        );
        let final_state: serde_json::Value =
            serde_json::from_slice(&fs::read(output.join("statistics-final.json"))?)?;
        let restarted: serde_json::Value =
            serde_json::from_slice(&fs::read(output.join("statistics-restarted.json"))?)?;
        ensure!(
            final_state["history"] == restarted["history"]
                && final_state["samples"] == restarted["samples"]
                && restarted["statisticsPending"] == false
                && restarted["statisticsGap"] == false
                && restarted["readFailed"] == false,
            "statistics projection changed or remained unhealthy after restart"
        );
        Ok(())
    })();

    for (source, name) in [
        (&first_log, "statistics-first-gui.log"),
        (&restart_log, "statistics-restart-gui.log"),
    ] {
        if source.exists() {
            write_sanitized_log(source, &output.join(name))?;
        }
    }
    let driver_log = stage.with_extension("driver.log");
    if driver_log.is_file() {
        let diagnostics = fs::read_to_string(driver_log)?
            .lines()
            .filter(|line| {
                [
                    "Unhandled exception:",
                    "Bad state:",
                    "StateError:",
                    "TimeoutException",
                    "DriverError",
                    "FormatException",
                ]
                .iter()
                .any(|prefix| line.starts_with(prefix))
                    && !line.contains("http://")
                    && !line.contains("https://")
            })
            .map(|line| line.chars().take(240).collect::<String>())
            .collect::<Vec<_>>();
        fs::write(
            output.join("statistics-driver-errors.txt"),
            diagnostics.join("\n"),
        )?;
    }
    if stage.is_file() {
        let value = fs::read_to_string(&stage)?;
        if value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
        {
            fs::write(output.join("statistics-stage.txt"), value)?;
        }
    }
    let fixture_result = fixture.stop(requests_file);
    drop(fixture);
    write_fixture_log(fixture_log, &output.join("fixture.log"))?;
    let requests = if requests_file.is_file() {
        sanitize_requests(requests_file, &output.join("requests.json"))?;
        serde_json::from_slice::<Vec<serde_json::Value>>(&fs::read(requests_file)?)?
    } else {
        Vec::new()
    };
    let accepted = requests
        .iter()
        .filter(|row| row["accepted"] == true)
        .count();
    let rejected = requests
        .iter()
        .filter(|row| row["accepted"] == false)
        .count();
    let stream: Option<pl_provider_fixture::StressReport> = if report_file.is_file() {
        serde_json::from_slice(&fs::read(report_file)?)?
    } else {
        None
    };
    fs::write(
        output.join("fixture-status.json"),
        serde_json::to_vec_pretty(&serde_json::json!({
            "scenario": "statistics", "acceptedRequests": accepted, "rejectedRequests": rejected,
            "emittedEvents": stream.as_ref().map(|row| row.emitted_events),
            "elapsedMillis": stream.as_ref().map(|row| row.elapsed_millis),
            "completed": fixture_result.as_ref().is_ok_and(|status| status.success()),
        }))?,
    )?;
    let errors = [first_log, restart_log]
        .iter()
        .filter(|path| path.is_file())
        .map(|path| count_gui_errors(path))
        .collect::<Result<Vec<_>>>()?
        .into_iter()
        .sum::<usize>();
    fs::write(
        output.join("gui-health.json"),
        serde_json::to_vec_pretty(&serde_json::json!({
            "unhandledErrors": errors, "humanVerdict": "pending"
        }))?,
    )?;
    println!("Evidence: {} (human verdict pending)", output.display());
    journey?;
    ensure!(
        fixture_result.is_ok_and(|status| status.success())
            && accepted >= 2
            && rejected == 0
            && stream.as_ref().is_some_and(|row| {
                row.emitted_events == pl_provider_fixture::GUI_STATISTICS_PACED_EVENTS
                    && row.elapsed_millis >= 800
            })
            && errors == 0,
        "statistics fixture or GUI health incomplete; human verdict pending"
    );
    Ok(())
}

fn start_statistics_gui(workspace: &Path, home: &Path, log_path: &Path) -> Result<OwnedProcess> {
    let log = File::create(log_path)?;
    let mut command = Command::new("cargo");
    command
        .current_dir(workspace)
        .args(["xtask", "run-gui", "--driver"])
        .env("ANYWORK_HOME", home)
        .stdout(Stdio::from(log.try_clone()?))
        .stderr(Stdio::from(log));
    OwnedProcess::start(&mut command, false)
}

fn start_statistics_driver(
    context: &StatisticsScenario<'_>,
    vm_url: &str,
    phase: &str,
    project: &Path,
    stage: &Path,
) -> Result<OwnedProcess> {
    let StatisticsScenario {
        app_dir,
        home,
        output,
        working,
        ..
    } = context;
    let log = File::create(stage.with_extension("driver.log"))?;
    let expected = if phase == "restart" {
        let final_state: serde_json::Value =
            serde_json::from_slice(&fs::read(output.join("statistics-final.json"))?)?;
        final_state["history"]
            .as_u64()
            .context("final history count unavailable")?
            .to_string()
    } else {
        "0".to_owned()
    };
    let mut command = Command::new("dart");
    command
        .current_dir(app_dir)
        .args(["run", "test_driver/statistics_journey.dart", phase, vm_url])
        .arg(project)
        .arg(output)
        .arg(working)
        .arg(expected)
        .env("ANYWORK_HOME", home)
        .stdout(Stdio::from(log.try_clone()?))
        .stderr(Stdio::from(log));
    OwnedProcess::start(&mut command, false)
}

fn wait_for_statistics_stage(
    expected: &str,
    stage: &Path,
    driver: &mut OwnedProcess,
    gui: &mut OwnedProcess,
    fixture: &mut OwnedProcess,
    interrupt: &mpsc::Receiver<()>,
    timeout: Duration,
) -> Result<()> {
    let deadline = Instant::now() + timeout;
    loop {
        if stage.is_file() && fs::read_to_string(stage)? == expected {
            return Ok(());
        }
        if let Some(status) = driver.child.try_wait()? {
            bail!("statistics Driver exited before {expected}: {status}");
        }
        ensure!(!gui.exited()?, "GUI exited before {expected}");
        ensure!(
            !fixture.exited()?,
            "provider fixture exited before {expected}"
        );
        ensure!(
            interrupt.try_recv().is_err(),
            "statistics journey cancelled"
        );
        ensure!(
            Instant::now() < deadline,
            "statistics Driver timed out before {expected}"
        );
        thread::sleep(Duration::from_millis(100));
    }
}

fn wait_for_statistics_driver(
    phase: &str,
    stage: &Path,
    driver: &mut OwnedProcess,
    gui: &mut OwnedProcess,
    fixture: &mut OwnedProcess,
    interrupt: &mpsc::Receiver<()>,
) -> Result<()> {
    let expected = if phase == "restart" {
        "restart_shutdown"
    } else {
        "first_shutdown"
    };
    wait_for_statistics_stage(
        expected,
        stage,
        driver,
        gui,
        fixture,
        interrupt,
        Duration::from_secs(240),
    )?;
    let status = driver.child.wait()?;
    driver.stopped = true;
    ensure!(
        status.success(),
        "statistics Driver failed after {expected}"
    );
    Ok(())
}

fn statistics_db_counts(home: &Path) -> Result<StatisticsDbCounts> {
    let database = home.join("v2/calls/calls.sqlite");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    runtime.block_on(async move {
        let options = SqliteConnectOptions::new()
            .filename(&database)
            .read_only(true);
        let mut connection = SqliteConnection::connect_with(&options).await?;
        let (rows, committed, samples): (i64, i64, i64) = sea_orm::sqlx::query_as(
            "SELECT COUNT(*),
                    COALESCE(SUM(CASE WHEN terminal=1 AND status='committed' THEN 1 ELSE 0 END),0),
                    COALESCE(SUM(CASE WHEN terminal=1 AND status='committed' AND decode_millis>0
                        AND output_tokens IS NOT NULL THEN 1 ELSE 0 END),0)
             FROM model_calls",
        )
        .fetch_one(&mut connection)
        .await
        .with_context(|| format!("failed to count model calls in {}", database.display()))?;
        Ok(StatisticsDbCounts {
            model_calls: rows.try_into()?,
            committed_calls: committed.try_into()?,
            performance_samples: samples.try_into()?,
        })
    })
}

/// Non-optional steps in the realtime fixture script.
///
/// The fixture only verifies after every one of them is consumed, so a
/// successful run accepts at least this many requests; the count also fails the
/// journey if the script silently stops exercising the realtime states. A tool
/// command is more than one request: the assistant `exec` call, the scripted
/// model's supported `wait` on the task receipt, and the step that observes the
/// delivered result (the two parallel commands add one extra `wait` for the
/// still-running task, and the approved command is gated behind consent).
const REALTIME_REQUIRED_STEPS: usize = 15;

fn run_realtime_scenario(context: &RealtimeScenario<'_>, mut fixture: OwnedProcess) -> Result<()> {
    let RealtimeScenario {
        workspace,
        app_dir,
        home,
        working,
        output,
        fixture_log,
        requests_file,
        interrupt,
        // `scenario` stays on `context`; it is only read via `context.scenario`.
        ..
    } = context;
    let gui_log_path = working.join("realtime-gui.log");
    let stage = working.join("realtime-stage");
    let project = working.join("realtime-project");
    let mut driver_exit: Option<std::process::ExitStatus> = None;
    let journey = (|| -> Result<()> {
        fs::create_dir(&project)?;
        ensure!(
            Command::new("git")
                .args(["init", "-q"])
                .arg(&project)
                .status()?
                .success(),
            "failed to initialize isolated realtime project"
        );
        {
            let mut gui = start_realtime_gui(workspace, home, &gui_log_path)?;
            let vm_url = wait_for_vm(&gui_log_path, &mut gui, &mut fixture, interrupt)?;
            let mut driver =
                start_realtime_driver(app_dir, home, &vm_url, &project, output, working, &stage)?;
            let status =
                wait_for_realtime_driver(&stage, &mut driver, &mut gui, &mut fixture, interrupt)?;
            driver_exit = Some(status);
            ensure!(
                status.success(),
                "realtime Driver failed with {status} (last stage: {})",
                // No stage file means the Dart script never reached `main`
                // (compile or launch error); name that instead of guessing.
                fs::read_to_string(&stage).unwrap_or_else(|_| {
                    "driver_not_started (see realtime-driver-errors.txt)".to_owned()
                })
            );
        }
        Ok(())
    })();

    if gui_log_path.is_file() {
        write_sanitized_log(&gui_log_path, &output.join("realtime-gui.log"))?;
    }
    // The Driver log lives under the isolated coordination directory, which is
    // removed with the run: persist a sanitized copy plus an error digest before
    // it disappears, so a Dart compile error or connection failure is always
    // reviewable and never leaves an empty error file.
    write_realtime_driver_evidence(
        &stage.with_extension("driver.log"),
        output,
        "realtime",
        driver_exit,
        journey.as_ref().err(),
        &[
            (app_dir.to_string_lossy().into_owned(), "<app>"),
            (workspace.to_string_lossy().into_owned(), "<workspace>"),
            (home.to_string_lossy().into_owned(), "<home>"),
            (working.to_string_lossy().into_owned(), "<coord>"),
        ],
    )?;
    if stage.is_file() {
        let value = fs::read_to_string(&stage)?;
        if value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'.'))
        {
            fs::write(output.join("realtime-stage.txt"), value)?;
        }
    }

    let fixture_result = fixture.stop(requests_file);
    drop(fixture);
    write_fixture_log(fixture_log, &output.join("fixture.log"))?;
    let requests = if requests_file.is_file() {
        sanitize_requests(requests_file, &output.join("requests.json"))?;
        serde_json::from_slice::<Vec<serde_json::Value>>(&fs::read(requests_file)?)?
    } else {
        Vec::new()
    };
    let accepted = requests
        .iter()
        .filter(|row| row["accepted"] == true)
        .count();
    let rejected = requests
        .iter()
        .filter(|row| row["accepted"] == false)
        .count();
    fs::write(
        output.join("fixture-status.json"),
        serde_json::to_vec_pretty(&serde_json::json!({
            "scenario": "realtime", "acceptedRequests": accepted, "rejectedRequests": rejected,
            "completed": fixture_result.as_ref().is_ok_and(|status| status.success()),
        }))?,
    )?;
    let errors = if gui_log_path.is_file() {
        count_gui_errors(&gui_log_path)?
    } else {
        0
    };
    fs::write(
        output.join("gui-health.json"),
        serde_json::to_vec_pretty(&serde_json::json!({
            "unhandledErrors": errors, "humanVerdict": "pending"
        }))?,
    )?;
    println!("Evidence: {} (human verdict pending)", output.display());
    journey?;
    let summary: serde_json::Value = serde_json::from_slice(
        &fs::read(output.join("realtime-summary.json"))
            .context("Flutter Driver did not write the realtime journey summary")?,
    )?;
    ensure!(
        summary["scenario"] == "realtime" && summary["verdict"] == "pending",
        "realtime journey summary is missing its scenario or pending verdict"
    );
    let checks = summary["checks"]
        .as_object()
        .context("realtime journey summary has no checks")?;
    for (name, value) in checks {
        ensure!(
            value == &serde_json::Value::Bool(true),
            "realtime journey check {name} did not pass"
        );
    }
    ensure!(!checks.is_empty(), "realtime journey recorded no checks");
    // The long-command in-flight proof keys off the integrated typed timeline
    // tool trio (output/progressBytes/itemRevision). A report that omits it is
    // never accepted as a pass: the missing diagnostic is a gap, not a success.
    ensure!(
        checks.get("longCommandInFlightOutputGrows") == Some(&serde_json::Value::Bool(true)),
        "realtime journey did not prove the long-command in-flight output growth \
         (longCommandInFlightOutputGrows missing; see pendingEvidence)"
    );
    ensure!(
        fixture_result.is_ok_and(|status| status.success())
            && accepted >= REALTIME_REQUIRED_STEPS
            && rejected == 0
            && errors == 0,
        "realtime fixture or GUI health incomplete; human verdict pending"
    );
    Ok(())
}

/// Single-item long-body stress journey driven entirely through Flutter Driver.
///
/// The fixture appends every increment to one stable item/part with the same
/// event count and nominal rate as the multi-item stress script, so the load is
/// comparable while the evidence is recorded separately. The `stress-body-large`
/// scenario keeps the same event count but pads each increment above the 256KiB
/// native window and slows the paced rate only there, so the generation-time
/// growth above the threshold can be sampled. The synthetic provider's byte
/// count is reported as provider output bytes only; it is never presented as the
/// volume transferred across the bridge.
fn run_stress_body_scenario(
    context: &StressBodyScenario<'_>,
    mut fixture: OwnedProcess,
) -> Result<()> {
    let StressBodyScenario {
        workspace,
        app_dir,
        home,
        working,
        output,
        fixture_log,
        requests_file,
        interrupt,
        ..
    } = context;
    let gui_log_path = working.join("stress-body-gui.log");
    let stage = working.join("stress-body-stage");
    let project = working.join("stress-body-project");
    let report_file = working.join("stress-report.json");
    let mut driver_exit: Option<std::process::ExitStatus> = None;
    let journey = (|| -> Result<()> {
        fs::create_dir(&project)?;
        ensure!(
            Command::new("git")
                .args(["init", "-q"])
                .arg(&project)
                .status()?
                .success(),
            "failed to initialize isolated stress-body project"
        );
        {
            let mut gui = start_stress_body_gui(workspace, home, &gui_log_path)?;
            let vm_url = wait_for_vm(&gui_log_path, &mut gui, &mut fixture, interrupt)?;
            let mut driver = start_stress_body_driver(context, &vm_url, &project, &stage)?;
            let status =
                wait_for_realtime_driver(&stage, &mut driver, &mut gui, &mut fixture, interrupt)?;
            driver_exit = Some(status);
            ensure!(
                status.success(),
                "stress-body Driver failed with {status} (last stage: {})",
                fs::read_to_string(&stage).unwrap_or_else(|_| {
                    "driver_not_started (see stress-body-driver-errors.txt)".to_owned()
                })
            );
        }
        Ok(())
    })();

    if gui_log_path.is_file() {
        write_sanitized_log(&gui_log_path, &output.join("stress-body-gui.log"))?;
    }
    write_realtime_driver_evidence(
        &stage.with_extension("driver.log"),
        output,
        context.scenario,
        driver_exit,
        journey.as_ref().err(),
        &[
            (app_dir.to_string_lossy().into_owned(), "<app>"),
            (workspace.to_string_lossy().into_owned(), "<workspace>"),
            (home.to_string_lossy().into_owned(), "<home>"),
            (working.to_string_lossy().into_owned(), "<coord>"),
        ],
    )?;
    if stage.is_file() {
        let value = fs::read_to_string(&stage)?;
        if value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'.'))
        {
            fs::write(output.join("stress-body-stage.txt"), value)?;
        }
    }
    let fixture_result = fixture.stop(requests_file);
    drop(fixture);
    // The fixture only writes its `--report-file` while shutting down, so the
    // stress report is read after the graceful stop, never before it.
    let host_report = if report_file.is_file() {
        let report: Option<pl_provider_fixture::StressReport> =
            serde_json::from_slice(&fs::read(&report_file)?)?;
        report
    } else {
        None
    };
    write_fixture_log(fixture_log, &output.join("fixture.log"))?;
    let requests = if requests_file.is_file() {
        sanitize_requests(requests_file, &output.join("requests.json"))?;
        serde_json::from_slice::<Vec<serde_json::Value>>(&fs::read(requests_file)?)?
    } else {
        Vec::new()
    };
    let accepted = requests
        .iter()
        .filter(|row| row["accepted"] == true)
        .count();
    let rejected = requests
        .iter()
        .filter(|row| row["accepted"] == false)
        .count();
    fs::write(
        output.join("fixture-status.json"),
        serde_json::to_vec_pretty(&serde_json::json!({
            "scenario": context.scenario,
            "acceptedRequests": accepted,
            "rejectedRequests": rejected,
            "completed": fixture_result.as_ref().is_ok_and(|status| status.success()),
            "providerEmittedEvents": host_report.as_ref().map(|report| report.emitted_events),
            // Synthetic provider text bytes; explicitly not an FRB transfer measurement.
            "providerOutputBytes": host_report.as_ref().map(|report| report.emitted_bytes),
            "providerOutputBytesNote": "synthetic provider output text bytes, not FRB transfer volume",
        }))?,
    )?;
    let errors = if gui_log_path.is_file() {
        count_gui_errors(&gui_log_path)?
    } else {
        0
    };
    fs::write(
        output.join("gui-health.json"),
        serde_json::to_vec_pretty(&serde_json::json!({
            "unhandledErrors": errors, "humanVerdict": "pending"
        }))?,
    )?;
    println!("Evidence: {} (human verdict pending)", output.display());
    journey?;
    let driver_report: serde_json::Value = serde_json::from_slice(
        &fs::read(output.join("stress-body-report.json"))
            .context("Flutter Driver did not write the stress-body report")?,
    )?;
    ensure!(
        driver_report["scenario"] == context.scenario && driver_report["verdict"] == "pending",
        "stress-body report is missing its scenario or pending verdict"
    );
    ensure!(
        driver_report["status"] == "complete",
        "stress-body journey did not complete: stage={} failedChecks={}",
        driver_report["stage"],
        driver_report["failedChecks"],
    );
    let session_switch = driver_report["observations"]["sessionSwitch"]
        .as_object()
        .context("stress-body report has no session-switch or reading-restoration evidence")?;
    ensure!(
        session_switch["returnedToOriginal"] == true,
        "stress-body journey did not return to the original session"
    );
    // Cross-block selection/copy: the real `SelectionArea` copy of the whole
    // long reply must contain the exact canonical body as one contiguous run and
    // the body must have been carried by more than one real `RenderParagraph`, so
    // a single-paragraph render or a direct domain-text write cannot pass.
    let selection_copy = driver_report["observations"]["selectionCopy"]
        .as_object()
        .context("stress-body report has no cross-block selection-copy evidence")?;
    ensure!(
        selection_copy["pass"] == true,
        "stress-body cross-block selection copy did not pass: {selection_copy:?}"
    );
    ensure!(
        selection_copy["copyButtonPresent"] == true,
        "stress-body copy did not go through the product context-menu callback: \
         {selection_copy:?}"
    );
    ensure!(
        selection_copy["copiedParagraphCount"].as_u64().unwrap_or(0) >= 2
            && selection_copy["renderedParagraphCount"]
                .as_u64()
                .unwrap_or(0)
                >= 2,
        "stress-body body was not rendered as multiple real paragraphs: {selection_copy:?}"
    );
    ensure!(
        selection_copy["renderedMatchesCanonicalBody"] == true
            && selection_copy["copiedContainsCanonicalBody"] == true
            && selection_copy["copiedBodyMatchesCanonical"] == true
            && selection_copy["copiedChunksMatchCanonical"] == true
            && selection_copy["selectionCleared"] == true,
        "stress-body clipboard readback did not match the canonical body: {selection_copy:?}"
    );
    // The large scenario must deliver a body genuinely above the 256KiB native
    // timeline window; the fixed 220,000-character body cannot, so this is a
    // distinct requirement, not a restatement of the full-text check.
    if context.scenario == "stress-body-large" {
        let long_body = driver_report["observations"]["longBody"]
            .as_object()
            .context("stress-body-large report has no long-body evidence")?;
        ensure!(
            long_body["largeBody"] == true,
            "stress-body-large did not record its large-body marker: {long_body:?}"
        );
        // Tie the Driver's mirrored expectation to the fixture constant so the
        // two cannot drift apart silently.
        ensure!(
            long_body["expectedCharacters"].as_u64()
                == Some(pl_provider_fixture::GUI_STRESS_BODY_LARGE_CHARACTERS as u64),
            "stress-body-large expected body drifted from the fixture constant: {long_body:?}"
        );
        ensure!(
            long_body["fullTextPresent"] == true
                && long_body["prefixOk"] == true
                && long_body["suffixOk"] == true,
            "stress-body-large did not deliver the full body: {long_body:?}"
        );
        ensure!(
            long_body["deliveredBytes"].as_u64().unwrap_or(0) > 256 * 1024,
            "stress-body-large body did not exceed the 256KiB native window: {long_body:?}"
        );
        // Generation-time proof: the body kept following the stream above the
        // threshold in at least two real growth frames of the same row.
        let growth = &driver_report["observations"]["streamingGrowth"];
        ensure!(
            growth["windowPassed"] == true
                && growth["identityStable"] == true
                && growth["growthFramesAboveWindow"].as_u64().unwrap_or(0) >= 2,
            "stress-body-large did not prove the body kept following the stream above \
             the 256KiB window: {growth:?}"
        );
    }
    let report = host_report.context("stress-body fixture did not emit a stress report")?;
    ensure!(
        report.emitted_events == pl_provider_fixture::STRESS_EVENT_COUNT,
        "stress-body stream incomplete: {}/{} events",
        report.emitted_events,
        pl_provider_fixture::STRESS_EVENT_COUNT
    );
    ensure!(
        report.emitted_bytes > 0,
        "stress-body stream reported no provider output bytes"
    );
    // The large scenario is deliberately paced slower (2,500 token/s) so the
    // window above the 256KiB threshold stays long enough to sample at least two
    // growing frames; the default body keeps its 5,000 token/s, 4s baseline.
    let (min_elapsed, max_elapsed, nominal_rate) = if context.scenario == "stress-body-large" {
        (7_000, 20_000, 2_500)
    } else {
        (3_800, 10_000, 5_000)
    };
    ensure!(
        report.elapsed_millis >= min_elapsed,
        "stress-body stream ran faster than the {nominal_rate} nominal tokens/s"
    );
    ensure!(
        report.elapsed_millis <= max_elapsed,
        "stress-body stream fell behind the {nominal_rate} token/s fixture: {}ms",
        report.elapsed_millis
    );
    ensure!(
        fixture_result.is_ok_and(|status| status.success()) && rejected == 0 && errors == 0,
        "stress-body fixture or GUI health incomplete; human verdict pending"
    );
    Ok(())
}

/// Paused-history-writer acceptance journey driven through Flutter Driver.
///
/// The coordinator holds one exclusive external write transaction on the real
/// per-session `history.sqlite` while the fixture streams a reply, so the journey
/// proves the live content still grows and the current activity stays visible
/// while the durable writer cannot commit, the turn reaches its terminal state
/// while still locked, and the durable history then catches up with the answer
/// exactly once after the lock is released.
///
/// It never fabricates a fault: the pause stays inside the runtime's retryable
/// conflict window, so the run records ordinary SQLite blocking plus the exact
/// live/durable divergence rather than claiming a typed fault. The typed fault,
/// its generation and the explicit retry/resume are exercised by the separate
/// history-fault scenario; the precise scope is written to
/// `history-lock-interface-needs.json` instead of being guessed.
fn run_history_lock_scenario(
    context: &HistoryLockScenario<'_>,
    mut fixture: OwnedProcess,
) -> Result<()> {
    let HistoryLockScenario {
        workspace,
        app_dir,
        home,
        working,
        output,
        fixture_log,
        requests_file,
        interrupt,
    } = context;
    let gui_log_path = working.join("history-lock-gui.log");
    let stage = working.join("history-lock-stage");
    let project = working.join("history-lock-project");
    let acquired_marker = working.join("history-lock-acquired");
    let released_marker = working.join("history-lock-released");
    let observed_file = working.join("history-lock-observed.json");
    let mut driver_exit: Option<std::process::ExitStatus> = None;
    let mut lock_report: Option<serde_json::Value> = None;
    let mut after: Option<(HistoryLockWatermark, HistoryLockTurnItems)> = None;
    let journey = (|| -> Result<()> {
        fs::create_dir(&project)?;
        ensure!(
            Command::new("git")
                .args(["init", "-q"])
                .arg(&project)
                .status()?
                .success(),
            "failed to initialize isolated history-lock project"
        );
        // The GUI handle is scoped so the resident process is reaped before the
        // durable history library is read back for the "caught up" assertion.
        {
            let mut gui = start_history_lock_gui(workspace, home, &gui_log_path)?;
            let vm_url = wait_for_vm(&gui_log_path, &mut gui, &mut fixture, interrupt)?;
            let mut driver = start_history_lock_driver(
                app_dir, home, &vm_url, &project, output, working, &stage,
            )?;
            // 1. The driver durably settles a priming turn, so the real
            // per-session history library exists before the lock is taken.
            wait_for_history_lock_stage(
                "history_lock_ready",
                &stage,
                &mut driver,
                &mut gui,
                &mut fixture,
                interrupt,
                Duration::from_secs(240),
            )?;
            let database = history_lock_database(home)?;
            let before = history_lock_watermark(&database)?;
            // 2. Real exclusive write transaction on the actual library. The
            // runtime writer for this exact database cannot commit while held.
            let mut lock =
                SqliteWriteLock::acquire(&database, "history.sqlite", Duration::from_secs(90))?;
            let locked_at = Instant::now();
            fs::write(&acquired_marker, "acquired")?;
            // 3. Live growth and the terminal state are observed while locked.
            wait_for_history_lock_stage(
                "history_lock_terminal_while_locked",
                &stage,
                &mut driver,
                &mut gui,
                &mut fixture,
                interrupt,
                Duration::from_secs(300),
            )?;
            let observed: serde_json::Value = serde_json::from_slice(&fs::read(&observed_file)?)?;
            let turn_id = observed["turnId"]
                .as_str()
                .context("history-lock Driver did not record the locked turn id")?
                .to_owned();
            // Read the durable watermarks while the lock is still held: WAL
            // readers are not blocked, and a failure is recorded rather than
            // asserted so a busy read can never be mistaken for a pass.
            let while_locked = history_lock_watermark(&database).ok();
            let while_items = history_lock_turn_items(
                &database,
                &turn_id,
                pl_provider_fixture::GUI_HISTORY_LOCK_ANSWER,
            )
            .ok();
            let held_millis = locked_at.elapsed().as_millis();
            // 4. Release, then unblock the driver. `finish` commits and closes
            // the transaction; `Drop` is the backstop for any earlier error.
            let release = lock.finish();
            fs::write(&released_marker, "released")?;
            release?;
            let status = wait_for_history_lock_driver(
                &stage,
                &mut driver,
                &mut gui,
                &mut fixture,
                interrupt,
            )?;
            driver_exit = Some(status);
            ensure!(
                status.success(),
                "history-lock Driver failed with {status} (last stage: {})",
                fs::read_to_string(&stage).unwrap_or_else(|_| {
                    "driver_not_started (see history-lock-driver-errors.txt)".to_owned()
                })
            );
            lock_report = Some(serde_json::json!({
                "database": database.file_name().and_then(|name| name.to_str()),
                "databaseDirectory": database
                    .parent()
                    .and_then(|parent| parent.file_name())
                    .and_then(|name| name.to_str()),
                "heldMillis": held_millis,
                "before": before,
                "whileLocked": while_locked,
                "whileLockedItems": while_items,
                "observed": observed,
            }));
        }
        // 5. The GUI has exited, so the durable library is read back cleanly.
        let database = history_lock_database(home)?;
        let report = lock_report
            .as_ref()
            .context("history-lock journey did not record its lock evidence")?;
        let turn_id = report["observed"]["turnId"]
            .as_str()
            .context("history-lock lock evidence has no locked turn id")?
            .to_owned();
        after = Some((
            history_lock_watermark(&database)?,
            history_lock_turn_items(
                &database,
                &turn_id,
                pl_provider_fixture::GUI_HISTORY_LOCK_ANSWER,
            )?,
        ));
        Ok(())
    })();

    if gui_log_path.is_file() {
        write_sanitized_log(&gui_log_path, &output.join("history-lock-gui.log"))?;
    }
    write_realtime_driver_evidence(
        &stage.with_extension("driver.log"),
        output,
        "history-lock",
        driver_exit,
        journey.as_ref().err(),
        &[
            (app_dir.to_string_lossy().into_owned(), "<app>"),
            (workspace.to_string_lossy().into_owned(), "<workspace>"),
            (home.to_string_lossy().into_owned(), "<home>"),
            (working.to_string_lossy().into_owned(), "<coord>"),
        ],
    )?;
    if let Some(report) = lock_report.as_ref() {
        fs::write(
            output.join("history-lock-lock.json"),
            serde_json::to_vec_pretty(report)?,
        )?;
    }
    fs::write(
        output.join("history-lock-interface-needs.json"),
        serde_json::to_vec_pretty(&serde_json::json!({
            "scenario": "history-lock",
            "coveredNow": [
                "exclusive external BEGIN IMMEDIATE on the real per-session history.sqlite while one reply streams",
                "live content keeps growing and the current activity stays visible while the writer cannot commit",
                "the turn reaches its terminal state while the writer is still locked, and no tool starts on it",
                "after release the durable history catches up with the answer exactly once and exactly",
            ],
            "typedFault": {
                "observed": false,
                "retryableConflictWindowMillis": 30000,
                "reason": "the pause is deliberately kept inside the runtime's retryable-conflict window \
                           (thread_writer.rs RETRYABLE_BUSY_WINDOW, 30s), so the writer absorbed it as ordinary \
                           SQLite blocking and drained after release; the held duration is in \
                           history-lock-lock.json. Ordinary blocking is not a typed fault and is never reported as one",
            },
            "scopeNote": [
                "this scenario deliberately keeps the pause inside the retryable-conflict window, so it never \
                 produces a hard typed fault; ordinary SQLite blocking is never reported as one",
                "the typed writeFailed fault, its stable generation and the explicit retry/resume are exercised \
                 by the separate history-fault scenario, where the integrated Driver build exposes the typed \
                 storageRecovery entry (fault/faultGeneration/acceptedSequence/durableSequence/execution/ \
                 pressurePaused/resumeRequired/canResume/blocksContinuation/lastError)",
            ],
            "notChanged": [
                "no production test hook, no data deletion, no faked state, no shortened tool",
                "existing realtime, stress-body and statistics scenarios are unchanged",
            ],
        }))?,
    )?;
    let fixture_result = fixture.stop(requests_file);
    drop(fixture);
    write_fixture_log(fixture_log, &output.join("fixture.log"))?;
    let requests = if requests_file.is_file() {
        sanitize_requests(requests_file, &output.join("requests.json"))?;
        serde_json::from_slice::<Vec<serde_json::Value>>(&fs::read(requests_file)?)?
    } else {
        Vec::new()
    };
    let accepted = requests
        .iter()
        .filter(|row| row["accepted"] == true)
        .count();
    let rejected = requests
        .iter()
        .filter(|row| row["accepted"] == false)
        .count();
    fs::write(
        output.join("fixture-status.json"),
        serde_json::to_vec_pretty(&serde_json::json!({
            "scenario": "history-lock",
            "acceptedRequests": accepted,
            "rejectedRequests": rejected,
            "completed": fixture_result.as_ref().is_ok_and(|status| status.success()),
        }))?,
    )?;
    let errors = if gui_log_path.is_file() {
        count_gui_errors(&gui_log_path)?
    } else {
        0
    };
    fs::write(
        output.join("gui-health.json"),
        serde_json::to_vec_pretty(&serde_json::json!({
            "unhandledErrors": errors, "humanVerdict": "pending"
        }))?,
    )?;
    println!("Evidence: {} (human verdict pending)", output.display());
    journey?;
    let driver_report: serde_json::Value = serde_json::from_slice(
        &fs::read(output.join("history-lock-summary.json"))
            .context("Flutter Driver did not write the history-lock journey summary")?,
    )?;
    ensure!(
        driver_report["scenario"] == "history-lock" && driver_report["verdict"] == "pending",
        "history-lock journey summary is missing its scenario or pending verdict"
    );
    ensure!(
        driver_report["status"] == "complete",
        "history-lock journey did not complete: stage={} failedChecks={}",
        driver_report["stage"],
        driver_report["failedChecks"],
    );
    let (before_watermark, after_watermark, after_items) = {
        let report = lock_report
            .as_ref()
            .context("history-lock lock evidence is missing")?;
        let before: HistoryLockWatermark = serde_json::from_value(report["before"].clone())?;
        let (after_watermark, after_items) =
            after.context("history-lock durable watermarks were not read")?;
        (before, after_watermark, after_items)
    };
    // The real library must have committed exactly the locked turn, and its
    // content must be present: the same file the acceptance locked.
    ensure!(
        after_watermark.turns == before_watermark.turns + 1,
        "history writer did not commit exactly the locked turn: {before_watermark:?} -> {after_watermark:?}"
    );
    ensure!(
        after_items.items >= 1 && after_items.answer_items >= 1,
        "durable history does not contain the delivered answer: {after_items:?}"
    );
    // Direct evidence the exclusive lock really paused the writer: the committed
    // turn count must be unchanged while the lock was held. A busy read is not
    // asserted; it simply stays recorded as a gap.
    let while_locked: Option<HistoryLockWatermark> = lock_report
        .as_ref()
        .and_then(|report| serde_json::from_value(report["whileLocked"].clone()).ok());
    if let Some(while_locked) = while_locked {
        ensure!(
            while_locked.turns == before_watermark.turns,
            "history writer committed while the exclusive lock was held: \
             {before_watermark:?} -> {while_locked:?}"
        );
    }
    ensure!(
        fixture_result.is_ok_and(|status| status.success())
            && accepted >= pl_provider_fixture::HISTORY_LOCK_REQUIRED_STEPS
            && rejected == 0
            && errors == 0,
        "history-lock fixture or GUI health incomplete; human verdict pending"
    );
    Ok(())
}

/// Typed history-fault retry/resume acceptance journey driven through Flutter Driver.
///
/// The coordinator holds the real per-session `history.sqlite` write lock past the
/// runtime's retryable-conflict window, so the durable writer must surface a typed
/// `writeFailed` fault instead of absorbing ordinary SQLite blocking. While the
/// lock is held the journey proves the live content still grows, the typed fault
/// and its stable generation are observable, and the fault reaches its safe
/// boundary: the safe `exec` the fault turn scheduled is *not* started and no new
/// model request is issued. The lock is then released with no automatic recovery,
/// and the journey drives the explicit controls in order — retry-save until the
/// backend verifies `canResume` while the hard latch `resumeRequired` stays set,
/// then continue — proving the deferred tool and the follow-up answer each
/// complete exactly once.
///
/// The integrated GUI projects a running tool's typed in-flight progress
/// (`output`/`progressBytes`/`itemRevision`), which the realtime journey asserts
/// directly; this fault journey does not depend on it, because the deferred-tool
/// proof is the typed pre-execution status before resume and the committed
/// `result` is the post-resume identity. A baseline build without the typed
/// `storageRecovery` array is reported, never faked as a pass.
fn run_history_fault_scenario(
    context: &HistoryFaultScenario<'_>,
    mut fixture: OwnedProcess,
) -> Result<()> {
    let HistoryFaultScenario {
        workspace,
        app_dir,
        home,
        working,
        output,
        fixture_log,
        requests_file,
        interrupt,
    } = context;
    let gui_log_path = working.join("history-fault-gui.log");
    let stage = working.join("history-fault-stage");
    let project = working.join("history-fault-project");
    let acquired_marker = working.join("history-fault-acquired");
    let released_marker = working.join("history-fault-released");
    let observed_file = working.join("history-fault-observed.json");
    let mut driver_exit: Option<std::process::ExitStatus> = None;
    let mut lock_report: Option<serde_json::Value> = None;
    let mut after: Option<HistoryFaultAfter> = None;
    let journey = (|| -> Result<()> {
        fs::create_dir(&project)?;
        ensure!(
            Command::new("git")
                .args(["init", "-q"])
                .arg(&project)
                .status()?
                .success(),
            "failed to initialize isolated history-fault project"
        );
        // The GUI handle is scoped so the resident process is reaped before the
        // durable library is read back for the "caught up" assertion.
        {
            let mut gui = start_history_fault_gui(workspace, home, &gui_log_path)?;
            let vm_url = wait_for_vm(&gui_log_path, &mut gui, &mut fixture, interrupt)?;
            let mut driver = start_history_fault_driver(
                app_dir, home, &vm_url, &project, output, working, &stage,
            )?;
            // 1. The driver durably settles a priming turn, so the real
            // per-session history library exists before the lock is taken.
            wait_for_history_fault_stage(
                "history_fault_ready",
                &stage,
                &mut driver,
                &mut gui,
                &mut fixture,
                interrupt,
                Duration::from_secs(300),
            )?;
            let database = history_lock_database(home)?;
            let before = history_lock_watermark(&database)?;
            // 2. Real exclusive write transaction on the actual library, held
            // past the writer's retryable window so it must surface a fault. The
            // hold timeout is generous: the fault appears only after the bounded
            // per-attempt busy waits that precede it.
            let mut lock =
                SqliteWriteLock::acquire(&database, "history.sqlite", Duration::from_secs(300))?;
            let locked_at = Instant::now();
            fs::write(&acquired_marker, "acquired")?;
            // Record a baseline lock evidence immediately after acquiring, so a
            // Driver failure before the safe boundary still leaves the
            // coordinator's before-watermark on disk. The boundary read below
            // replaces it with the full sample.
            lock_report = Some(serde_json::json!({
                "database": database.file_name().and_then(|name| name.to_str()),
                "databaseDirectory": database
                    .parent()
                    .and_then(|parent| parent.file_name())
                    .and_then(|name| name.to_str()),
                "heldMillis": 0,
                "before": before,
                "whileLocked": serde_json::Value::Null,
                "whileLockedItems": serde_json::Value::Null,
                "observed": serde_json::Value::Null,
            }));
            // 3. Live growth, the typed fault with a stable generation, and the
            // safe boundary are all observed while the lock is still held.
            wait_for_history_fault_stage(
                "history_fault_safe_boundary",
                &stage,
                &mut driver,
                &mut gui,
                &mut fixture,
                interrupt,
                Duration::from_secs(600),
            )?;
            let observed: serde_json::Value = serde_json::from_slice(&fs::read(&observed_file)?)?;
            let turn_id = observed["turnId"]
                .as_str()
                .context("history-fault Driver did not record the faulted turn id")?
                .to_owned();
            // Read the durable watermarks while the lock is still held: WAL
            // readers are not blocked, and a failure is recorded rather than
            // asserted so a busy read can never be mistaken for a pass.
            let while_locked = history_lock_watermark(&database).ok();
            let while_items = history_lock_turn_items(
                &database,
                &turn_id,
                pl_provider_fixture::GUI_HISTORY_FAULT_ANSWER,
            )
            .ok();
            let held_millis = locked_at.elapsed().as_millis();
            // Record the lock evidence now, before release and the Driver wait,
            // so the coordinator's heldMillis and the durable watermarks are
            // always preserved — even if the Driver later fails and the journey
            // reports it. The observed boundary fact is already on disk (it gates
            // the `history_fault_safe_boundary` stage above).
            lock_report = Some(serde_json::json!({
                "database": database.file_name().and_then(|name| name.to_str()),
                "databaseDirectory": database
                    .parent()
                    .and_then(|parent| parent.file_name())
                    .and_then(|name| name.to_str()),
                "heldMillis": held_millis,
                "before": before,
                "whileLocked": while_locked,
                "whileLockedItems": while_items,
                "observed": observed,
            }));
            // 4. Release, then let the driver run the explicit retry/resume path.
            // `finish` commits and closes the transaction; `Drop` is the backstop
            // for any earlier error, so the lock is always recovered.
            let release = lock.finish();
            fs::write(&released_marker, "released")?;
            release?;
            let status = wait_for_history_fault_driver(
                &stage,
                &mut driver,
                &mut gui,
                &mut fixture,
                interrupt,
            )?;
            driver_exit = Some(status);
            ensure!(
                status.success(),
                "history-fault Driver failed with {status} (last stage: {})",
                fs::read_to_string(&stage).unwrap_or_else(|_| {
                    "driver_not_started (see history-fault-driver-errors.txt)".to_owned()
                })
            );
        }
        // 5. The GUI has exited, so the durable library is read back cleanly.
        let database = history_lock_database(home)?;
        let report = lock_report
            .as_ref()
            .context("history-fault journey did not record its lock evidence")?;
        let turn_id = report["observed"]["turnId"]
            .as_str()
            .context("history-fault lock evidence has no faulted turn id")?
            .to_owned();
        after = Some(HistoryFaultAfter {
            watermark: history_lock_watermark(&database)?,
            items: history_lock_turn_items(
                &database,
                &turn_id,
                pl_provider_fixture::GUI_HISTORY_FAULT_ANSWER,
            )?,
            tool_marker_items: history_fault_payload_count(
                &database,
                &turn_id,
                pl_provider_fixture::GUI_HISTORY_FAULT_TOOL_MARKER,
            )?,
        });
        Ok(())
    })();

    if gui_log_path.is_file() {
        write_sanitized_log(&gui_log_path, &output.join("history-fault-gui.log"))?;
    }
    write_realtime_driver_evidence(
        &stage.with_extension("driver.log"),
        output,
        "history-fault",
        driver_exit,
        journey.as_ref().err(),
        &[
            (app_dir.to_string_lossy().into_owned(), "<app>"),
            (workspace.to_string_lossy().into_owned(), "<workspace>"),
            (home.to_string_lossy().into_owned(), "<home>"),
            (working.to_string_lossy().into_owned(), "<coord>"),
        ],
    )?;
    if let Some(report) = lock_report.as_ref() {
        fs::write(
            output.join("history-fault-lock.json"),
            serde_json::to_vec_pretty(report)?,
        )?;
    }
    fs::write(
        output.join("history-fault-interface-needs.json"),
        serde_json::to_vec_pretty(&history_fault_interface_needs())?,
    )?;
    let fixture_result = fixture.stop(requests_file);
    drop(fixture);
    write_fixture_log(fixture_log, &output.join("fixture.log"))?;
    let requests = if requests_file.is_file() {
        sanitize_requests(requests_file, &output.join("requests.json"))?;
        serde_json::from_slice::<Vec<serde_json::Value>>(&fs::read(requests_file)?)?
    } else {
        Vec::new()
    };
    let accepted = requests
        .iter()
        .filter(|row| row["accepted"] == true)
        .count();
    let rejected = requests
        .iter()
        .filter(|row| row["accepted"] == false)
        .count();
    fs::write(
        output.join("fixture-status.json"),
        serde_json::to_vec_pretty(&serde_json::json!({
            "scenario": "history-fault",
            "acceptedRequests": accepted,
            "rejectedRequests": rejected,
            "completed": fixture_result.as_ref().is_ok_and(|status| status.success()),
        }))?,
    )?;
    let errors = if gui_log_path.is_file() {
        count_gui_errors(&gui_log_path)?
    } else {
        0
    };
    fs::write(
        output.join("gui-health.json"),
        serde_json::to_vec_pretty(&serde_json::json!({
            "unhandledErrors": errors, "humanVerdict": "pending"
        }))?,
    )?;
    println!("Evidence: {} (human verdict pending)", output.display());
    journey?;
    let driver_report: serde_json::Value = serde_json::from_slice(
        &fs::read(output.join("history-fault-summary.json"))
            .context("Flutter Driver did not write the history-fault journey summary")?,
    )?;
    ensure!(
        driver_report["scenario"] == "history-fault" && driver_report["verdict"] == "pending",
        "history-fault journey summary is missing its scenario or pending verdict"
    );
    ensure!(
        driver_report["status"] == "complete",
        "history-fault journey did not complete: stage={} failedChecks={}",
        driver_report["stage"],
        driver_report["failedChecks"],
    );
    let (before_watermark, after_state) = {
        let report = lock_report
            .as_ref()
            .context("history-fault lock evidence is missing")?;
        let before: HistoryLockWatermark = serde_json::from_value(report["before"].clone())?;
        (
            before,
            after.context("history-fault durable watermarks were not read")?,
        )
    };
    // The real library must commit exactly the faulted turn once the fault latch
    // was released through the explicit retry/resume path, and its content must be
    // present: the same file the acceptance locked.
    ensure!(
        after_state.watermark.turns == before_watermark.turns + 1,
        "history writer did not commit exactly the faulted turn: {before_watermark:?} -> {:?}",
        after_state.watermark
    );
    ensure!(
        after_state.items.answer_items >= 1,
        "durable history does not contain the delivered answer: {:?}",
        after_state.items
    );
    ensure!(
        after_state.tool_marker_items >= 1,
        "durable history does not contain the committed safe-tool result for the faulted turn: \
         {} marker items",
        after_state.tool_marker_items
    );
    // Direct evidence the exclusive lock really paused the writer and the fault
    // held the answer out of durable history: the committed turn count and write
    // watermark must be unchanged while the lock was held, and the answer must be
    // absent. A busy read is not asserted; it simply stays recorded as a gap.
    let while_locked: Option<HistoryLockWatermark> = lock_report
        .as_ref()
        .and_then(|report| serde_json::from_value(report["whileLocked"].clone()).ok());
    if let Some(while_locked) = while_locked {
        ensure!(
            while_locked == before_watermark,
            "history writer committed while the exclusive lock was held: \
             {before_watermark:?} -> {while_locked:?}"
        );
    }
    let while_items: Option<HistoryLockTurnItems> = lock_report
        .as_ref()
        .and_then(|report| serde_json::from_value(report["whileLockedItems"].clone()).ok());
    if let Some(while_items) = while_items {
        ensure!(
            while_items.answer_items == 0,
            "the answer was already durable while the exclusive lock was held: {while_items:?}"
        );
    }
    ensure!(
        fixture_result.is_ok_and(|status| status.success())
            && accepted >= pl_provider_fixture::HISTORY_FAULT_REQUIRED_STEPS
            && rejected == 0
            && errors == 0,
        "history-fault fixture or GUI health incomplete; human verdict pending"
    );
    Ok(())
}

/// Durable history state read back after the faulted turn committed.
struct HistoryFaultAfter {
    watermark: HistoryLockWatermark,
    items: HistoryLockTurnItems,
    /// Committed items of the faulted turn whose payload carries the safe-tool marker.
    tool_marker_items: i64,
}

/// Interface requirements and gaps for the history-fault acceptance.
///
/// The covered facts are the ones the journey really asserts; the gaps name the
/// exact stable field still needed so nothing is inferred from a weaker signal.
fn history_fault_interface_needs() -> serde_json::Value {
    serde_json::json!({
        "scenario": "history-fault",
        "coveredNow": [
            "exclusive external BEGIN IMMEDIATE on the real per-session history.sqlite, held past the writer's retryable-conflict window (thread_writer.rs RETRYABLE_BUSY_WINDOW, 30s)",
            "live content still grows while the durable writer cannot commit",
            "a typed HistoryFault (writeFailed) with a stable faultGeneration is observable from the Driver snapshot's storageRecovery entry for the faulted Thread, together with the pause reason the UI renders from the same typed facts",
            "the fault reaches its canonical safe boundary (continuation deferred: resumeRequired with canResume false): the safe exec the fault turn scheduled stays in an explicit pre-execution state (queued/awaitingApproval, no committed marker), this turn has no final answer, and no follow-up model request is issued",
            "release without automatic recovery, then retry-save until the backend-verified canResume is true while resumeRequired stays set, then continue; the deferred tool and the follow-up answer each complete exactly once",
        ],
        "typedToolProgress": {
            "observed": true,
            "detail": "the integrated GUI projects a running tool's typed in-flight progress: the timeline tool item carries `output` (same source as the committed `result`), `progressBytes` (UTF-8 count) and `itemRevision`, so realtime_journey.dart asserts the realtime long-command output step by step (longCommandInFlightOutputGrows) without reading raw JSON or substituting the final result",
            "historyFaultUse": "this fault journey does not depend on in-flight progress: the deferred-tool proof is the typed pre-execution status (`queued`/`awaitingApproval`) at the latched safe boundary and before/after retry, and the committed `result` carries the post-resume identity",
        },
        "retryResumeContract": {
            "buttons": [
                "history-retry-<threadId>: retry the durable save for the fault generation",
                "history-resume-<threadId>: explicit continue, only enabled when typed canResume is true",
            ],
            "typedState": "storageRecovery[threadId] = {fault, faultGeneration, acceptedSequence, durableSequence, execution, pressurePaused, resumeRequired, canResume, blocksContinuation, lastError}; canResume is derived by the backend from the retry generation/fence, never from lastError text",
            "generation": "a stale generation can never drive a resume; that fence is proved by the runtime's own generation tests, and this journey confirms the button uses the canonical faultGeneration",
        },
        "notChanged": [
            "no production test hook, no data deletion, no faked state, no shortened safe tool",
            "existing realtime, stress-body, statistics and history-lock scenarios are unchanged",
        ],
    })
}

/// Per-session history watermarks read directly from the real `history.sqlite`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
struct HistoryLockWatermark {
    /// Committed `history_turns` rows.
    turns: i64,
    /// `history_meta.applied_write_seq`, the durable write watermark.
    write_seq: i64,
}

/// Items of one Turn, and how many of them carry the expected answer text.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
struct HistoryLockTurnItems {
    items: i64,
    answer_items: i64,
}

/// Locates the single per-session history library of the isolated home.
///
/// The acceptance deliberately locks the *real* library, never a copy. The home
/// is isolated to this run and holds exactly one Thread, so more than one
/// candidate (or none) is reported instead of guessing which library to lock.
fn history_lock_database(home: &Path) -> Result<std::path::PathBuf> {
    let sessions = home.join("v2/sessions");
    let mut candidates = Vec::new();
    if sessions.is_dir() {
        for entry in fs::read_dir(&sessions)? {
            let database = entry?.path().join("history.sqlite");
            if database.is_file() {
                candidates.push(database);
            }
        }
    }
    ensure!(
        candidates.len() == 1,
        "expected exactly one per-session history.sqlite under {}, found {}",
        sessions.display(),
        candidates.len()
    );
    Ok(candidates.remove(0))
}

/// Reads the durable history watermarks through a read-only connection.
fn history_lock_watermark(database: &Path) -> Result<HistoryLockWatermark> {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    let database = database.to_owned();
    let display = database.clone();
    runtime.block_on(async move {
        let options = SqliteConnectOptions::new()
            .filename(&database)
            .read_only(true);
        let mut connection = SqliteConnection::connect_with(&options).await?;
        let (turns, write_seq): (i64, i64) = sea_orm::sqlx::query_as(
            "SELECT (SELECT COUNT(*) FROM history_turns),
                    COALESCE((SELECT applied_write_seq FROM history_meta WHERE id = 1), 0)",
        )
        .fetch_one(&mut connection)
        .await
        .with_context(|| {
            format!(
                "failed to read history watermarks from {}",
                display.display()
            )
        })?;
        Ok(HistoryLockWatermark { turns, write_seq })
    })
}

/// Counts one Turn's committed items and how many carry [answer].
fn history_lock_turn_items(
    database: &Path,
    turn_id: &str,
    answer: &str,
) -> Result<HistoryLockTurnItems> {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    let database = database.to_owned();
    let display = database.clone();
    let turn_id = turn_id.to_owned();
    let answer = answer.to_owned();
    runtime.block_on(async move {
        let options = SqliteConnectOptions::new()
            .filename(&database)
            .read_only(true);
        let mut connection = SqliteConnection::connect_with(&options).await?;
        let statement = "SELECT COUNT(*), \
             COALESCE(SUM(CASE WHEN payload LIKE ?1 THEN 1 ELSE 0 END), 0) \
             FROM history_items WHERE turn_id = ?2";
        let (items, answer_items): (i64, i64) = sea_orm::sqlx::query_as(statement)
            .bind(format!("%{answer}%"))
            .bind(turn_id.clone())
            .fetch_one(&mut connection)
            .await
            .with_context(|| {
                format!(
                    "failed to read committed items for turn {turn_id} from {}",
                    display.display()
                )
            })?;
        Ok(HistoryLockTurnItems {
            items,
            answer_items,
        })
    })
}

/// Counts one Turn's committed items whose payload carries [needle].
///
/// Used for the safe-tool marker, whose committed presence proves the deferred
/// tool result really landed for that turn. It is read-only and never creates or
/// upgrades the library.
fn history_fault_payload_count(database: &Path, turn_id: &str, needle: &str) -> Result<i64> {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    let database = database.to_owned();
    let display = database.clone();
    let turn_id = turn_id.to_owned();
    let needle = needle.to_owned();
    runtime.block_on(async move {
        let options = SqliteConnectOptions::new()
            .filename(&database)
            .read_only(true);
        let mut connection = SqliteConnection::connect_with(&options).await?;
        let statement = "SELECT COUNT(*) FROM history_items WHERE turn_id = ?2 AND payload LIKE ?1";
        let (count,): (i64,) = sea_orm::sqlx::query_as(statement)
            .bind(format!("%{needle}%"))
            .bind(turn_id.clone())
            .fetch_one(&mut connection)
            .await
            .with_context(|| {
                format!(
                    "failed to count committed items for turn {turn_id} from {}",
                    display.display()
                )
            })?;
        Ok(count)
    })
}

fn start_stress_body_gui(workspace: &Path, home: &Path, log_path: &Path) -> Result<OwnedProcess> {
    let log = File::create(log_path)?;
    let mut command = Command::new("cargo");
    command
        .current_dir(workspace)
        // Profile/AOT keeps the recorded frame timings representative, matching stress.
        .args(["xtask", "run-gui", "--driver", "--profile"])
        .env("ANYWORK_HOME", home)
        .stdout(Stdio::from(log.try_clone()?))
        .stderr(Stdio::from(log));
    OwnedProcess::start(&mut command, false)
}

fn start_stress_body_driver(
    context: &StressBodyScenario<'_>,
    vm_url: &str,
    project: &Path,
    stage: &Path,
) -> Result<OwnedProcess> {
    let log = File::create(stage.with_extension("driver.log"))?;
    let mut command = Command::new("dart");
    command
        .current_dir(context.app_dir)
        .args(["run", "test_driver/stress_body.dart", vm_url])
        .arg(project)
        .arg(context.output)
        .arg(context.working)
        .arg(context.scenario)
        .env("ANYWORK_HOME", context.home)
        .stdout(Stdio::from(log.try_clone()?))
        .stderr(Stdio::from(log));
    OwnedProcess::start(&mut command, false)
}

fn start_realtime_gui(workspace: &Path, home: &Path, log_path: &Path) -> Result<OwnedProcess> {
    let log = File::create(log_path)?;
    let mut command = Command::new("cargo");
    command
        .current_dir(workspace)
        // Profile/AOT keeps the recorded frame timings representative, matching stress.
        .args(["xtask", "run-gui", "--driver", "--profile"])
        .env("ANYWORK_HOME", home)
        .stdout(Stdio::from(log.try_clone()?))
        .stderr(Stdio::from(log));
    OwnedProcess::start(&mut command, false)
}

fn start_realtime_driver(
    app_dir: &Path,
    home: &Path,
    vm_url: &str,
    project: &Path,
    output: &Path,
    working: &Path,
    stage: &Path,
) -> Result<OwnedProcess> {
    let log = File::create(stage.with_extension("driver.log"))?;
    let mut command = Command::new("dart");
    command
        .current_dir(app_dir)
        .args(["run", "test_driver/realtime_journey.dart", vm_url])
        .arg(project)
        .arg(output)
        .arg(working)
        .env("ANYWORK_HOME", home)
        .stdout(Stdio::from(log.try_clone()?))
        .stderr(Stdio::from(log));
    OwnedProcess::start(&mut command, false)
}

fn wait_for_realtime_driver(
    stage: &Path,
    driver: &mut OwnedProcess,
    gui: &mut OwnedProcess,
    fixture: &mut OwnedProcess,
    interrupt: &mpsc::Receiver<()>,
) -> Result<std::process::ExitStatus> {
    let deadline = Instant::now() + Duration::from_secs(900);
    loop {
        if let Some(status) = driver.child.try_wait()? {
            driver.stopped = true;
            // Return the status even on failure so the caller can record the
            // exit code; it decides whether the run counts as successful.
            return Ok(status);
        }
        ensure!(
            !gui.exited()?,
            "GUI exited before the realtime journey completed"
        );
        ensure!(
            !fixture.exited()?,
            "provider fixture exited during the realtime journey"
        );
        ensure!(interrupt.try_recv().is_err(), "realtime journey cancelled");
        ensure!(
            Instant::now() < deadline,
            "realtime journey timed out (last stage: {})",
            fs::read_to_string(stage).unwrap_or_else(|_| "connect".to_owned())
        );
        thread::sleep(Duration::from_millis(200));
    }
}

fn start_history_lock_gui(workspace: &Path, home: &Path, log_path: &Path) -> Result<OwnedProcess> {
    let log = File::create(log_path)?;
    let mut command = Command::new("cargo");
    command
        .current_dir(workspace)
        // Profile/AOT keeps the observed timings representative, like realtime.
        .args(["xtask", "run-gui", "--driver", "--profile"])
        .env("ANYWORK_HOME", home)
        .stdout(Stdio::from(log.try_clone()?))
        .stderr(Stdio::from(log));
    OwnedProcess::start(&mut command, false)
}

fn start_history_lock_driver(
    app_dir: &Path,
    home: &Path,
    vm_url: &str,
    project: &Path,
    output: &Path,
    working: &Path,
    stage: &Path,
) -> Result<OwnedProcess> {
    let log = File::create(stage.with_extension("driver.log"))?;
    let mut command = Command::new("dart");
    command
        .current_dir(app_dir)
        .args(["run", "test_driver/history_writer_journey.dart", vm_url])
        .arg(project)
        .arg(output)
        .arg(working)
        .env("ANYWORK_HOME", home)
        .stdout(Stdio::from(log.try_clone()?))
        .stderr(Stdio::from(log));
    OwnedProcess::start(&mut command, false)
}

/// Waits, bounded, for the Driver to publish [expected] as its current stage.
fn wait_for_history_lock_stage(
    expected: &str,
    stage: &Path,
    driver: &mut OwnedProcess,
    gui: &mut OwnedProcess,
    fixture: &mut OwnedProcess,
    interrupt: &mpsc::Receiver<()>,
    timeout: Duration,
) -> Result<()> {
    let deadline = Instant::now() + timeout;
    loop {
        if stage.is_file() && fs::read_to_string(stage)? == expected {
            return Ok(());
        }
        if let Some(status) = driver.child.try_wait()? {
            bail!("history-lock Driver exited before {expected}: {status}");
        }
        ensure!(!gui.exited()?, "GUI exited before {expected}");
        ensure!(
            !fixture.exited()?,
            "provider fixture exited before {expected}"
        );
        ensure!(
            interrupt.try_recv().is_err(),
            "history-lock journey cancelled"
        );
        ensure!(
            Instant::now() < deadline,
            "history-lock Driver timed out before {expected} (last stage: {})",
            fs::read_to_string(stage).unwrap_or_else(|_| "connect".to_owned())
        );
        thread::sleep(Duration::from_millis(100));
    }
}

/// Waits for the Driver to finish, returning its real exit status.
fn wait_for_history_lock_driver(
    stage: &Path,
    driver: &mut OwnedProcess,
    gui: &mut OwnedProcess,
    fixture: &mut OwnedProcess,
    interrupt: &mpsc::Receiver<()>,
) -> Result<std::process::ExitStatus> {
    let deadline = Instant::now() + Duration::from_secs(600);
    loop {
        if let Some(status) = driver.child.try_wait()? {
            driver.stopped = true;
            return Ok(status);
        }
        ensure!(
            !gui.exited()?,
            "GUI exited before the history-lock journey completed"
        );
        ensure!(
            !fixture.exited()?,
            "provider fixture exited during the history-lock journey"
        );
        ensure!(
            interrupt.try_recv().is_err(),
            "history-lock journey cancelled"
        );
        ensure!(
            Instant::now() < deadline,
            "history-lock journey timed out (last stage: {})",
            fs::read_to_string(stage).unwrap_or_else(|_| "connect".to_owned())
        );
        thread::sleep(Duration::from_millis(200));
    }
}

fn start_history_fault_gui(workspace: &Path, home: &Path, log_path: &Path) -> Result<OwnedProcess> {
    let log = File::create(log_path)?;
    let mut command = Command::new("cargo");
    command
        .current_dir(workspace)
        // Profile/AOT keeps the observed timings representative, like history-lock.
        .args(["xtask", "run-gui", "--driver", "--profile"])
        .env("ANYWORK_HOME", home)
        .stdout(Stdio::from(log.try_clone()?))
        .stderr(Stdio::from(log));
    OwnedProcess::start(&mut command, false)
}

fn start_history_fault_driver(
    app_dir: &Path,
    home: &Path,
    vm_url: &str,
    project: &Path,
    output: &Path,
    working: &Path,
    stage: &Path,
) -> Result<OwnedProcess> {
    let log = File::create(stage.with_extension("driver.log"))?;
    let mut command = Command::new("dart");
    command
        .current_dir(app_dir)
        .args(["run", "test_driver/history_fault_journey.dart", vm_url])
        .arg(project)
        .arg(output)
        .arg(working)
        .env("ANYWORK_HOME", home)
        .stdout(Stdio::from(log.try_clone()?))
        .stderr(Stdio::from(log));
    OwnedProcess::start(&mut command, false)
}

/// Waits, bounded, for the Driver to publish [expected] as its current stage.
fn wait_for_history_fault_stage(
    expected: &str,
    stage: &Path,
    driver: &mut OwnedProcess,
    gui: &mut OwnedProcess,
    fixture: &mut OwnedProcess,
    interrupt: &mpsc::Receiver<()>,
    timeout: Duration,
) -> Result<()> {
    let deadline = Instant::now() + timeout;
    loop {
        if stage.is_file() && fs::read_to_string(stage)? == expected {
            return Ok(());
        }
        if let Some(status) = driver.child.try_wait()? {
            bail!("history-fault Driver exited before {expected}: {status}");
        }
        ensure!(!gui.exited()?, "GUI exited before {expected}");
        ensure!(
            !fixture.exited()?,
            "provider fixture exited before {expected}"
        );
        ensure!(
            interrupt.try_recv().is_err(),
            "history-fault journey cancelled"
        );
        ensure!(
            Instant::now() < deadline,
            "history-fault Driver timed out before {expected} (last stage: {})",
            fs::read_to_string(stage).unwrap_or_else(|_| "connect".to_owned())
        );
        thread::sleep(Duration::from_millis(100));
    }
}

/// Waits for the Driver to finish, returning its real exit status.
fn wait_for_history_fault_driver(
    stage: &Path,
    driver: &mut OwnedProcess,
    gui: &mut OwnedProcess,
    fixture: &mut OwnedProcess,
    interrupt: &mpsc::Receiver<()>,
) -> Result<std::process::ExitStatus> {
    let deadline = Instant::now() + Duration::from_secs(600);
    loop {
        if let Some(status) = driver.child.try_wait()? {
            driver.stopped = true;
            return Ok(status);
        }
        ensure!(
            !gui.exited()?,
            "GUI exited before the history-fault journey completed"
        );
        ensure!(
            !fixture.exited()?,
            "provider fixture exited during the history-fault journey"
        );
        ensure!(
            interrupt.try_recv().is_err(),
            "history-fault journey cancelled"
        );
        ensure!(
            Instant::now() < deadline,
            "history-fault journey timed out (last stage: {})",
            fs::read_to_string(stage).unwrap_or_else(|_| "connect".to_owned())
        );
        thread::sleep(Duration::from_millis(200));
    }
}

fn write_sanitized_log(source: &Path, destination: &Path) -> Result<()> {
    let log = fs::read_to_string(source)?;
    let mut output = File::create(destination)?;
    for line in log.lines() {
        if let Some(stage) = line.strip_prefix("startup_stage=") {
            if stage
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'=' | b' '))
            {
                writeln!(output, "startup_stage={stage}")?;
            }
        } else if line.contains("available at: http://127.0.0.1:") {
            writeln!(output, "vm_service=ready (address redacted)")?;
        } else if line.contains("resident command exited:") {
            writeln!(output, "gui_process=exited")?;
        } else if let Some(metrics) = line.split("timeline_frame_work ").nth(1)
            && metrics
                .bytes()
                .all(|byte| byte.is_ascii_digit() || matches!(byte, b' ' | b'=' | b'_' | b'\r'))
        {
            writeln!(output, "timeline_frame_work {metrics}")?;
        }
    }
    Ok(())
}

/// Persists the Flutter Driver log and an error digest as reviewable evidence.
///
/// The source log lives under the isolated coordination directory that is
/// removed with the run, so both files are written here. The full log is
/// sanitized line by line; the digest always records the exit code and the
/// journey error and never ends up empty when the log has content.
fn write_realtime_driver_evidence(
    driver_log: &Path,
    output: &Path,
    stem: &str,
    exit: Option<std::process::ExitStatus>,
    journey_error: Option<&anyhow::Error>,
    redactions: &[(String, &str)],
) -> Result<()> {
    let sanitized = if driver_log.is_file() {
        let raw = fs::read_to_string(driver_log)?;
        let mut sanitized = String::new();
        for line in raw.lines() {
            sanitized.push_str(&sanitize_driver_line(line, redactions));
            sanitized.push('\n');
        }
        fs::write(output.join(format!("{stem}-driver.log")), &sanitized)?;
        sanitized
    } else {
        String::new()
    };

    let mut digest = String::new();
    match exit {
        Some(status) => digest.push_str(&format!("driver_exit={status}\n")),
        None => digest.push_str("driver_exit=unknown (no exit status observed)\n"),
    }
    if let Some(error) = journey_error {
        digest.push_str(&format!(
            "journey_error={}\n",
            sanitize_driver_line(&error.to_string(), redactions)
        ));
    }
    let mut wrote_diagnostic = false;
    for line in sanitized.lines() {
        if driver_line_is_diagnostic(line) {
            digest.push_str(line);
            digest.push('\n');
            wrote_diagnostic = true;
        }
    }
    if !wrote_diagnostic {
        digest.push_str("driver_log_tail:\n");
        let mut tail: Vec<&str> = sanitized.lines().rev().take(40).collect();
        tail.reverse();
        for line in tail {
            digest.push_str(line);
            digest.push('\n');
        }
    }
    fs::write(output.join(format!("{stem}-driver-errors.txt")), digest)?;
    Ok(())
}

/// Redacts local filesystem paths and loopback ports from one Driver log line.
fn sanitize_driver_line(line: &str, redactions: &[(String, &str)]) -> String {
    let mut sanitized = line.to_owned();
    for (path, replacement) in redactions {
        if !path.is_empty() {
            sanitized = sanitized.replace(path, replacement);
        }
    }
    let sanitized = redact_loopback_urls(&sanitized);
    let sanitized = redact_loopback_ports(&sanitized);
    sanitized.chars().take(480).collect()
}

/// Replaces a whole loopback URL token (scheme, port and ephemeral VM-service
/// auth path) with a marker so the local address is not persisted.
fn redact_loopback_urls(line: &str) -> String {
    let mut output = line.to_owned();
    for scheme in [
        "http://127.0.0.1:",
        "ws://127.0.0.1:",
        "http://localhost:",
        "ws://localhost:",
        "http://0.0.0.0:",
        "ws://0.0.0.0:",
    ] {
        while let Some(index) = output.find(scheme) {
            let end = output[index..]
                .char_indices()
                .find(|&(_, c)| c.is_whitespace() || matches!(c, '"' | '\'' | ')' | ']' | ','))
                .map_or(output.len(), |(offset, _)| index + offset);
            output.replace_range(index..end, "<loopback-url>");
        }
    }
    output
}

/// Replaces a bare `host:<port>` loopback address while keeping `file:line:col`
/// diagnostics intact (only the numeric port after a loopback host is removed).
fn redact_loopback_ports(line: &str) -> String {
    let mut output = line.to_owned();
    for host in ["127.0.0.1:", "localhost:", "0.0.0.0:"] {
        while let Some(index) = output.find(host) {
            let digits = output[index + host.len()..]
                .chars()
                .take_while(char::is_ascii_digit)
                .count();
            if digits == 0 {
                break;
            }
            output.replace_range(index + host.len()..index + host.len() + digits, "<port>");
        }
    }
    output
}

/// Whether a Driver log line carries a failure reason worth keeping.
///
/// Broad on purpose: Dart compile errors, analyzer output, unhandled
/// exceptions and stack frames all end up in the digest.
fn driver_line_is_diagnostic(line: &str) -> bool {
    let trimmed = line.trim_start();
    if trimmed.is_empty() {
        return false;
    }
    if trimmed.starts_with('#') {
        return true;
    }
    [
        "Unhandled exception",
        "Exception",
        "Bad state",
        "StateError",
        "TimeoutException",
        "DriverError",
        "FormatException",
        "SocketException",
        "Error",
        "error",
        "Failed",
        "failed",
        "Compilation",
        "compiler",
        "Connection refused",
        "exit code",
        "Exited",
    ]
    .iter()
    .any(|marker| line.contains(marker))
}

fn count_gui_errors(source: &Path) -> Result<usize> {
    let log = fs::read_to_string(source)?;
    Ok(log
        .lines()
        .filter(|line| {
            line.contains("Unhandled Exception")
                || line.contains("EXCEPTION CAUGHT BY")
                || line.starts_with("E/flutter ")
                || line.contains("] E/flutter ")
                || line.contains("ERROR:flutter/runtime")
        })
        .count())
}

fn sanitize_requests(source: &Path, destination: &Path) -> Result<()> {
    let requests: Vec<serde_json::Value> = serde_json::from_slice(&fs::read(source)?)?;
    let safe: Vec<_> = requests.iter().map(|request| {
        serde_json::json!({
            "method": request.get("method").and_then(|v| v.as_str()).filter(|m| *m == "POST"),
            "path": request.get("path").and_then(|v| v.as_str()).filter(|p| matches!(*p, "/v1/responses" | "/v1/chat/completions")),
            "accepted": request.get("accepted").and_then(|v| v.as_bool()),
            "body": "[redacted]",
            // Strict-match facts for a rejected step: scenario-owned prompt only,
            // never the caller's actual prompt or tool output.
            "diagnostic": request.get("diagnostic").filter(|value| !value.is_null())
        })
    }).collect();
    fs::write(destination, serde_json::to_vec_pretty(&safe)?)?;
    Ok(())
}

/// Persists the synthetic provider log as reviewable evidence.
///
/// The fixture prints one `fixture_*` line per strict-match fact; those lines
/// carry scenario-owned prompts and a match category but never the caller's
/// request body, so they are passed through verbatim. The outermost failure
/// reason is preserved (loopback addresses and local paths still redacted) so a
/// rejected step is never collapsed into an opaque marker. Any other line is
/// dropped because it can contain user content or local paths.
fn write_fixture_log(source: &Path, destination: &Path) -> Result<()> {
    let log = fs::read_to_string(source)?;
    let mut file = File::create(destination)?;
    for line in log.lines() {
        let trimmed = line.trim_end();
        if trimmed.starts_with("fixture_") {
            writeln!(file, "{trimmed}")?;
        } else if let Some(reason) = failure_reason(trimmed) {
            writeln!(file, "fixture_error={reason}")?;
        }
    }
    Ok(())
}

/// Extracts the outermost failure reason from one fixture log line.
///
/// Returns `None` for progress lines that are not failures, keeping the digest
/// focused on why the run stopped rather than every line the fixture wrote.
fn failure_reason(line: &str) -> Option<String> {
    let trimmed = line.trim();
    let reason = trimmed
        .strip_prefix("Error:")
        .or_else(|| trimmed.strip_prefix("error:"))?
        .trim();
    if reason.is_empty() {
        return None;
    }
    let sanitized = redact_loopback_ports(&redact_loopback_urls(reason));
    Some(sanitized.chars().take(480).collect())
}
