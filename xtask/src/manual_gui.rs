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

struct StatisticsLock {
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

impl StatisticsLock {
    fn acquire(database: &Path) -> Result<Self> {
        let (acquired_tx, acquired_rx) = mpsc::sync_channel(1);
        let (release, release_rx) = mpsc::channel();
        let path = database.to_owned();
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
                    .recv_timeout(Duration::from_secs(90))
                    .context("statistics SQLite lock release timed out")?;
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
            bail!("calls.sqlite BEGIN IMMEDIATE was not acquired");
        }
        Ok(lock)
    }

    fn finish(&mut self) -> Result<()> {
        self.release.send(()).ok();
        if let Some(worker) = self.worker.take() {
            worker
                .join()
                .map_err(|_| anyhow::anyhow!("statistics SQLite holder panicked"))??;
        }
        Ok(())
    }
}

impl Drop for StatisticsLock {
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
    if options.scenario != "statistics" {
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
        let sessions: StressSessionsReport = serde_json::from_slice(
            &fs::read(&stress_sessions_report)
                .context("Flutter Driver did not complete the multi-session journey")?,
        )?;
        ensure!(
            sessions.original_reopened
                && !sessions.original_thread_id.is_empty()
                && sessions.original_window_items <= 96
                && sessions.session_count == pl_provider_fixture::GUI_STRESS_SESSION_COUNT
                && sessions.sessions.len() == sessions.session_count
                && sessions.directory_count > sessions.session_count,
            "multi-session stress journey incomplete"
        );
        let mut ids = HashSet::from([sessions.original_thread_id]);
        for (index, session) in sessions.sessions.iter().enumerate() {
            ensure!(
                session.ordinal == index + 1
                    && session.window_items <= 96
                    && ids.insert(session.thread_id.clone()),
                "multi-session stress item {} invalid",
                index + 1
            );
        }
        ensure!(
            sessions.sessions[0].previewed_bodies > 0,
            "long response was not previewed in the GUI"
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
        matches!(ready.scenario.as_str(), "gui" | "stress" | "statistics"),
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
            let mut lock = StatisticsLock::acquire(&calls_db)?;
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
            "body": "[redacted]"
        })
    }).collect();
    fs::write(destination, serde_json::to_vec_pretty(&safe)?)?;
    Ok(())
}

fn write_fixture_log(source: &Path, destination: &Path) -> Result<()> {
    let log = fs::read_to_string(source)?;
    let mut file = File::create(destination)?;
    for line in log.lines() {
        if line.contains("fixture completed") {
            writeln!(file, "fixture_completed=reported (details redacted)")?;
        } else if line.contains("error:") || line.contains("Error:") {
            writeln!(file, "fixture_error=reported (details redacted)")?;
        }
    }
    Ok(())
}
