use crate::cli::ManualGuiOptions;
use crate::paths;
use crate::process;
use anyhow::{Context, Result, bail, ensure};
use pl_model::config::{ProviderConfig, ProviderId};
use pl_model::model::{ModelInfo, ModelTransportProfile};
use pl_model::provider::ProviderEndpoint;
use pl_studio_runtime::config::StudioConfig;
use serde::{Deserialize, Serialize};
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
    ensure!(
        io::stdin().is_terminal(),
        "manual-gui requires an interactive terminal"
    );
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
            "gui",
            "--ready-file",
        ])
        .arg(&ready_file)
        .arg("--requests-file")
        .arg(&requests_file)
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
    if let Err(error) = write_config(&home, &ready) {
        let _ = fixture.stop(&requests_file);
        drop(fixture);
        write_fixture_log(&fixture_log_path, &output.join("fixture.log"))?;
        return Err(error);
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
        println!("Native GUI is ready. Evidence: {}", output.display());
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
    drop(gui);
    let fixture_result = fixture.stop(&requests_file);
    drop(fixture);
    write_fixture_log(&fixture_log_path, &output.join("fixture.log"))?;
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
        }))?,
    )?;
    println!("Evidence: {} (human verdict pending)", output.display());
    gui_log_result?;
    session?;
    if fixture_state == "pending" {
        println!("Fixture main step was not exercised; review remains pending.");
        return Ok(());
    }
    ensure!(
        fixture_state == "completed",
        "fixture {fixture_state}; verdict remains pending"
    );
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
        ready.scenario == "gui",
        "fixture ready-file scenario must be gui"
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
        }
    }
    Ok(())
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
