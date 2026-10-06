//! 30-second application-shutdown acceptance.
//!
//! The scenario drives a real native GUI through Flutter Driver, like the other
//! manual journeys, and judges the application exit from the OS process itself:
//! the Driver request only *requests* the exit, while the coordinator OS-waits
//! the real native pid (from the Driver `pid` request, guarded by a `/proc`
//! start-time) and reads the artifact's genuine wait status from the launcher's
//! structured `--native-launch` report — never the `cargo`/Flutter wrapper exit
//! code, which the platform launch tool always reports as `0`. Every phase
//! shares the real bridge, isolated `ANYWORK_HOME`s and the local provider
//! fixture; nothing is faked with demo mode or an invented snapshot.
//!
//! Phases (each keeps its own non-overwriting evidence):
//!
//! * `normal`   — one durable turn, the typed cleanup acknowledgement, then the
//!   real exit that must return code 0.
//! * `reopen`   — reopen the same home and prove the saved turn is restored.
//! * `busy`     — a second instance on the same home records the instance-lock
//!   refusal; after it is truly closed the first instance continues and saves.
//! * `busy-reopen` — reopen the same home and prove that continued turn restored.
//! * `duplicate`— a repeated exit request, seconds after the first arm, must
//!   stay inside the single budget measured from the first arm.
//! * `hang`     — the Driver hangs the isolate after the host armed the budget;
//!   the native host must force exit code 1 at the 30-second deadline.
//! * `runtime-unavailable` — the isolated home's v2 data root cannot be created,
//!   so the runtime fails with a real typed error yet the fatal widget renders
//!   and the app still closes with diagnostics.
//! * `storage-lock` — a durable prime turn plus a writer-lock-blocked turn; the
//!   forced exit must not lose the prime and a reopen must pass `quick_check`.
//! * `service-unresponsive` — a frozen provider fixture must not block exit.
//! * `close-during-init` — a Driver-only `pending-init` fault keeps the runtime
//!   initializing; the exit must report Degraded + Unknown and force exit 1.
//! * `bridge-unavailable` — a Driver-only `bridge-load-error` fault leaves no
//!   runtime owner; the close must be a clean NotStarted exit 0.
//! * `subscription-fault` — a Driver-only faulty shutdown-progress subscription
//!   must surface a typed `progress` diagnostic and a non-zero exit.
//! * `dart-error-fallback` — an unwritable `ANYWORK_HOME/studio` must fall back
//!   to the isolated `TMPDIR` and still record stage/correlation/stack.
//! * `native-subtree-reclamation` — a scenario-owned stdio MCP server and its
//!   SIGTERM-ignoring grandchild are proven live before any close, then the
//!   Driver isolate is blocked and the native host forces the process out at
//!   the single 30-second deadline (exit code 1) while the product supervisor
//!   reclaims the whole subtree before this harness intervenes.
//! * `lsp-subtree-reclamation` — a real `lsp_query` turn starts the
//!   scenario-owned fake LSP server (declared in `[lsp.servers.*]`) and its
//!   SIGTERM-ignoring grandchild; both are proven live, then the isolate is
//!   blocked and the same forced 30-second native exit must let the product
//!   reclaim them.
//! * `tool-subtree-reclamation` — a real background `exec` turn runs the
//!   scenario-owned `--tool-peer` and its SIGTERM-ignoring grandchild; both are
//!   proven live, then the isolate is blocked and the same forced 30-second
//!   native exit must let the product reclaim them.
//! * `concurrent-stop` — a home that declares the scenario-owned stdio MCP
//!   server runs one background `exec` turn, so a single turn starts two
//!   independent supervised resources. Before taking the exclusive writer lock
//!   the coordinator waits for the strict fixture to accept the turn's required
//!   receipt continuation (its live counters report every remaining step
//!   optional), so the block lands while the turn is still streaming rather than
//!   cancelling it at the receipt. It then holds that lock across the exit
//!   request (the artificially blocked terminal save) and proves both subtrees
//!   are reclaimed on their own while that chain is still waiting. The blocked
//!   save forces a coordinated degraded finish at the 28-second cleanup budget
//!   (the 30-second watchdog is reserved for a
//!   Dart/bridge that never returns), so the phase judges the exit from the real
//!   OS status plus the native diagnostics, never a fabricated ~30-second
//!   target. Each resource is confirmed only when *both* its process and its
//!   SIGTERM-ignoring grandchild were observed to exit, and its completion is
//!   the later of the two, so a grandchild that stops early can never stand in
//!   for a parent that lingers to the native force-out.
//!
//! The second instance in the `busy` phase is closed through a real
//! `WM_DELETE_WINDOW` on an isolated `Xvfb`, so the native GTK close hook is
//! exercised (not the Dart exit the Driver covers). Nothing skipped is
//! fabricated here. `coverage.json` lists every requirement as executed or not
//! covered; a skipped branch is never written as passed.

use super::{
    FixtureReady, LspServerSpec, OwnedProcess, SqliteWriteLock, count_gui_errors,
    history_lock_database, history_lock_quick_check, history_lock_watermark, sanitize_requests,
    wait_for_vm, write_config, write_config_with_lsp_server, write_config_with_mcp_server,
    write_fixture_log, write_realtime_driver_evidence, write_sanitized_log,
};
use anyhow::{Context, Result, bail, ensure};
use pl_dev_support::process;
use serde::Serialize;
use std::fs::{self, File};
use std::path::{Path, PathBuf};
use std::process::{Command, ExitStatus, Stdio};
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

/// The single application-exit budget mandated by design 18 §18.5.3.
const EXIT_BUDGET: Duration = Duration::from_secs(30);

/// How long a phase may keep waiting on the OS process after the budget before
/// the coordinator itself fails: this only guards against a permanently wedged
/// process, it is never treated as a success signal.
const EXIT_GRACE: Duration = Duration::from_secs(45);

/// Upper bound, measured from just before the exit request, within which each
/// independently supervised resource of the `concurrent-stop` phase must have
/// been reclaimed by the product while the artificially blocked save chain is
/// still waiting.
///
/// A resource that only disappeared with the 30-second force-out would be a
/// serialized stop misrecorded as concurrent, so the bound sits well under the
/// single budget; the real OS pid timing (not the elapsed total) is the proof.
const CONCURRENT_STOP_ACK_BOUND: Duration = Duration::from_secs(15);

/// The Dart cleanup budget the blocked save must exhaust.
///
/// The single exit budget is 30 seconds, of which two seconds are reserved for
/// diagnostics, so the coordinated cleanup runs at most 28 seconds and then
/// returns a Degraded report whose `finishExit(1)` ends the native process
/// immediately. The 30-second watchdog is only for a Dart/bridge/engine that
/// never returns, so the concurrent phase must not demand exactly 30 seconds:
/// it requires the elapsed to stay inside the deadline *and* to have reached
/// this cleanup budget, where the still-held writer lock forces the save to
/// remain uncommitted.
const CONCURRENT_CLEANUP_BUDGET: Duration = Duration::from_secs(28);

pub(super) struct ShutdownScenario<'a> {
    pub(super) workspace: &'a Path,
    pub(super) app_dir: &'a Path,
    pub(super) home: &'a Path,
    pub(super) working: &'a Path,
    pub(super) output: &'a Path,
    pub(super) fixture_log: &'a Path,
    pub(super) requests_file: &'a Path,
    pub(super) status_file: &'a Path,
    pub(super) interrupt: &'a mpsc::Receiver<()>,
}

/// One phase's machine-checkable lifetime record.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct PhaseRecord {
    name: &'static str,
    status: &'static str,
    /// `executed` when the phase ran and produced evidence, `notCovered` when it
    /// was deliberately skipped with a stated reason.
    coverage: &'static str,
    detail: String,
    exit_code: Option<i32>,
    elapsed_millis: Option<u128>,
    native_pid: Option<u32>,
    owned_children: Vec<u32>,
}

/// A requirement that the Linux-only run could not execute, with its reason.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct GapRecord {
    requirement: &'static str,
    reason: &'static str,
    interface_needs: &'static str,
}

/// A Driver log to persist after the run, with its real exit status for review.
struct DriverEvidence {
    log: PathBuf,
    stem: String,
    exit: Option<ExitStatus>,
}

pub(super) fn run(context: &ShutdownScenario<'_>, mut fixture: OwnedProcess) -> Result<()> {
    let ShutdownScenario {
        workspace,
        app_dir,
        home,
        working,
        output,
        fixture_log,
        requests_file,
        status_file,
        interrupt,
    } = context;

    let ready: FixtureReady = serde_json::from_slice(
        &fs::read(working.join("fixture-ready.json"))
            .context("shutdown acceptance lost the fixture ready file")?,
    )?;

    // Isolated homes per non-shared phase. Every real closure home carries the
    // same isolated provider config so the runtime installs for real; the
    // unreadable home is the only one deliberately broken.
    let home_duplicate = working.join("shutdown-duplicate-home");
    let home_hang = working.join("shutdown-hang-home");
    let home_storage = working.join("shutdown-storage-home");
    let home_service = working.join("shutdown-service-home");
    let home_unavailable = working.join("shutdown-unavailable-home");
    for extra in [&home_duplicate, &home_hang, &home_storage, &home_service] {
        fs::create_dir_all(extra)?;
        write_config(extra, &ready, None)?;
    }
    // The runtime is never allowed to install here: a regular file occupies the
    // path where the v2 data root must be created, so the runtime fails with a
    // real typed startup error on any host (a read-only directory is ignored
    // when the acceptance runs as root, which proves nothing). The Dart app and
    // its Driver still start and stay alive to be asked to exit. (Pointing
    // `ANYWORK_BRIDGE_LIBRARY` at a missing file is not usable here: the Linux
    // CMake configure step refuses a non-existent bridge, so the GUI would never
    // start.)
    fs::create_dir(&home_unavailable)?;
    write_config(&home_unavailable, &ready, None)?;
    fs::write(home_unavailable.join("v2"), b"not a directory\n")?;

    // Driver-only fault homes: the two injected startup faults, the
    // subscription fault and the temp-fallback diagnostics home share the
    // normal isolated provider config.
    let home_init_close = working.join("shutdown-init-close-home");
    let home_bridge = working.join("shutdown-bridge-home");
    let home_subscription = working.join("shutdown-subscription-home");
    let home_tmp_fallback = working.join("shutdown-tmp-fallback-home");
    for extra in [&home_init_close, &home_bridge, &home_subscription] {
        fs::create_dir_all(extra)?;
        write_config(extra, &ready, None)?;
    }
    // The temp-fallback home cannot create its canonical diagnostic directory:
    // a regular file occupies `<home>/studio`, so `recordDartError`'s canonical
    // write fails on any host (permission bits are ignored when running as
    // root). The fallback log lands in the isolated `TMPDIR` below.
    fs::create_dir(&home_tmp_fallback)?;
    write_config(&home_tmp_fallback, &ready, None)?;
    fs::write(
        home_tmp_fallback.join("studio"),
        b"occupied by the acceptance\n",
    )?;
    let tmp_fallback_tmp = working.join("shutdown-tmp-fallback-tmp");
    fs::create_dir(&tmp_fallback_tmp)?;

    // The MCP-reclamation home starts the scenario-owned stdio MCP server
    // through the product's real supervised worker. The server spawns a
    // SIGTERM-ignoring grandchild in its own process group and records the
    // business pids so the coordinator can OS-poll them.
    let home_mcp = working.join("shutdown-mcp-home");
    fs::create_dir(&home_mcp)?;
    let fixture_bin = resolve_fixture_binary(workspace)?;
    let mcp_coord = working.join("shutdown-mcp-coord.json");
    write_config_with_mcp_server(
        &home_mcp,
        &ready,
        "shutdown-fixture",
        &fixture_bin,
        &[
            "--mcp-stdio".to_owned(),
            "--coord-file".to_owned(),
            mcp_coord.to_string_lossy().into_owned(),
        ],
    )?;

    // The LSP-reclamation home declares one scenario-owned `[lsp.servers.<id>]`
    // entry that runs the same fixture binary in `--lsp-stdio` mode. The product
    // starts it through its real supervised worker when the scripted model issues
    // the `lsp_query` tool call; the server speaks the LSP base protocol and
    // spawns a SIGTERM-ignoring grandchild in its own process group so the
    // coordinator can OS-poll the real business pids. The language id is unique
    // (the builtin rust-analyzer owns `rust`) or `apply_user_servers` fails loud.
    let home_lsp = working.join("shutdown-lsp-home");
    fs::create_dir(&home_lsp)?;
    let lsp_coord = working.join("shutdown-lsp-coord.json");
    write_config_with_lsp_server(
        &home_lsp,
        &ready,
        &LspServerSpec {
            id: "shutdown-lsp-fixture",
            command: &fixture_bin,
            args: &[
                "--lsp-stdio".to_owned(),
                "--coord-file".to_owned(),
                lsp_coord.to_string_lossy().into_owned(),
            ],
            language_id: pl_provider_fixture::GUI_SHUTDOWN_LSP_LANGUAGE_ID,
            detection: &[],
            extensions: &[],
        },
    )?;

    // The tool-reclamation home needs no extra config: the scripted model issues
    // one background `exec` call whose command runs the scenario-owned
    // `--tool-peer` inside the opened project, so the product's real tool worker
    // starts the subtree the forced exit must reclaim. The coordinator reads the
    // business pids from the peer's coordination record beside the project.
    let home_tool = working.join("shutdown-tool-home");
    fs::create_dir(&home_tool)?;
    write_config(&home_tool, &ready, None)?;
    let tool_coord = working.join("shutdown-tool-coord.json");

    // The concurrent-stop home declares the scenario-owned stdio MCP server, so
    // a single background-tool turn starts two independent supervised
    // resources: the MCP server through the thread's service lease and the
    // `--tool-peer` through the tool worker. The coordinator then holds the
    // exclusive writer lock across the exit request and OS-polls both subtrees.
    let home_concurrent = working.join("shutdown-concurrent-home");
    fs::create_dir(&home_concurrent)?;
    let concurrent_mcp_coord = working.join("shutdown-concurrent-mcp-coord.json");
    write_config_with_mcp_server(
        &home_concurrent,
        &ready,
        "shutdown-fixture",
        &fixture_bin,
        &[
            "--mcp-stdio".to_owned(),
            "--coord-file".to_owned(),
            concurrent_mcp_coord.to_string_lossy().into_owned(),
        ],
    )?;
    let concurrent_tool_coord = working.join("shutdown-concurrent-tool-coord.json");

    let mut phases: Vec<PhaseRecord> = Vec::new();
    let mut driver_evidence: Vec<DriverEvidence> = Vec::new();
    let mut fixture_paused = false;

    let journey = (|| -> Result<()> {
        // ----------------------------------------------------------- normal --
        {
            let log = working.join("shutdown-normal-gui.log");
            let project = prepare_project(working, "shutdown-normal-project")?;
            let mut gui = start_gui(workspace, home, &log, working, "normal")?;
            let vm_url = wait_for_vm(&log, &mut gui, &mut fixture, interrupt)?;
            let driver_log = working.join("shutdown-normal-driver.log");
            let mut driver = start_driver(
                app_dir,
                home,
                "normal",
                &vm_url,
                &project,
                output,
                working,
                &driver_log,
            )?;
            wait_stage(
                "normal_cleanup_ack",
                working,
                &mut driver,
                &mut gui,
                &mut fixture,
                interrupt,
                Duration::from_secs(600),
            )?;
            // The measurement starts at the real native arm, not at the save
            // window above nor after the Driver finishes.
            let (native_pid, start_ticks, children) = arm_native(
                "normal",
                "normal_exit_requested",
                output,
                interrupt,
                working,
                &mut driver,
                &mut gui,
                &mut fixture,
            )?;
            let arm = Instant::now();
            let native = wait_native_exit(
                "normal",
                working,
                output,
                native_pid,
                start_ticks,
                &mut gui,
                interrupt,
                EXIT_GRACE,
                arm,
            )?;
            let driver_exit = wait_driver(
                "normal",
                working,
                &mut driver,
                interrupt,
                Duration::from_secs(180),
            )?;
            driver_evidence.push(DriverEvidence {
                log: driver_log,
                stem: "shutdown-normal".into(),
                exit: Some(driver_exit),
            });
            phases.push(PhaseRecord {
                name: "normal",
                status: "executed",
                coverage: "executed",
                detail: format!(
                    "saved a durable turn, acknowledged the typed cleanup, then requested the real app exit: {}",
                    native.summary()
                ),
                exit_code: native.code,
                elapsed_millis: Some(native.elapsed.as_millis()),
                native_pid: Some(native.pid),
                owned_children: children,
            });
            ensure!(
                native.code == Some(0) && native.signal.is_none(),
                "normal native exit was not code 0: {}",
                native.summary()
            );
            ensure!(
                native.reclaimed_descendants.is_empty(),
                "normal native exit left a child tree for the harness: {:?}",
                native.reclaimed_descendants
            );
            write_sanitized_log(&log, &output.join("shutdown-normal-gui.log"))?;
        }

        // ----------------------------------------------------------- reopen --
        {
            let log = working.join("shutdown-reopen-gui.log");
            let project = working.join("shutdown-normal-project");
            let mut gui = start_gui(workspace, home, &log, working, "reopen")?;
            let vm_url = wait_for_vm(&log, &mut gui, &mut fixture, interrupt)?;
            let driver_log = working.join("shutdown-reopen-driver.log");
            let mut driver = start_driver(
                app_dir,
                home,
                "reopen",
                &vm_url,
                &project,
                output,
                working,
                &driver_log,
            )?;
            wait_stage(
                "reopen_restored",
                working,
                &mut driver,
                &mut gui,
                &mut fixture,
                interrupt,
                Duration::from_secs(300),
            )?;
            let (native_pid, start_ticks, children) = arm_native(
                "reopen",
                "reopen_exit_requested",
                output,
                interrupt,
                working,
                &mut driver,
                &mut gui,
                &mut fixture,
            )?;
            let arm = Instant::now();
            let native = wait_native_exit(
                "reopen",
                working,
                output,
                native_pid,
                start_ticks,
                &mut gui,
                interrupt,
                EXIT_GRACE,
                arm,
            )?;
            let driver_exit = wait_driver(
                "reopen",
                working,
                &mut driver,
                interrupt,
                Duration::from_secs(180),
            )?;
            driver_evidence.push(DriverEvidence {
                log: driver_log,
                stem: "shutdown-reopen".into(),
                exit: Some(driver_exit),
            });
            phases.push(PhaseRecord {
                name: "reopen",
                status: "executed",
                coverage: "executed",
                detail: format!(
                    "reopened the same home and verified the restored Thread, committed answer and settled persistence: {}",
                    native.summary()
                ),
                exit_code: native.code,
                elapsed_millis: Some(native.elapsed.as_millis()),
                native_pid: Some(native.pid),
                owned_children: children,
            });
            ensure!(
                native.code == Some(0) && native.signal.is_none(),
                "reopen native exit was not code 0: {}",
                native.summary()
            );
            ensure!(
                native.reclaimed_descendants.is_empty(),
                "reopen native exit left a child tree for the harness: {:?}",
                native.reclaimed_descendants
            );
            write_sanitized_log(&log, &output.join("shutdown-reopen-gui.log"))?;
        }

        // ------------------------------------------------------------- busy --
        {
            let log = working.join("shutdown-busy-first-gui.log");
            let project = working.join("shutdown-normal-project");
            let mut first = start_gui(workspace, home, &log, working, "busy-hold")?;
            let vm_url = wait_for_vm(&log, &mut first, &mut fixture, interrupt)?;
            let first_log = working.join("shutdown-busy-first-driver.log");
            let mut first_driver = start_driver(
                app_dir,
                home,
                "busy-hold",
                &vm_url,
                &project,
                output,
                working,
                &first_log,
            )?;
            wait_stage(
                "busy_hold_reopened",
                working,
                &mut first_driver,
                &mut first,
                &mut fixture,
                interrupt,
                Duration::from_secs(300),
            )?;

            // The second process shares the same home. It may fail to publish a
            // usable Driver (allowed), so both the Driver snapshot and the GUI
            // log are captured as the instance-busy evidence. It renders into an
            // isolated Xvfb so the delete-event close can exercise the native GTK
            // hook without touching the user's desktop.
            let second_log = working.join("shutdown-busy-second-gui.log");
            let (mut xvfb, display) = start_isolated_xvfb(working)?;
            let second = start_gui_with(
                workspace,
                home,
                &second_log,
                working,
                "busy-second",
                &[("DISPLAY", display.as_str())],
                None,
                None,
            )?;
            let busy_evidence = capture_second_instance(
                app_dir,
                home,
                workspace,
                &second_log,
                output,
                working,
                &project,
                &display,
                second,
                interrupt,
                &mut driver_evidence,
            )?;
            // The isolated display server is owned by this run; stop it once the
            // second instance is done so nothing is left behind.
            let _ = force_close(&mut xvfb);
            fs::write(
                output.join("shutdown-busy-second.json"),
                serde_json::to_vec_pretty(&busy_evidence)?,
            )?;
            ensure!(
                busy_evidence["instanceBusyEvidence"] == true,
                "the second instance produced no instanceBusy evidence: {busy_evidence}"
            );
            ensure!(
                busy_evidence["argumentError"] == false,
                "the second instance logged an ArgumentError: {busy_evidence}"
            );
            // The second instance is truly closed before the first continues.
            ensure!(
                busy_evidence["closed"] == true,
                "the second instance did not close: {busy_evidence}"
            );
            ensure!(
                busy_evidence["secondNativeReport"]["exitCode"].as_i64() == Some(0)
                    && busy_evidence["secondNativeReport"]["signal"].is_null(),
                "the second instance had no real clean OS exit report: {busy_evidence}"
            );
            ensure!(
                busy_evidence["secondNativeReport"]["reclaimedDescendants"]
                    .as_array()
                    .is_some_and(Vec::is_empty),
                "the second instance left a child tree for the harness: {busy_evidence}"
            );
            // The second instance must have been closed by the real
            // `WM_DELETE_WINDOW` on its isolated display, not by a fallback: the
            // delete-event pid is the same native host the launcher reports, it
            // is start-time-guarded gone, and that host exited with code 0.
            ensure!(
                busy_evidence["deleteEventPid"]
                    .as_u64()
                    .is_some_and(|pid| pid > 0),
                "no real WM_DELETE_WINDOW was delivered to the second instance: {busy_evidence}"
            );
            ensure!(
                busy_evidence["deleteEventMatchesNative"] == true,
                "the WM_DELETE_WINDOW target was not the launcher-reported native host: {busy_evidence}"
            );
            ensure!(
                busy_evidence["deleteEventNativeExited"] == true,
                "the WM_DELETE_WINDOW native host did not exit: {busy_evidence}"
            );
            ensure!(
                busy_evidence["secondNativeExitCode"].as_i64() == Some(0),
                "the WM_DELETE_WINDOW native host did not exit with code 0: {busy_evidence}"
            );
            fs::write(working.join("shutdown-signal-busy-second-closed"), "closed")?;

            wait_stage(
                "busy_continued",
                working,
                &mut first_driver,
                &mut first,
                &mut fixture,
                interrupt,
                Duration::from_secs(300),
            )?;
            let (native_pid, start_ticks, children) = arm_native(
                "busy-hold",
                "busy_exit_requested",
                output,
                interrupt,
                working,
                &mut first_driver,
                &mut first,
                &mut fixture,
            )?;
            let arm = Instant::now();
            let native = wait_native_exit(
                "busy-hold",
                working,
                output,
                native_pid,
                start_ticks,
                &mut first,
                interrupt,
                EXIT_GRACE,
                arm,
            )?;
            let driver_exit = wait_driver(
                "busy",
                working,
                &mut first_driver,
                interrupt,
                Duration::from_secs(180),
            )?;
            driver_evidence.push(DriverEvidence {
                log: first_log,
                stem: "shutdown-busy-first".into(),
                exit: Some(driver_exit),
            });
            phases.push(PhaseRecord {
                name: "busy",
                status: "executed",
                coverage: "executed",
                detail: format!(
                    "second instance on the same home recorded the instance lock; after it closed the first instance submitted and saved another turn: {}",
                    native.summary()
                ),
                exit_code: native.code,
                elapsed_millis: Some(native.elapsed.as_millis()),
                native_pid: Some(native.pid),
                owned_children: children,
            });
            ensure!(
                native.code == Some(0) && native.signal.is_none(),
                "first instance exit after the second was closed was not code 0: {}",
                native.summary()
            );
            ensure!(
                native.reclaimed_descendants.is_empty(),
                "first instance exit left a child tree for the harness: {:?}",
                native.reclaimed_descendants
            );
            write_sanitized_log(&log, &output.join("shutdown-busy-first-gui.log"))?;
        }

        // ------------------------------------------------------ busy-reopen --
        {
            // Reopen the same home and prove the turn the first instance saved
            // after the second was truly closed is restored from durable history.
            let log = working.join("shutdown-busy-reopen-gui.log");
            let project = working.join("shutdown-normal-project");
            let mut gui = start_gui(workspace, home, &log, working, "busy-reopen")?;
            let vm_url = wait_for_vm(&log, &mut gui, &mut fixture, interrupt)?;
            let driver_log = working.join("shutdown-busy-reopen-driver.log");
            let mut driver = start_driver(
                app_dir,
                home,
                "busy-reopen",
                &vm_url,
                &project,
                output,
                working,
                &driver_log,
            )?;
            wait_stage(
                "busy-reopen_restored",
                working,
                &mut driver,
                &mut gui,
                &mut fixture,
                interrupt,
                Duration::from_secs(300),
            )?;
            let (native_pid, start_ticks, children) = arm_native(
                "busy-reopen",
                "busy-reopen_exit_requested",
                output,
                interrupt,
                working,
                &mut driver,
                &mut gui,
                &mut fixture,
            )?;
            let arm = Instant::now();
            let native = wait_native_exit(
                "busy-reopen",
                working,
                output,
                native_pid,
                start_ticks,
                &mut gui,
                interrupt,
                EXIT_GRACE,
                arm,
            )?;
            let driver_exit = wait_driver(
                "busy-reopen",
                working,
                &mut driver,
                interrupt,
                Duration::from_secs(180),
            )?;
            driver_evidence.push(DriverEvidence {
                log: driver_log,
                stem: "shutdown-busy-reopen".into(),
                exit: Some(driver_exit),
            });
            phases.push(PhaseRecord {
                name: "busy-reopen",
                status: "executed",
                coverage: "executed",
                detail: format!(
                    "reopened the same home and verified the turn saved after the second instance closed was restored: {}",
                    native.summary()
                ),
                exit_code: native.code,
                elapsed_millis: Some(native.elapsed.as_millis()),
                native_pid: Some(native.pid),
                owned_children: children,
            });
            ensure!(
                native.code == Some(0) && native.signal.is_none(),
                "busy-reopen native exit was not code 0: {}",
                native.summary()
            );
            ensure!(
                native.reclaimed_descendants.is_empty(),
                "busy-reopen native exit left a child tree for the harness: {:?}",
                native.reclaimed_descendants
            );
            write_sanitized_log(&log, &output.join("shutdown-busy-reopen-gui.log"))?;
        }

        // -------------------------------------------------------- duplicate --
        {
            // The repeated close is driven through the existing Driver-only
            // subscription-fault entrypoint: its faulty progress subscription
            // emits a typed `progress` issue and its cancel stays pending for
            // the coordinator's bounded 2s, so the single central cleanup
            // genuinely outlives the 500ms between the two repeated requests.
            // No production interface or Driver command is added; the fixture
            // only reuses the entrypoint the subscription-fault phase already
            // needs, which is what makes the second delivery real.
            let log = working.join("shutdown-duplicate-gui.log");
            let project = prepare_project(working, "shutdown-duplicate-project")?;
            let mut gui = start_gui_with(
                workspace,
                &home_duplicate,
                &log,
                working,
                "duplicate",
                &[],
                Some("test_driver/shutdown_fault_driver.dart"),
                None,
            )?;
            let vm_url = wait_for_vm(&log, &mut gui, &mut fixture, interrupt)?;
            let driver_log = working.join("shutdown-duplicate-driver.log");
            let mut driver = start_driver(
                app_dir,
                &home_duplicate,
                "duplicate",
                &vm_url,
                &project,
                output,
                working,
                &driver_log,
            )?;
            // The arm marker fires at the first native arm; the journey then
            // waits several seconds and repeats the close/request inside the
            // same shared deadline. The total is measured from this first arm.
            let (native_pid, start_ticks, children) = arm_native(
                "duplicate",
                "duplicate_exit_requested",
                output,
                interrupt,
                working,
                &mut driver,
                &mut gui,
                &mut fixture,
            )?;
            let arm = Instant::now();
            let native = wait_native_exit(
                "duplicate",
                working,
                output,
                native_pid,
                start_ticks,
                &mut gui,
                interrupt,
                EXIT_GRACE,
                arm,
            )?;
            let driver_exit = wait_driver(
                "duplicate",
                working,
                &mut driver,
                interrupt,
                Duration::from_secs(120),
            )?;
            driver_evidence.push(DriverEvidence {
                log: driver_log,
                stem: "shutdown-duplicate".into(),
                exit: Some(driver_exit),
            });
            // The repeated close's proof is judged from the Driver's real
            // readings, not from the fact that a command was sent: the second
            // `beginExit` inside `request-app-exit-twice` must report the shared
            // budget still decreasing, and the read-only native projection must
            // show that both requests reached the same budget. A refreshed
            // deadline, a missing read, or a request that was never delivered
            // all fail the phase instead of being recorded as a pass.
            let summary = read_phase_json(&output.join("shutdown-duplicate-summary.json"))?;
            ensure!(
                summary["checks"]["duplicateDeadlineNotRefreshed"] == true,
                "the repeated exit request refreshed the single deadline: {summary}"
            );
            ensure!(
                summary["checks"]["duplicateRequestsAtLeastTwo"] == true,
                "the repeated exit request was not delivered twice on the shared deadline: {summary}"
            );
            // The reused fault entrypoint makes the cleanup report Degraded, so
            // the real exit must be the non-zero Degraded code below; the typed
            // report fields are recorded as corroborating evidence (the report is
            // published just before `finishExit`, so it can race the read).
            fs::write(
                output.join("shutdown-duplicate-deadline.json"),
                serde_json::to_vec_pretty(&serde_json::json!({
                    "armedRemainingMs": summary["observations"]["duplicateArmedRemainingMs"],
                    "twiceArmedRemainingMs": summary["observations"]["duplicateTwiceArmedRemainingMs"],
                    "postRemainingMs": summary["observations"]["duplicatePostRemainingMs"],
                    "maxRequests": summary["observations"]["duplicateMaxRequests"],
                    "requestsAtLeastTwo": summary["checks"]["duplicateRequestsAtLeastTwo"],
                    "deadlineNotRefreshed": summary["checks"]["duplicateDeadlineNotRefreshed"],
                    "reportOutcome": summary["observations"]["duplicateReportOutcome"],
                    "reportPersistence": summary["observations"]["duplicateReportPersistence"],
                    "nativeElapsedMillis": native.elapsed.as_millis(),
                    "budgetMillis": EXIT_BUDGET.as_millis(),
                }))?,
            )?;
            phases.push(PhaseRecord {
                name: "duplicate",
                status: "executed",
                coverage: "executed",
                detail: format!(
                    "a repeated close/request several seconds after the first arm was delivered twice on the same single 30-second budget (its length kept decreasing) and the reused fault entrypoint reported Degraded: {}",
                    native.summary()
                ),
                exit_code: native.code,
                elapsed_millis: Some(native.elapsed.as_millis()),
                native_pid: Some(native.pid),
                owned_children: children,
            });
            ensure!(
                native.code == Some(1) && native.signal.is_none(),
                "duplicate-exit native exit was not the expected code 1 (Degraded): {}",
                native.summary()
            );
            ensure!(
                native.elapsed <= EXIT_BUDGET + Duration::from_secs(1),
                "duplicated exit request ran past the single budget measured from the first arm: {:?}",
                native.elapsed
            );
            ensure!(
                native.reclaimed_descendants.is_empty(),
                "duplicate-exit native exit left a child tree for the harness: {:?}",
                native.reclaimed_descendants
            );
            write_sanitized_log(&log, &output.join("shutdown-duplicate-gui.log"))?;
        }

        // ------------------------------------------------------------- hang --
        {
            let log = working.join("shutdown-hang-gui.log");
            let project = prepare_project(working, "shutdown-hang-project")?;
            let mut gui = start_gui(workspace, &home_hang, &log, working, "hang")?;
            let vm_url = wait_for_vm(&log, &mut gui, &mut fixture, interrupt)?;
            let driver_log = working.join("shutdown-hang-driver.log");
            let mut driver = start_driver(
                app_dir,
                &home_hang,
                "hang",
                &vm_url,
                &project,
                output,
                working,
                &driver_log,
            )?;
            let (native_pid, start_ticks, children) = arm_native(
                "hang",
                "hang_exit_requested",
                output,
                interrupt,
                working,
                &mut driver,
                &mut gui,
                &mut fixture,
            )?;
            let arm = Instant::now();
            // The Driver hangs its isolate after arming; the coordinator's own
            // native OS wait is the only authoritative signal and must see the
            // host force exit 1 at the single 30-second deadline.
            let native = wait_native_exit(
                "hang",
                working,
                output,
                native_pid,
                start_ticks,
                &mut gui,
                interrupt,
                EXIT_GRACE,
                arm,
            )?;
            // The hung Driver isolate never answers; reclaim its process rather
            // than waiting on a request that cannot complete.
            let driver_reclaimed = force_close(&mut driver);
            driver_evidence.push(DriverEvidence {
                log: driver_log,
                stem: "shutdown-hang".into(),
                exit: None,
            });
            let stage = fs::read_to_string(working.join("shutdown-stage"))
                .unwrap_or_else(|_| "unknown".into());
            fs::write(
                output.join("shutdown-hang-trace.json"),
                serde_json::to_vec_pretty(&serde_json::json!({
                    "nativePid": native_pid,
                    "nativeExitCode": native.code,
                    "nativeSignal": native.signal,
                    "elapsedMillis": native.elapsed.as_millis(),
                    "stage": stage,
                    "budgetMillis": EXIT_BUDGET.as_millis(),
                    "driverReclaimed": driver_reclaimed,
                    "reclaimedDescendants": native.reclaimed_descendants,
                }))?,
            )?;
            phases.push(PhaseRecord {
                name: "hang",
                status: "executed",
                coverage: "executed",
                detail: format!(
                    "the hung Driver isolate was forced out by the native 30-second deadline: {}",
                    native.summary()
                ),
                exit_code: native.code,
                elapsed_millis: Some(native.elapsed.as_millis()),
                native_pid: Some(native.pid),
                owned_children: children,
            });
            ensure!(
                native.code == Some(1) && native.signal.is_none(),
                "hung shutdown native exit was not code 1: {}",
                native.summary()
            );
            ensure!(
                native.elapsed <= EXIT_BUDGET + Duration::from_secs(1)
                    && native.elapsed + Duration::from_secs(1) >= EXIT_BUDGET,
                "hung shutdown did not exit at the single 30-second deadline (measured from the real arm): {:?}",
                native.elapsed
            );
            write_sanitized_log(&log, &output.join("shutdown-hang-gui.log"))?;
        }

        // ------------------------------------------------- runtime-unavailable --
        {
            let log = working.join("shutdown-unavailable-gui.log");
            let project = prepare_project(working, "shutdown-unavailable-project")?;
            let mut gui = start_gui(
                workspace,
                &home_unavailable,
                &log,
                working,
                "runtime-unavailable",
            )?;
            let vm_url = wait_for_vm(&log, &mut gui, &mut fixture, interrupt)?;
            let driver_log = working.join("shutdown-unavailable-driver.log");
            let mut driver = start_driver(
                app_dir,
                &home_unavailable,
                "runtime-unavailable",
                &vm_url,
                &project,
                output,
                working,
                &driver_log,
            )?;
            wait_stage(
                "runtime_unavailable_observed",
                working,
                &mut driver,
                &mut gui,
                &mut fixture,
                interrupt,
                Duration::from_secs(180),
            )?;
            let (native_pid, start_ticks, children) = arm_native(
                "runtime-unavailable",
                "runtime_unavailable_exit_requested",
                output,
                interrupt,
                working,
                &mut driver,
                &mut gui,
                &mut fixture,
            )?;
            let arm = Instant::now();
            let native = wait_native_exit(
                "runtime-unavailable",
                working,
                output,
                native_pid,
                start_ticks,
                &mut gui,
                interrupt,
                EXIT_GRACE,
                arm,
            )?;
            let driver_exit = wait_driver(
                "runtime-unavailable",
                working,
                &mut driver,
                interrupt,
                Duration::from_secs(120),
            )?;
            driver_evidence.push(DriverEvidence {
                log: driver_log,
                stem: "shutdown-unavailable".into(),
                exit: Some(driver_exit),
            });
            phases.push(PhaseRecord {
                name: "runtime-unavailable",
                status: "executed",
                coverage: "executed",
                detail: format!(
                    "a real typed startup failure (v2 data root uncreatable) still rendered the fatal widget and closed through the real exit path: {}",
                    native.summary()
                ),
                exit_code: native.code,
                elapsed_millis: Some(native.elapsed.as_millis()),
                native_pid: Some(native.pid),
                owned_children: children,
            });
            ensure!(
                native.code == Some(0) && native.signal.is_none(),
                "runtime-unavailable native exit was not code 0: {}",
                native.summary()
            );
            ensure!(
                native.reclaimed_descendants.is_empty(),
                "runtime-unavailable native exit left a child tree for the harness: {:?}",
                native.reclaimed_descendants
            );
            write_sanitized_log(&log, &output.join("shutdown-unavailable-gui.log"))?;
        }

        // ------------------------------------------------------- storage lock --
        {
            let log = working.join("shutdown-storage-prime-gui.log");
            let project = prepare_project(working, "shutdown-storage-project")?;
            let mut gui = start_gui(workspace, &home_storage, &log, working, "storage-lock")?;
            let vm_url = wait_for_vm(&log, &mut gui, &mut fixture, interrupt)?;
            let prime_driver_log = working.join("shutdown-storage-prime-driver.log");
            let mut prime_driver = start_driver(
                app_dir,
                &home_storage,
                "storage-lock",
                &vm_url,
                &project,
                output,
                working,
                &prime_driver_log,
            )?;
            wait_stage(
                "storage_prime_settled",
                working,
                &mut prime_driver,
                &mut gui,
                &mut fixture,
                interrupt,
                Duration::from_secs(300),
            )?;
            let database = history_lock_database(&home_storage)?;
            let before = history_lock_watermark(&database)?;
            // Take the exclusive writer lock, then let the app start the paced
            // turn and only afterwards ask it to exit. The lock is released after
            // the forced exit so the reopen can read the exact file back.
            let mut lock = SqliteWriteLock::acquire(
                &database,
                "shutdown history.sqlite",
                Duration::from_secs(120),
            )?;
            fs::write(working.join("shutdown-signal-storage-lock-held"), "held")?;
            wait_stage(
                "storage_blocked",
                working,
                &mut prime_driver,
                &mut gui,
                &mut fixture,
                interrupt,
                Duration::from_secs(180),
            )?;
            fs::write(working.join("shutdown-signal-storage-exit"), "exit")?;
            let (native_pid, start_ticks, storage_children) = arm_native(
                "storage-lock",
                "storage_exit_requested",
                output,
                interrupt,
                working,
                &mut prime_driver,
                &mut gui,
                &mut fixture,
            )?;
            let arm = Instant::now();
            let native = wait_native_exit(
                "storage-lock",
                working,
                output,
                native_pid,
                start_ticks,
                &mut gui,
                interrupt,
                EXIT_GRACE,
                arm,
            )?;
            // Release the lock only after the process is gone: while it is held the
            // writer cannot commit, which is the refusal the acceptance observes.
            lock.finish()?;
            let prime_driver_exit = wait_driver(
                "storage-prime",
                working,
                &mut prime_driver,
                interrupt,
                Duration::from_secs(60),
            )?;
            driver_evidence.push(DriverEvidence {
                log: prime_driver_log,
                stem: "shutdown-storage-prime".into(),
                exit: Some(prime_driver_exit),
            });
            write_sanitized_log(&log, &output.join("shutdown-storage-prime-gui.log"))?;

            // Reopen and verify the prime survived and the database still opens.
            let verify_log = working.join("shutdown-storage-verify-gui.log");
            let mut verify = start_gui(
                workspace,
                &home_storage,
                &verify_log,
                working,
                "storage-verify",
            )?;
            let vm_url = wait_for_vm(&verify_log, &mut verify, &mut fixture, interrupt)?;
            let verify_driver_log = working.join("shutdown-storage-verify-driver.log");
            let mut verify_driver = start_driver(
                app_dir,
                &home_storage,
                "storage-verify",
                &vm_url,
                &project,
                output,
                working,
                &verify_driver_log,
            )?;
            wait_stage(
                "storage-verify_restored",
                working,
                &mut verify_driver,
                &mut verify,
                &mut fixture,
                interrupt,
                Duration::from_secs(300),
            )?;
            let (verify_pid, verify_ticks, verify_children) = arm_native(
                "storage-verify",
                "storage-verify_exit_requested",
                output,
                interrupt,
                working,
                &mut verify_driver,
                &mut verify,
                &mut fixture,
            )?;
            let verify_arm = Instant::now();
            let verify_native = wait_native_exit(
                "storage-verify",
                working,
                output,
                verify_pid,
                verify_ticks,
                &mut verify,
                interrupt,
                EXIT_GRACE,
                verify_arm,
            )?;
            let verify_driver_exit = wait_driver(
                "storage-verify",
                working,
                &mut verify_driver,
                interrupt,
                Duration::from_secs(120),
            )?;
            driver_evidence.push(DriverEvidence {
                log: verify_driver_log,
                stem: "shutdown-storage-verify".into(),
                exit: Some(verify_driver_exit),
            });
            let database = history_lock_database(&home_storage)?;
            let after = history_lock_watermark(&database)?;
            let quick_check = history_lock_quick_check(&database)?;
            phases.push(PhaseRecord {
                name: "storage-lock",
                status: "executed",
                coverage: "executed",
                detail: format!(
                    "writer lock blocked the paced turn; forced native exit {:?}; reopened with writer watermark {before:?} -> {after:?}, quick_check={quick_check}",
                    native.code
                ),
                exit_code: native.code,
                elapsed_millis: Some(native.elapsed.as_millis()),
                native_pid: Some(native.pid),
                owned_children: storage_children,
            });
            fs::write(
                output.join("shutdown-storage-lock.json"),
                serde_json::to_vec_pretty(&serde_json::json!({
                    "blockedExitCode": native.code,
                    "blockedExitSignal": native.signal,
                    "blockedElapsedMillis": native.elapsed.as_millis(),
                    "blockedReclaimedDescendants": native.reclaimed_descendants,
                    "before": before,
                    "after": after,
                    "verifyQuickCheck": quick_check,
                    "verifiedExitCode": verify_native.code,
                    "verifiedExitSignal": verify_native.signal,
                    "verifiedNativePid": verify_native.pid,
                    "verifiedOwnedChildren": verify_children,
                }))?,
            )?;
            ensure!(
                verify_native.code == Some(0) && verify_native.signal.is_none(),
                "storage verify native exit was not code 0: {}",
                verify_native.summary()
            );
            ensure!(
                quick_check == "ok",
                "reopened history database failed PRAGMA quick_check: {quick_check}"
            );
            // The blocked write is a real refusal: the app cannot exit 0 while
            // the writer lock is held, so a clean forced exit here is either a
            // drained (still recorded) or degraded (exit 1) shutdown, never a
            // fabricated `Stopped`.
            ensure!(
                matches!(native.code, Some(0) | Some(1)),
                "storage blocked exit recorded no real exit code: {}",
                native.summary()
            );
            ensure!(
                after.turns >= before.turns && after.turns >= 1,
                "the durable prime turn was lost across the forced exit: {before:?} -> {after:?}"
            );
            write_sanitized_log(&verify_log, &output.join("shutdown-storage-verify-gui.log"))?;
        }

        // ---------------------------------------------- service unresponsive --
        {
            let log = working.join("shutdown-service-gui.log");
            let project = prepare_project(working, "shutdown-service-project")?;
            let mut gui = start_gui(
                workspace,
                &home_service,
                &log,
                working,
                "service-unresponsive",
            )?;
            let vm_url = wait_for_vm(&log, &mut gui, &mut fixture, interrupt)?;
            let driver_log = working.join("shutdown-service-driver.log");
            let mut driver = start_driver(
                app_dir,
                &home_service,
                "service-unresponsive",
                &vm_url,
                &project,
                output,
                working,
                &driver_log,
            )?;
            wait_stage(
                "service_inflight",
                working,
                &mut driver,
                &mut gui,
                &mut fixture,
                interrupt,
                Duration::from_secs(300),
            )?;
            // Freeze the real provider fixture mid-turn so it is genuinely
            // unresponsive; the exit must not wait on it. The Driver only marks
            // `service_inflight` after the first paced chunk arrived, which
            // proves the request was accepted before the freeze.
            freeze_fixture(&fixture)?;
            fixture_paused = true;
            fs::write(working.join("shutdown-signal-service-stopped"), "stopped")?;
            let (native_pid, start_ticks, children) = arm_native(
                "service-unresponsive",
                "service_exit_requested",
                output,
                interrupt,
                working,
                &mut driver,
                &mut gui,
                &mut fixture,
            )?;
            let arm = Instant::now();
            let native = wait_native_exit(
                "service-unresponsive",
                working,
                output,
                native_pid,
                start_ticks,
                &mut gui,
                interrupt,
                EXIT_GRACE,
                arm,
            )?;
            resume_fixture(&fixture)?;
            fixture_paused = false;
            let driver_exit = wait_driver(
                "service",
                working,
                &mut driver,
                interrupt,
                Duration::from_secs(60),
            )?;
            driver_evidence.push(DriverEvidence {
                log: driver_log,
                stem: "shutdown-service".into(),
                exit: Some(driver_exit),
            });
            phases.push(PhaseRecord {
                name: "service-unresponsive",
                status: "executed",
                coverage: "executed",
                detail: format!(
                    "the frozen provider fixture did not block the real app exit: {}",
                    native.summary()
                ),
                exit_code: native.code,
                elapsed_millis: Some(native.elapsed.as_millis()),
                native_pid: Some(native.pid),
                owned_children: children,
            });
            ensure!(
                native.code == Some(0) && native.signal.is_none(),
                "unresponsive-service native exit was not code 0: {}",
                native.summary()
            );
            ensure!(
                native.reclaimed_descendants.is_empty(),
                "unresponsive-service native exit left a child tree for the harness: {:?}",
                native.reclaimed_descendants
            );
            write_sanitized_log(&log, &output.join("shutdown-service-gui.log"))?;
        }

        // ------------------------------------------------- close during init --
        {
            // Driver-only injected fault: the runtime never leaves the
            // initializing phase (a never-completing Completer through the
            // existing `FrbStudioApi.debugOverrideInitialization`). An exit
            // requested now must report Degraded + Unknown and force exit 1,
            // never a fabricated NotStarted/Stopped. The injection is a
            // Driver-only test override applied inside `driver_main.dart`
            // (`_applyShutdownFaultOverride`), not a production fault API.
            let log = working.join("shutdown-init-close-gui.log");
            let project = prepare_project(working, "shutdown-init-close-project")?;
            let mut gui = start_gui_with(
                workspace,
                &home_init_close,
                &log,
                working,
                "init-close",
                &[("ANYWORK_DRIVER_SHUTDOWN_FAULT", "pending-init")],
                None,
                None,
            )?;
            let vm_url = wait_for_vm(&log, &mut gui, &mut fixture, interrupt)?;
            let driver_log = working.join("shutdown-init-close-driver.log");
            let mut driver = start_driver(
                app_dir,
                &home_init_close,
                "close-during-init",
                &vm_url,
                &project,
                output,
                working,
                &driver_log,
            )?;
            let (native_pid, start_ticks, children) = arm_native(
                "init-close",
                "init-close_exit_requested",
                output,
                interrupt,
                working,
                &mut driver,
                &mut gui,
                &mut fixture,
            )?;
            let arm = Instant::now();
            let native = wait_native_exit(
                "init-close",
                working,
                output,
                native_pid,
                start_ticks,
                &mut gui,
                interrupt,
                EXIT_GRACE,
                arm,
            )?;
            let driver_exit = wait_driver(
                "close-during-init",
                working,
                &mut driver,
                interrupt,
                Duration::from_secs(180),
            )?;
            driver_evidence.push(DriverEvidence {
                log: driver_log,
                stem: "shutdown-init-close".into(),
                exit: Some(driver_exit),
            });
            let ack = read_phase_json(&output.join("shutdown-close-during-init-ack.json"))?;
            phases.push(PhaseRecord {
                name: "close-during-init",
                status: "executed",
                coverage: "executed",
                detail: format!(
                    "an exit requested while initialization was permanently pending reported degraded/unknown and forced exit 1 (typed cleanup ack: {ack}): {}",
                    native.summary()
                ),
                exit_code: native.code,
                elapsed_millis: Some(native.elapsed.as_millis()),
                native_pid: Some(native.pid),
                owned_children: children,
            });
            ensure!(
                native.code == Some(1) && native.signal.is_none(),
                "close-during-init native exit was not code 1: {}",
                native.summary()
            );
            ensure!(
                ack["shutdown"] == "degraded"
                    && ack["persistence"] == "unknown"
                    && ack["outcome"] != "notStarted",
                "close-during-init typed report was not degraded/unknown: {ack}"
            );
            write_sanitized_log(&log, &output.join("shutdown-init-close-gui.log"))?;
        }

        // ------------------------------------------------- bridge-unavailable --
        {
            // Driver-only injected fault at the same override point: the
            // initialization override throws before `RustLib.init()`, so no
            // runtime owner ever installs. This is a Driver-only injected init
            // failure (`FrbStudioApi.debugOverrideInitialization` inside
            // `driver_main.dart`), NOT a real dynamic-library `dlopen` failure;
            // the real repository build refuses a missing bridge at CMake
            // configure time. The exit must still be a clean NotStarted exit 0.
            let log = working.join("shutdown-bridge-gui.log");
            let project = prepare_project(working, "shutdown-bridge-project")?;
            let mut gui = start_gui_with(
                workspace,
                &home_bridge,
                &log,
                working,
                "bridge",
                &[("ANYWORK_DRIVER_SHUTDOWN_FAULT", "bridge-load-error")],
                None,
                None,
            )?;
            let vm_url = wait_for_vm(&log, &mut gui, &mut fixture, interrupt)?;
            let driver_log = working.join("shutdown-bridge-driver.log");
            let mut driver = start_driver(
                app_dir,
                &home_bridge,
                "bridge-unavailable",
                &vm_url,
                &project,
                output,
                working,
                &driver_log,
            )?;
            let (native_pid, start_ticks, children) = arm_native(
                "bridge",
                "bridge_exit_requested",
                output,
                interrupt,
                working,
                &mut driver,
                &mut gui,
                &mut fixture,
            )?;
            let arm = Instant::now();
            let native = wait_native_exit(
                "bridge",
                working,
                output,
                native_pid,
                start_ticks,
                &mut gui,
                interrupt,
                EXIT_GRACE,
                arm,
            )?;
            let driver_exit = wait_driver(
                "bridge-unavailable",
                working,
                &mut driver,
                interrupt,
                Duration::from_secs(180),
            )?;
            driver_evidence.push(DriverEvidence {
                log: driver_log,
                stem: "shutdown-bridge".into(),
                exit: Some(driver_exit),
            });
            let ack = read_phase_json(&output.join("shutdown-bridge-unavailable-ack.json"))?;
            phases.push(PhaseRecord {
                name: "bridge-unavailable",
                status: "executed",
                coverage: "executed",
                detail: format!(
                    "a Driver-only injected bridge-load failure (init settled with no runtime owner) closed as NotStarted with exit 0 (typed cleanup ack: {ack}): {}",
                    native.summary()
                ),
                exit_code: native.code,
                elapsed_millis: Some(native.elapsed.as_millis()),
                native_pid: Some(native.pid),
                owned_children: children,
            });
            ensure!(
                native.code == Some(0) && native.signal.is_none(),
                "bridge-unavailable native exit was not code 0: {}",
                native.summary()
            );
            ensure!(
                ack["outcome"] == "notStarted" && ack["shutdown"] == "completed",
                "bridge-unavailable typed report was not NotStarted/clean-exit: {ack}"
            );
            write_sanitized_log(&log, &output.join("shutdown-bridge-gui.log"))?;
        }

        // ------------------------------------------------- subscription fault --
        {
            // Driver-only fault entrypoint: the same central exit coordinator
            // over a real `FrbStudioApi` subclass whose `subscribeShutdownProgress`
            // errors and then hangs on cancel. The progress error must become a
            // typed `progress` issue (Degraded), never a fabricated Stopped, and
            // the exit must be non-zero.
            let log = working.join("shutdown-subscription-gui.log");
            let project = prepare_project(working, "shutdown-subscription-project")?;
            let mut gui = start_gui_with(
                workspace,
                &home_subscription,
                &log,
                working,
                "subscription-fault",
                &[],
                Some("test_driver/shutdown_fault_driver.dart"),
                None,
            )?;
            let vm_url = wait_for_vm(&log, &mut gui, &mut fixture, interrupt)?;
            let driver_log = working.join("shutdown-subscription-driver.log");
            let mut driver = start_driver(
                app_dir,
                &home_subscription,
                "subscription-fault",
                &vm_url,
                &project,
                output,
                working,
                &driver_log,
            )?;
            let (native_pid, start_ticks, children) = arm_native(
                "subscription-fault",
                "subscription-fault_exit_requested",
                output,
                interrupt,
                working,
                &mut driver,
                &mut gui,
                &mut fixture,
            )?;
            let arm = Instant::now();
            let native = wait_native_exit(
                "subscription-fault",
                working,
                output,
                native_pid,
                start_ticks,
                &mut gui,
                interrupt,
                EXIT_GRACE,
                arm,
            )?;
            let driver_exit = wait_driver(
                "subscription-fault",
                working,
                &mut driver,
                interrupt,
                Duration::from_secs(180),
            )?;
            driver_evidence.push(DriverEvidence {
                log: driver_log,
                stem: "shutdown-subscription-fault".into(),
                exit: Some(driver_exit),
            });
            // The typed progress issue is only observable through the canonical
            // diagnostics log (the app exits before the report can be read), so
            // the copy is machine-validated rather than merely stashed.
            copy_home_diagnostics(&home_subscription, output, "subscription-fault")?;
            let log_check = validate_dart_error_dir(
                &output.join("diagnostics").join("subscription-fault"),
                "progress",
            )?;
            phases.push(PhaseRecord {
                name: "subscription-fault",
                status: "executed",
                coverage: "executed",
                detail: format!(
                    "a Driver-only faulty shutdown-progress subscription produced a typed progress diagnostic and forced exit 1; log check {log_check}: {}",
                    native.summary()
                ),
                exit_code: native.code,
                elapsed_millis: Some(native.elapsed.as_millis()),
                native_pid: Some(native.pid),
                owned_children: children,
            });
            ensure!(
                native.code == Some(1) && native.signal.is_none(),
                "subscription-fault native exit was not code 1: {}",
                native.summary()
            );
            ensure!(
                log_check["containsStage"] == true
                    && log_check["containsCorrelation"] == true
                    && log_check["containsStack"] == true,
                "subscription-fault diagnostics did not record the typed progress issue: {log_check}"
            );
            write_sanitized_log(&log, &output.join("shutdown-subscription-gui.log"))?;
        }

        // ------------------------------------------------- dart error fallback --
        {
            // Same Driver-only fault entrypoint, but the isolated home cannot
            // create its canonical diagnostic directory, so `recordDartError`
            // must fall back to the isolated `TMPDIR`. Proves the real
            // diagnostics record (stage/correlation/stack) survives an
            // unwritable canonical path without ever touching a shared temp dir.
            let log = working.join("shutdown-tmp-fallback-gui.log");
            let project = prepare_project(working, "shutdown-tmp-fallback-project")?;
            let mut gui = start_gui_with(
                workspace,
                &home_tmp_fallback,
                &log,
                working,
                "tmp-fallback",
                &[],
                Some("test_driver/shutdown_fault_driver.dart"),
                Some(&tmp_fallback_tmp),
            )?;
            let vm_url = wait_for_vm(&log, &mut gui, &mut fixture, interrupt)?;
            let driver_log = working.join("shutdown-tmp-fallback-driver.log");
            let mut driver = start_driver(
                app_dir,
                &home_tmp_fallback,
                "dart-error-fallback",
                &vm_url,
                &project,
                output,
                working,
                &driver_log,
            )?;
            let (native_pid, start_ticks, children) = arm_native(
                "tmp-fallback",
                "tmp-fallback_exit_requested",
                output,
                interrupt,
                working,
                &mut driver,
                &mut gui,
                &mut fixture,
            )?;
            let arm = Instant::now();
            let native = wait_native_exit(
                "tmp-fallback",
                working,
                output,
                native_pid,
                start_ticks,
                &mut gui,
                interrupt,
                EXIT_GRACE,
                arm,
            )?;
            let driver_exit = wait_driver(
                "dart-error-fallback",
                working,
                &mut driver,
                interrupt,
                Duration::from_secs(180),
            )?;
            driver_evidence.push(DriverEvidence {
                log: driver_log,
                stem: "shutdown-tmp-fallback".into(),
                exit: Some(driver_exit),
            });
            // Preserve the isolated TMPDIR fallback log beside the rest of the
            // evidence so the stage/correlation/stack record is reviewable.
            let fallback_dest = output.join("diagnostics").join("tmp-fallback-temp");
            fs::create_dir_all(&fallback_dest)?;
            if tmp_fallback_tmp.is_dir() {
                for entry in fs::read_dir(&tmp_fallback_tmp)? {
                    let entry = entry?;
                    if entry.file_type()?.is_file() {
                        fs::copy(entry.path(), fallback_dest.join(entry.file_name()))?;
                    }
                }
            }
            let fallback_check = validate_dart_error_dir(&tmp_fallback_tmp, "progress")?;
            phases.push(PhaseRecord {
                name: "dart-error-fallback",
                status: "executed",
                coverage: "executed",
                detail: format!(
                    "an unwritable canonical diagnostics path fell back to the isolated TMPDIR and still recorded stage/correlation/stack (native pid {native_pid}); check {fallback_check}: {}",
                    native.summary()
                ),
                exit_code: native.code,
                elapsed_millis: Some(native.elapsed.as_millis()),
                native_pid: Some(native.pid),
                owned_children: children,
            });
            ensure!(
                native.code == Some(1) && native.signal.is_none(),
                "dart-error-fallback native exit was not code 1: {}",
                native.summary()
            );
            ensure!(
                fallback_check["containsStage"] == true
                    && fallback_check["containsCorrelation"] == true
                    && fallback_check["containsStack"] == true,
                "dart-error-fallback recorded no temp-fallback diagnostic: {fallback_check}"
            );
            write_sanitized_log(&log, &output.join("shutdown-tmp-fallback-gui.log"))?;
        }

        // ------------------------------------------------- native subtree reclamation --
        {
            // The home starts a scenario-owned stdio MCP server that spawns a
            // SIGTERM-ignoring grandchild in its own process group. After the
            // forced exit the product's own supervisor must reclaim that whole
            // subtree BEFORE this harness does: a non-empty reclaimed-descendant
            // set would mean the product leaked and the harness had to cover it.
            let log = working.join("shutdown-mcp-gui.log");
            let project = prepare_project(working, "shutdown-mcp-project")?;
            let mut gui = start_gui_with(
                workspace,
                &home_mcp,
                &log,
                working,
                "mcp-reclamation",
                &[],
                None,
                None,
            )?;
            let vm_url = wait_for_vm(&log, &mut gui, &mut fixture, interrupt)?;
            let driver_log = working.join("shutdown-mcp-driver.log");
            let mut driver = start_driver(
                app_dir,
                &home_mcp,
                "mcp-reclamation",
                &vm_url,
                &project,
                output,
                working,
                &driver_log,
            )?;
            // Wait until the Driver has proven the real MCP server subtree is
            // live, then resolve the native host and the business pids BEFORE any
            // close is requested: the alive gate and the independent monitor must
            // both exist first, or a fast normal exit could race the observation
            // and the later "pids gone" reading would prove nothing.
            wait_stage(
                "mcp-reclamation_served",
                working,
                &mut driver,
                &mut gui,
                &mut fixture,
                interrupt,
                Duration::from_secs(300),
            )?;
            let (native_pid, start_ticks, children) = snapshot_native(output, "mcp-reclamation")?;
            // The scenario server records its own and the grandchild's pid; the
            // coordinator OS-polls the real business pids with a reuse guard.
            let coord = read_phase_json(&mcp_coord)?;
            let mcp_pid = coord["mcpPid"].as_u64().unwrap_or_default() as u32;
            let mcp_ticks = coord["mcpStartTicks"].as_u64();
            let grandchild_pid = coord["grandchildPid"].as_u64().unwrap_or_default() as u32;
            let grandchild_ticks = coord["grandchildStartTicks"].as_u64();
            ensure!(
                mcp_pid > 0 && grandchild_pid > 0 && grandchild_pid != mcp_pid,
                "the scenario MCP server recorded no distinct business pids: {coord}"
            );
            // Hard pre-request gate: both business pids must be genuinely live
            // BEFORE the forced exit is requested, otherwise their later
            // disappearance would prove nothing.
            #[cfg(target_os = "linux")]
            {
                ensure!(
                    proc_alive(mcp_pid, mcp_ticks) && proc_alive(grandchild_pid, grandchild_ticks),
                    "the MCP business pids were not alive BEFORE the forced exit was requested: mcp={mcp_pid} grandchild={grandchild_pid}"
                );
            }
            let monitor = monitor_business_pids(
                vec![(mcp_pid, mcp_ticks), (grandchild_pid, grandchild_ticks)],
                EXIT_GRACE,
            );
            // Only now does the run let the isolate request the Driver-only hang;
            // the native host then arms the single 30-second deadline and forces
            // the process out (exit 1) while its supervisor reclaims the subtree.
            fs::write(
                working.join("shutdown-signal-mcp-reclamation-armed"),
                "armed",
            )?;
            wait_stage(
                "mcp-reclamation_exit_requested",
                working,
                &mut driver,
                &mut gui,
                &mut fixture,
                interrupt,
                Duration::from_secs(180),
            )?;
            let arm = Instant::now();
            let native = wait_native_exit(
                "mcp-reclamation",
                working,
                output,
                native_pid,
                start_ticks,
                &mut gui,
                interrupt,
                EXIT_GRACE,
                arm,
            )?;
            let reclamation = monitor
                .join()
                .unwrap_or_else(|_| serde_json::json!({"monitor": "panicked"}));
            // The hung Driver isolate never answers; reclaim its process instead
            // of waiting on a request that cannot complete.
            let driver_reclaimed = force_close(&mut driver);
            driver_evidence.push(DriverEvidence {
                log: driver_log,
                stem: "shutdown-mcp-reclamation".into(),
                exit: None,
            });
            // Safety net: a business pid that is still alive is a real product
            // leak already recorded above; kill it here only so the run leaves
            // no orphaned `sleep` behind, and record the forced-kill list.
            let mut forced_kill = Vec::new();
            for (pid, ticks) in [(grandchild_pid, grandchild_ticks), (mcp_pid, mcp_ticks)] {
                if proc_alive(pid, ticks) {
                    forced_kill.push(pid);
                    let _ = Command::new("kill")
                        .arg("-KILL")
                        .arg("--")
                        .arg(pid.to_string())
                        .status();
                }
            }
            fs::write(
                output.join("shutdown-mcp-reclamation.json"),
                serde_json::to_vec_pretty(&serde_json::json!({
                    "coord": coord,
                    "mcpPid": mcp_pid,
                    "grandchildPid": grandchild_pid,
                    "descendantsAtRootExit": native.descendants_at_root_exit,
                    "reclaimedDescendants": native.reclaimed_descendants,
                    "monitor": reclamation,
                    "driverReclaimed": driver_reclaimed,
                    "forcedKill": forced_kill,
                }))?,
            )?;
            phases.push(PhaseRecord {
                name: "native-subtree-reclamation",
                status: "executed",
                coverage: "executed",
                detail: format!(
                    "after the scenario-owned stdio MCP server and its SIGTERM-ignoring grandchild were proven live, the Driver isolate was blocked and the native host forced the process out (exit 1) at the single 30-second deadline while its supervisor reclaimed the whole subtree before this harness intervened: {}",
                    native.summary()
                ),
                exit_code: native.code,
                elapsed_millis: Some(native.elapsed.as_millis()),
                native_pid: Some(native.pid),
                owned_children: children,
            });
            ensure!(
                native.code == Some(1) && native.signal.is_none(),
                "mcp-reclamation native exit was not code 1 (hung isolate forced out at the fixed 30-second deadline): {}",
                native.summary()
            );
            ensure!(
                native.elapsed <= EXIT_BUDGET + Duration::from_secs(1)
                    && native.elapsed + Duration::from_secs(1) >= EXIT_BUDGET,
                "mcp-reclamation did not exit at the single 30-second deadline (measured from the real arm): {:?}",
                native.elapsed
            );
            // The strict criterion never depends on the harness kill: the product
            // must have reclaimed the subtree before the natural window closed.
            ensure!(
                native.reclaimed_descendants.is_empty(),
                "the product leaked the MCP subtree and the harness had to reclaim it: {:?}",
                native.reclaimed_descendants
            );
            ensure!(
                reclamation["stillAlive"]
                    .as_array()
                    .is_some_and(Vec::is_empty),
                "an MCP business pid was still alive at the observation deadline: {reclamation}"
            );
            write_sanitized_log(&log, &output.join("shutdown-mcp-gui.log"))?;
        }

        // ------------------------------------------------- lsp subtree reclamation --
        {
            // A real `lsp_query` tool turn makes the runtime start the configured
            // scenario-owned fake language server through its supervised worker.
            // The server speaks the LSP handshake and spawns a SIGTERM-ignoring
            // grandchild in its own process group; the product's own supervisor
            // must reclaim that whole subtree BEFORE this harness intervenes.
            let log = working.join("shutdown-lsp-gui.log");
            let project = prepare_project(working, "shutdown-lsp-project")?;
            // The `lsp_query` tool binds the workspace-relative probe file, so
            // the isolated project must actually contain it (no host approval).
            fs::write(
                project.join(pl_provider_fixture::GUI_SHUTDOWN_LSP_FILE),
                b"language: anywork-shutdown-fixture\n",
            )?;
            let mut gui = start_gui_with(
                workspace,
                &home_lsp,
                &log,
                working,
                "lsp-reclamation",
                &[],
                None,
                None,
            )?;
            let vm_url = wait_for_vm(&log, &mut gui, &mut fixture, interrupt)?;
            let driver_log = working.join("shutdown-lsp-driver.log");
            let mut driver = start_driver(
                app_dir,
                &home_lsp,
                "lsp-reclamation",
                &vm_url,
                &project,
                output,
                working,
                &driver_log,
            )?;
            // Wait until the Driver has proven the real LSP server subtree is
            // live, then resolve the native host and the business pids BEFORE any
            // close is requested: the alive gate and the independent monitor must
            // both exist first, or a fast normal exit could race the observation.
            wait_stage(
                "lsp-reclamation_served",
                working,
                &mut driver,
                &mut gui,
                &mut fixture,
                interrupt,
                Duration::from_secs(300),
            )?;
            let (native_pid, start_ticks, children) = snapshot_native(output, "lsp-reclamation")?;
            // The scenario server records its own and the grandchild's pid; the
            // coordinator OS-polls the real business pids with a reuse guard.
            let coord = read_phase_json(&lsp_coord)?;
            let lsp_pid = coord["lspPid"].as_u64().unwrap_or_default() as u32;
            let lsp_ticks = coord["lspStartTicks"].as_u64();
            let grandchild_pid = coord["grandchildPid"].as_u64().unwrap_or_default() as u32;
            let grandchild_ticks = coord["grandchildStartTicks"].as_u64();
            ensure!(
                lsp_pid > 0 && grandchild_pid > 0 && grandchild_pid != lsp_pid,
                "the scenario LSP server recorded no distinct business pids: {coord}"
            );
            // Hard pre-request gate: both business pids must be genuinely live
            // BEFORE the forced exit is requested.
            #[cfg(target_os = "linux")]
            {
                ensure!(
                    proc_alive(lsp_pid, lsp_ticks) && proc_alive(grandchild_pid, grandchild_ticks),
                    "the LSP business pids were not alive BEFORE the forced exit was requested: lsp={lsp_pid} grandchild={grandchild_pid}"
                );
            }
            let monitor = monitor_business_pids(
                vec![(lsp_pid, lsp_ticks), (grandchild_pid, grandchild_ticks)],
                EXIT_GRACE,
            );
            // Only now does the run let the isolate request the Driver-only hang;
            // the native host then arms the single 30-second deadline and forces
            // the process out (exit 1) while its supervisor reclaims the subtree.
            fs::write(
                working.join("shutdown-signal-lsp-reclamation-armed"),
                "armed",
            )?;
            wait_stage(
                "lsp-reclamation_exit_requested",
                working,
                &mut driver,
                &mut gui,
                &mut fixture,
                interrupt,
                Duration::from_secs(180),
            )?;
            let arm = Instant::now();
            let native = wait_native_exit(
                "lsp-reclamation",
                working,
                output,
                native_pid,
                start_ticks,
                &mut gui,
                interrupt,
                EXIT_GRACE,
                arm,
            )?;
            let reclamation = monitor
                .join()
                .unwrap_or_else(|_| serde_json::json!({"monitor": "panicked"}));
            // The hung Driver isolate never answers; reclaim its process instead
            // of waiting on a request that cannot complete.
            let driver_reclaimed = force_close(&mut driver);
            driver_evidence.push(DriverEvidence {
                log: driver_log,
                stem: "shutdown-lsp-reclamation".into(),
                exit: None,
            });
            // Safety net: a surviving business pid is a real product leak already
            // recorded above; kill it here only so no orphaned `sleep` is left.
            let mut forced_kill = Vec::new();
            for (pid, ticks) in [(grandchild_pid, grandchild_ticks), (lsp_pid, lsp_ticks)] {
                if proc_alive(pid, ticks) {
                    forced_kill.push(pid);
                    let _ = Command::new("kill")
                        .arg("-KILL")
                        .arg("--")
                        .arg(pid.to_string())
                        .status();
                }
            }
            fs::write(
                output.join("shutdown-lsp-reclamation.json"),
                serde_json::to_vec_pretty(&serde_json::json!({
                    "coord": coord,
                    "lspPid": lsp_pid,
                    "grandchildPid": grandchild_pid,
                    "descendantsAtRootExit": native.descendants_at_root_exit,
                    "reclaimedDescendants": native.reclaimed_descendants,
                    "monitor": reclamation,
                    "driverReclaimed": driver_reclaimed,
                    "forcedKill": forced_kill,
                }))?,
            )?;
            phases.push(PhaseRecord {
                name: "lsp-subtree-reclamation",
                status: "executed",
                coverage: "executed",
                detail: format!(
                    "after the scenario-owned fake LSP server (started by the real `lsp_query` tool turn) and its SIGTERM-ignoring grandchild were proven live, the Driver isolate was blocked and the native host forced the process out (exit 1) at the single 30-second deadline while its supervisor reclaimed the whole subtree before this harness intervened: {}",
                    native.summary()
                ),
                exit_code: native.code,
                elapsed_millis: Some(native.elapsed.as_millis()),
                native_pid: Some(native.pid),
                owned_children: children,
            });
            ensure!(
                native.code == Some(1) && native.signal.is_none(),
                "lsp-reclamation native exit was not code 1 (hung isolate forced out at the fixed 30-second deadline): {}",
                native.summary()
            );
            ensure!(
                native.elapsed <= EXIT_BUDGET + Duration::from_secs(1)
                    && native.elapsed + Duration::from_secs(1) >= EXIT_BUDGET,
                "lsp-reclamation did not exit at the single 30-second deadline (measured from the real arm): {:?}",
                native.elapsed
            );
            ensure!(
                native.reclaimed_descendants.is_empty(),
                "the product leaked the LSP subtree and the harness had to reclaim it: {:?}",
                native.reclaimed_descendants
            );
            ensure!(
                reclamation["stillAlive"]
                    .as_array()
                    .is_some_and(Vec::is_empty),
                "an LSP business pid was still alive at the observation deadline: {reclamation}"
            );
            write_sanitized_log(&log, &output.join("shutdown-lsp-gui.log"))?;
        }

        // ------------------------------------------------ tool subtree reclamation --
        {
            // A real background `exec` tool turn makes the runtime start the
            // scenario-owned `--tool-peer` through its supervised tool worker. The
            // peer spawns a SIGTERM-ignoring grandchild in its own process group
            // and keeps running; the forced exit must let the product reclaim that
            // whole subtree BEFORE this harness intervenes.
            let log = working.join("shutdown-tool-gui.log");
            let project = prepare_project(working, "shutdown-tool-project")?;
            let mut gui = start_gui_with(
                workspace,
                &home_tool,
                &log,
                working,
                "tool-reclamation",
                &[],
                None,
                None,
            )?;
            let vm_url = wait_for_vm(&log, &mut gui, &mut fixture, interrupt)?;
            let driver_log = working.join("shutdown-tool-driver.log");
            let mut driver = start_driver(
                app_dir,
                &home_tool,
                "tool-reclamation",
                &vm_url,
                &project,
                output,
                working,
                &driver_log,
            )?;
            // Wait until the Driver has proven the real background tool subtree
            // is live, then resolve the native host and the business pids BEFORE
            // any close is requested: the alive gate and the independent monitor
            // must both exist first, or a fast normal exit could race it.
            wait_stage(
                "tool-reclamation_served",
                working,
                &mut driver,
                &mut gui,
                &mut fixture,
                interrupt,
                Duration::from_secs(300),
            )?;
            let (native_pid, start_ticks, children) = snapshot_native(output, "tool-reclamation")?;
            let coord = read_phase_json(&tool_coord)?;
            let tool_pid = coord["toolPid"].as_u64().unwrap_or_default() as u32;
            let tool_ticks = coord["toolStartTicks"].as_u64();
            let grandchild_pid = coord["grandchildPid"].as_u64().unwrap_or_default() as u32;
            let grandchild_ticks = coord["grandchildStartTicks"].as_u64();
            ensure!(
                tool_pid > 0 && grandchild_pid > 0 && grandchild_pid != tool_pid,
                "the scenario background tool recorded no distinct business pids: {coord}"
            );
            // Hard pre-request gate: both business pids must be genuinely live
            // BEFORE the forced exit is requested.
            #[cfg(target_os = "linux")]
            {
                ensure!(
                    proc_alive(tool_pid, tool_ticks)
                        && proc_alive(grandchild_pid, grandchild_ticks),
                    "the tool business pids were not alive BEFORE the forced exit was requested: tool={tool_pid} grandchild={grandchild_pid}"
                );
            }
            let monitor = monitor_business_pids(
                vec![(tool_pid, tool_ticks), (grandchild_pid, grandchild_ticks)],
                EXIT_GRACE,
            );
            // Only now does the run let the isolate request the Driver-only hang;
            // the native host then arms the single 30-second deadline and forces
            // the process out (exit 1) while its supervisor reclaims the subtree.
            fs::write(
                working.join("shutdown-signal-tool-reclamation-armed"),
                "armed",
            )?;
            wait_stage(
                "tool-reclamation_exit_requested",
                working,
                &mut driver,
                &mut gui,
                &mut fixture,
                interrupt,
                Duration::from_secs(180),
            )?;
            let arm = Instant::now();
            let native = wait_native_exit(
                "tool-reclamation",
                working,
                output,
                native_pid,
                start_ticks,
                &mut gui,
                interrupt,
                EXIT_GRACE,
                arm,
            )?;
            let reclamation = monitor
                .join()
                .unwrap_or_else(|_| serde_json::json!({"monitor": "panicked"}));
            // The hung Driver isolate never answers; reclaim its process instead
            // of waiting on a request that cannot complete.
            let driver_reclaimed = force_close(&mut driver);
            driver_evidence.push(DriverEvidence {
                log: driver_log,
                stem: "shutdown-tool-reclamation".into(),
                exit: None,
            });
            let mut forced_kill = Vec::new();
            for (pid, ticks) in [(grandchild_pid, grandchild_ticks), (tool_pid, tool_ticks)] {
                if proc_alive(pid, ticks) {
                    forced_kill.push(pid);
                    let _ = Command::new("kill")
                        .arg("-KILL")
                        .arg("--")
                        .arg(pid.to_string())
                        .status();
                }
            }
            fs::write(
                output.join("shutdown-tool-reclamation.json"),
                serde_json::to_vec_pretty(&serde_json::json!({
                    "coord": coord,
                    "toolPid": tool_pid,
                    "grandchildPid": grandchild_pid,
                    "descendantsAtRootExit": native.descendants_at_root_exit,
                    "reclaimedDescendants": native.reclaimed_descendants,
                    "monitor": reclamation,
                    "driverReclaimed": driver_reclaimed,
                    "forcedKill": forced_kill,
                }))?,
            )?;
            phases.push(PhaseRecord {
                name: "tool-subtree-reclamation",
                status: "executed",
                coverage: "executed",
                detail: format!(
                    "after the scenario-owned background `exec` tool (started by the real supervised tool worker) and its SIGTERM-ignoring grandchild were proven live, the Driver isolate was blocked and the native host forced the process out (exit 1) at the single 30-second deadline while its supervisor reclaimed the whole subtree before this harness intervened: {}",
                    native.summary()
                ),
                exit_code: native.code,
                elapsed_millis: Some(native.elapsed.as_millis()),
                native_pid: Some(native.pid),
                owned_children: children,
            });
            ensure!(
                native.code == Some(1) && native.signal.is_none(),
                "tool-reclamation native exit was not code 1 (hung isolate forced out at the fixed 30-second deadline): {}",
                native.summary()
            );
            ensure!(
                native.elapsed <= EXIT_BUDGET + Duration::from_secs(1)
                    && native.elapsed + Duration::from_secs(1) >= EXIT_BUDGET,
                "tool-reclamation did not exit at the single 30-second deadline (measured from the real arm): {:?}",
                native.elapsed
            );
            ensure!(
                native.reclaimed_descendants.is_empty(),
                "the product leaked the tool subtree and the harness had to reclaim it: {:?}",
                native.reclaimed_descendants
            );
            ensure!(
                reclamation["stillAlive"]
                    .as_array()
                    .is_some_and(Vec::is_empty),
                "a tool business pid was still alive at the observation deadline: {reclamation}"
            );
            write_sanitized_log(&log, &output.join("shutdown-tool-gui.log"))?;
        }

        // ---------------------------------------------------- concurrent stop --
        {
            // One turn starts two independent supervised resources: the
            // scenario-owned stdio MCP server (through the thread's service
            // lease, because the isolated home declares it) and the background
            // `--tool-peer` (through the real supervised tool worker). The
            // coordinator holds the exclusive writer lock across the exit
            // request, so the terminal save cannot drain; the two resources must
            // be reclaimed on their own while that chain is still waiting, well
            // before the single 30-second deadline. The proof is real OS pid
            // timing with a start-time reuse guard, never the elapsed total.
            let log = working.join("shutdown-concurrent-gui.log");
            let project = prepare_project(working, "shutdown-concurrent-project")?;
            let mut gui = start_gui_with(
                workspace,
                &home_concurrent,
                &log,
                working,
                "concurrent-stop",
                &[],
                None,
                None,
            )?;
            let vm_url = wait_for_vm(&log, &mut gui, &mut fixture, interrupt)?;
            let driver_log = working.join("shutdown-concurrent-driver.log");
            let mut driver = start_driver(
                app_dir,
                &home_concurrent,
                "concurrent-stop",
                &vm_url,
                &project,
                output,
                working,
                &driver_log,
            )?;
            // Wait until the Driver has proven both independent subtrees started
            // and the concurrent turn is still in flight (its receipt
            // continuation is paced).
            wait_stage(
                "concurrent-stop_served",
                working,
                &mut driver,
                &mut gui,
                &mut fixture,
                interrupt,
                Duration::from_secs(300),
            )?;
            // `concurrent-stop_served` only proves both subtrees recorded their
            // pids; the runtime answers the background `exec` call with a task
            // receipt and then re-prompts the model, and that continuation is a
            // *required* strict-script step. If the coordinator took the writer
            // lock and released the close now, the turn could be cancelled at
            // the receipt before that step is matched, leaving the script short
            // behind an accepted-request count. Require the fixture's own live
            // counters to report every remaining step optional (the
            // continuation has really been accepted) while its paced reply is
            // still streaming, and only then block the terminal save.
            let continuation = wait_concurrent_continuation(
                status_file,
                &mut gui,
                &mut fixture,
                interrupt,
                Duration::from_secs(120),
            )?;
            let database = history_lock_database(&home_concurrent)?;
            let before = history_lock_watermark(&database)?;
            let mut lock = SqliteWriteLock::acquire(
                &database,
                "shutdown concurrent history.sqlite",
                Duration::from_secs(120),
            )?;
            let (native_pid, start_ticks, children) = snapshot_native(output, "concurrent-stop")?;
            let mcp = read_phase_json(&concurrent_mcp_coord)?;
            let mcp_pid = mcp["mcpPid"].as_u64().unwrap_or_default() as u32;
            let mcp_ticks = mcp["mcpStartTicks"].as_u64();
            let mcp_gc_pid = mcp["grandchildPid"].as_u64().unwrap_or_default() as u32;
            let mcp_gc_ticks = mcp["grandchildStartTicks"].as_u64();
            let tool = read_phase_json(&concurrent_tool_coord)?;
            let tool_pid = tool["toolPid"].as_u64().unwrap_or_default() as u32;
            let tool_ticks = tool["toolStartTicks"].as_u64();
            let tool_gc_pid = tool["grandchildPid"].as_u64().unwrap_or_default() as u32;
            let tool_gc_ticks = tool["grandchildStartTicks"].as_u64();
            ensure!(
                mcp_pid > 0 && mcp_gc_pid > 0 && mcp_gc_pid != mcp_pid,
                "the concurrent MCP server recorded no distinct business pids: {mcp}"
            );
            ensure!(
                tool_pid > 0 && tool_gc_pid > 0 && tool_gc_pid != tool_pid,
                "the concurrent background tool recorded no distinct business pids: {tool}"
            );
            // Hard pre-request gate: every business pid must be genuinely live
            // BEFORE the exit is requested, so a later disappearance proves the
            // product's own supervision instead of a race with a fast exit.
            #[cfg(target_os = "linux")]
            {
                ensure!(
                    proc_alive(mcp_pid, mcp_ticks)
                        && proc_alive(mcp_gc_pid, mcp_gc_ticks)
                        && proc_alive(tool_pid, tool_ticks)
                        && proc_alive(tool_gc_pid, tool_gc_ticks),
                    "a concurrent business pid was not alive BEFORE the exit was requested: \
                     mcp={mcp_pid} mcpGrandchild={mcp_gc_pid} tool={tool_pid} toolGrandchild={tool_gc_pid}"
                );
            }
            let monitor = monitor_concurrent_stop(
                (native_pid, start_ticks),
                vec![
                    (mcp_pid, mcp_ticks),
                    (mcp_gc_pid, mcp_gc_ticks),
                    (tool_pid, tool_ticks),
                    (tool_gc_pid, tool_gc_ticks),
                ],
                EXIT_GRACE,
            );
            fs::write(
                working.join("shutdown-signal-concurrent-stop-armed"),
                "armed",
            )?;
            wait_stage(
                "concurrent-stop_exit_requested",
                working,
                &mut driver,
                &mut gui,
                &mut fixture,
                interrupt,
                Duration::from_secs(180),
            )?;
            let arm = Instant::now();
            let native = wait_native_exit(
                "concurrent-stop",
                working,
                output,
                native_pid,
                start_ticks,
                &mut gui,
                interrupt,
                EXIT_GRACE,
                arm,
            )?;
            let reclamation = monitor
                .join()
                .unwrap_or_else(|_| serde_json::json!({"monitor": "panicked"}));
            // Release the lock only after the process is gone: while it is held
            // the terminal save cannot commit, which is the blocked chain the
            // acceptance observes.
            lock.finish()?;
            let driver_exit = wait_driver(
                "concurrent-stop",
                working,
                &mut driver,
                interrupt,
                Duration::from_secs(120),
            )?;
            driver_evidence.push(DriverEvidence {
                log: driver_log,
                stem: "shutdown-concurrent".into(),
                exit: Some(driver_exit),
            });
            // Safety net: a surviving business pid is a real product leak already
            // recorded above; kill it here only so the run leaves no orphan.
            let mut forced_kill = Vec::new();
            for (pid, ticks) in [
                (mcp_gc_pid, mcp_gc_ticks),
                (mcp_pid, mcp_ticks),
                (tool_gc_pid, tool_gc_ticks),
                (tool_pid, tool_ticks),
            ] {
                if proc_alive(pid, ticks) {
                    forced_kill.push(pid);
                    let _ = Command::new("kill")
                        .arg("-KILL")
                        .arg("--")
                        .arg(pid.to_string())
                        .status();
                }
            }
            // Per-PID stop times, taken from the independent monitor whose clock
            // started just before the exit request. Every business pid keeps its
            // own raw timing so a grandchild that stops early can never stand in
            // for a parent that only disappears with the native force-out.
            let stop_millis = |pid: u32| -> Option<u128> {
                reclamation["observed"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .find(|row| row["pid"].as_u64() == Some(u64::from(pid)))
                    .and_then(|row| row["exitedAtMillis"].as_u64())
                    .map(u128::from)
            };
            let mcp_process_stop = stop_millis(mcp_pid);
            let mcp_grandchild_stop = stop_millis(mcp_gc_pid);
            let tool_process_stop = stop_millis(tool_pid);
            let tool_grandchild_stop = stop_millis(tool_gc_pid);
            // A resource is only confirmed reclaimed when *both* its process and
            // its SIGTERM-ignoring grandchild were observed to exit, and its
            // completion is the later of the two: a missing observation for
            // either pid leaves the resource unconfirmed, and a resource whose
            // slower pid is still alive until the native force-out can never
            // read as a completed concurrent stop.
            let resource_stop = |process: Option<u128>, grandchild: Option<u128>| -> Option<u128> {
                match (process, grandchild) {
                    (Some(process), Some(grandchild)) => Some(process.max(grandchild)),
                    _ => None,
                }
            };
            let mcp_stop = resource_stop(mcp_process_stop, mcp_grandchild_stop);
            let tool_stop = resource_stop(tool_process_stop, tool_grandchild_stop);
            // Same-clock moment the native host itself disappeared.
            let native_exited_at = reclamation["nativeExitedAtMillis"].as_u64().map(u128::from);
            // Every business pid of both resources must precede the real native
            // exit on the shared monitor clock: a resource whose slower pid only
            // died with (or after) the native force-out is a serialized stop, not
            // concurrent.
            let mcp_before_native_exit =
                matches!((mcp_stop, native_exited_at), (Some(stop), Some(at)) if stop < at);
            let tool_before_native_exit =
                matches!((tool_stop, native_exited_at), (Some(stop), Some(at)) if stop < at);
            let database_after = history_lock_database(&home_concurrent)?;
            let after = history_lock_watermark(&database_after)?;
            let quick_check = history_lock_quick_check(&database_after)?;
            // The native host's own canonical diagnostics are the only terminal
            // shutdown facts independent of the Dart isolate: they distinguish a
            // coordinated ~28-second degraded `finishExit` from the 30-second
            // watchdog and record whether the blocked save was left uncommitted.
            let native_diagnostics = read_native_exit_diagnostics(&home_concurrent)?;
            let diagnostics_code = native_diagnostics["code"].as_str().unwrap_or_default();
            let diagnostics_pending = native_diagnostics["pending"].as_str().unwrap_or_default();
            fs::write(
                output.join("shutdown-concurrent-stop.json"),
                serde_json::to_vec_pretty(&serde_json::json!({
                    "nativePid": native.pid,
                    "nativeExitCode": native.code,
                    "nativeExitSignal": native.signal,
                    "nativeElapsedMillis": native.elapsed.as_millis(),
                    "budgetMillis": EXIT_BUDGET.as_millis(),
                    "cleanupBudgetMillis": CONCURRENT_CLEANUP_BUDGET.as_millis(),
                    "stopAckBoundMillis": CONCURRENT_STOP_ACK_BOUND.as_millis(),
                    "nativeExitedAtMillis": native_exited_at,
                    "writerLockHeldAcrossExit": true,
                    "continuationAccepted": true,
                    "continuationLiveStatus": continuation,
                    "nativeExitDiagnostics": native_diagnostics,
                    "mcp": {
                        "pid": mcp_pid,
                        "grandchildPid": mcp_gc_pid,
                        "processStopMillis": mcp_process_stop,
                        "grandchildStopMillis": mcp_grandchild_stop,
                        "stopMillis": mcp_stop,
                    },
                    "tool": {
                        "pid": tool_pid,
                        "grandchildPid": tool_gc_pid,
                        "processStopMillis": tool_process_stop,
                        "grandchildStopMillis": tool_grandchild_stop,
                        "stopMillis": tool_stop,
                    },
                    "monitor": reclamation,
                    "descendantsAtRootExit": native.descendants_at_root_exit,
                    "reclaimedDescendants": native.reclaimed_descendants,
                    "forcedKill": forced_kill,
                    "before": before,
                    "after": after,
                    "quickCheck": quick_check,
                }))?,
            )?;
            phases.push(PhaseRecord {
                name: "concurrent-stop",
                status: "executed",
                coverage: "executed",
                detail: format!(
                    "the concurrent turn's required receipt continuation was accepted (consumed {}/{} steps, remainingOptional={}) before the exclusive writer lock was taken across the exit (blocked terminal save); the scenario-owned stdio MCP server and the background tool subtree were each proven live and then reclaimed on their own (mcp stop {mcp_stop:?} ms, tool stop {tool_stop:?} ms) while that chain was still waiting; the native exit code 1 was a non-clean degraded shutdown (event={} code={} pending={}, coordinatedFinish={}, deadlineImminent={}): {}",
                    continuation.consumed_steps,
                    continuation.expected_steps,
                    continuation.remaining_optional,
                    native_diagnostics["event"].as_str().unwrap_or_default(),
                    diagnostics_code,
                    diagnostics_pending,
                    native_diagnostics["coordinatedFinish"],
                    native_diagnostics["deadlineImminent"],
                    native.summary()
                ),
                exit_code: native.code,
                elapsed_millis: Some(native.elapsed.as_millis()),
                native_pid: Some(native.pid),
                owned_children: children,
            });
            // The two independent resources really stopped: every business pid
            // was observed at zero and the harness had nothing to force-kill.
            ensure!(
                reclamation["stillAlive"]
                    .as_array()
                    .is_some_and(Vec::is_empty)
                    && forced_kill.is_empty(),
                "an independent resource was still alive at the observation deadline: {reclamation}"
            );
            ensure!(
                native.reclaimed_descendants.is_empty(),
                "the product leaked a concurrent subtree and the harness had to reclaim it: {:?}",
                native.reclaimed_descendants
            );
            // Each independent resource is a real stop ACK only when *both* its
            // process and its grandchild were observed to exit within the bound,
            // well under the single budget: neither may be serialized behind the
            // ~28-second save, and an unobserved pid is never treated as a pass.
            let stop_ack_bound_ms = CONCURRENT_STOP_ACK_BOUND.as_millis();
            ensure!(
                mcp_stop.is_some_and(|ms| ms <= stop_ack_bound_ms),
                "the MCP resource was not confirmed reclaimed within {stop_ack_bound_ms} ms: \
                 mcpProcess={mcp_process_stop:?} mcpGrandchild={mcp_grandchild_stop:?} \
                 (observed {reclamation})"
            );
            ensure!(
                tool_stop.is_some_and(|ms| ms <= stop_ack_bound_ms),
                "the tool resource was not confirmed reclaimed within {stop_ack_bound_ms} ms: \
                 toolProcess={tool_process_stop:?} toolGrandchild={tool_grandchild_stop:?} \
                 (observed {reclamation})"
            );
            ensure!(
                mcp_before_native_exit && tool_before_native_exit,
                "a resource did not stop before the real native exit, so the blocked chain was not \
                 still waiting: mcp={mcp_stop:?} (process {mcp_process_stop:?}, grandchild \
                 {mcp_grandchild_stop:?}) tool={tool_stop:?} (process {tool_process_stop:?}, \
                 grandchild {tool_grandchild_stop:?}) nativeExitedAt={native_exited_at:?} \
                 (observed {reclamation})"
            );
            // The still-blocked save forced a degraded exit code 1: a Clean
            // (code 0) exit would mean the lock never held the terminal save.
            ensure!(
                native.code == Some(1) && native.signal.is_none(),
                "concurrent-stop native exit was not the degraded code 1: {}",
                native.summary()
            );
            // The single hard deadline is an upper bound, not an exact target: a
            // coordinated degraded finish lands at the 28-second cleanup budget
            // (the 30-second watchdog is reserved for a Dart/bridge that never
            // returns), so demanding a ~30-second exit would fail a correct one.
            ensure!(
                native.elapsed <= EXIT_BUDGET + Duration::from_secs(1),
                "concurrent-stop ran past the single 30-second hard deadline: {:?}",
                native.elapsed
            );
            // A still-held writer lock cannot commit the save until this process
            // is gone, so the cleanup had to wait out its budget before the
            // degraded finish; an exit well before the budget would mean the save
            // was never actually blocked.
            ensure!(
                native.elapsed + Duration::from_secs(1) >= CONCURRENT_CLEANUP_BUDGET,
                "concurrent-stop exited before the 28-second cleanup budget, so the writer lock \
                 did not block the save to the budget: {:?}",
                native.elapsed
            );
            ensure!(
                quick_check == "ok",
                "reopened concurrent history database failed PRAGMA quick_check: {quick_check}"
            );
            // The blocked save must never be recorded as a clean, fully drained
            // Stopped: a `cleanExit` code or a `pending=0` terminal would mean the
            // lock never held the save, so the phase fails rather than accepting a
            // fabricated clean exit.
            ensure!(
                diagnostics_code != "cleanExit"
                    && (diagnostics_pending == "unknown"
                        || matches!(
                            diagnostics_pending.parse::<u64>(),
                            Ok(pending) if pending > 0
                        )),
                "the concurrent shutdown was recorded as clean or fully drained, so the writer \
                 lock did not block the save: {native_diagnostics}"
            );
            write_sanitized_log(&log, &output.join("shutdown-concurrent-gui.log"))?;
        }

        Ok(())
    })();

    if fixture_paused {
        let _ = resume_fixture(&fixture);
    }

    let redactions = [
        (app_dir.to_string_lossy().into_owned(), "<app>"),
        (workspace.to_string_lossy().into_owned(), "<workspace>"),
        (home.to_string_lossy().into_owned(), "<home>"),
        (working.to_string_lossy().into_owned(), "<coord>"),
    ];
    for evidence in &driver_evidence {
        write_realtime_driver_evidence(
            &evidence.log,
            output,
            &evidence.stem,
            evidence.exit,
            journey.as_ref().err(),
            &redactions,
        )?;
    }

    // Canonical diagnostics live under `ANYWORK_HOME/studio/logs`; copy them so
    // the PID/stage/elapsed/error-code/correlation/persistence facts survive the
    // run's temporary coordination directory, and never rely on stdout alone.
    let mut diagnostics = serde_json::Map::new();
    let diagnostic_homes: [(&str, &Path); 14] = [
        ("normal", *home),
        ("duplicate", home_duplicate.as_path()),
        ("hang", home_hang.as_path()),
        ("storage", home_storage.as_path()),
        ("service", home_service.as_path()),
        ("runtime-unavailable", home_unavailable.as_path()),
        ("init-close", home_init_close.as_path()),
        ("bridge", home_bridge.as_path()),
        ("subscription-fault", home_subscription.as_path()),
        ("tmp-fallback", home_tmp_fallback.as_path()),
        ("mcp-reclamation", home_mcp.as_path()),
        ("lsp-reclamation", home_lsp.as_path()),
        ("tool-reclamation", home_tool.as_path()),
        ("concurrent-stop", home_concurrent.as_path()),
    ];
    for (label, dir) in diagnostic_homes {
        let copied = copy_home_diagnostics(dir, output, label)?;
        diagnostics.insert(
            label.to_owned(),
            serde_json::json!({
                "homeStudioLogs": dir.join("studio/logs").display().to_string(),
                "copiedFiles": copied,
            }),
        );
    }
    fs::write(
        output.join("diagnostics.json"),
        serde_json::to_vec_pretty(&serde_json::Value::Object(diagnostics))?,
    )?;

    // Every requirement the Linux acceptance did not actually execute is listed
    // here with the real reason; nothing skipped is written as passed. The
    // round-2 gaps (MCP/LSP/tool subtree reclamation, init/bridge/subscription
    // faults, temp fallback) are now implemented as phases above; only the
    // approved Windows-only not-run remains.
    let gaps = vec![GapRecord {
        requirement: "windows-native-acceptance",
        reason: "the user specified Linux as the acceptance platform; the Windows Job Object reclamation path was not executed (approved as not-run, not a Linux gap)",
        interface_needs: "a Windows host to run the same --scenario shutdown phases",
    }];

    // The second instance must have exited through the real delete event; read
    // the persisted evidence rather than re-deriving it from logs.
    let busy_evidence = fs::read(output.join("shutdown-busy-second.json"))
        .ok()
        .and_then(|bytes| serde_json::from_slice::<serde_json::Value>(&bytes).ok());
    let window_delete_executed = busy_evidence.as_ref().is_some_and(|evidence| {
        evidence["deleteEventMatchesNative"] == true
            && evidence["deleteEventNativeExited"] == true
            && evidence["secondNativeExitCode"].as_i64() == Some(0)
    });

    // Machine checks are `passed` only for the requirements this Linux run
    // actually executed; skipped requirements are `notCovered`, never assumed.
    let machine_checks = serde_json::json!({
        "normalExit": if phases.iter().any(|phase| phase.name == "normal") { "executed" } else { "notCovered" },
        "reopenRestore": if phases.iter().any(|phase| phase.name == "reopen") { "executed" } else { "notCovered" },
        "secondInstanceBusy": if phases.iter().any(|phase| phase.name == "busy") { "executed" } else { "notCovered" },
        "busyReopenRestore": if phases.iter().any(|phase| phase.name == "busy-reopen") { "executed" } else { "notCovered" },
        "duplicateDeadline": if phases.iter().any(|phase| phase.name == "duplicate") { "executed" } else { "notCovered" },
        "dartHangForced30s": if phases.iter().any(|phase| phase.name == "hang") { "executed" } else { "notCovered" },
        "runtimeUnavailableClose": if phases.iter().any(|phase| phase.name == "runtime-unavailable") { "executed" } else { "notCovered" },
        "storageLockReopen": if phases.iter().any(|phase| phase.name == "storage-lock") { "executed" } else { "notCovered" },
        "serviceUnresponsive": if phases.iter().any(|phase| phase.name == "service-unresponsive") { "executed" } else { "notCovered" },
        "nativeSubtreeReclamation": if phases.iter().any(|phase| phase.name == "native-subtree-reclamation") { "executed" } else { "notCovered" },
        "closeDuringInit": if phases.iter().any(|phase| phase.name == "close-during-init") { "executed" } else { "notCovered" },
        "bridgeUnavailable": if phases.iter().any(|phase| phase.name == "bridge-unavailable") { "executed" } else { "notCovered" },
        "subscriptionCancelFault": if phases.iter().any(|phase| phase.name == "subscription-fault") { "executed" } else { "notCovered" },
        "dartErrorTempFallback": if phases.iter().any(|phase| phase.name == "dart-error-fallback") { "executed" } else { "notCovered" },
        "lspSubtreeReclamation": if phases.iter().any(|phase| phase.name == "lsp-subtree-reclamation") { "executed" } else { "notCovered" },
        "toolSubtreeReclamation": if phases.iter().any(|phase| phase.name == "tool-subtree-reclamation") { "executed" } else { "notCovered" },
        "concurrentResourceStop": if phases.iter().any(|phase| phase.name == "concurrent-stop") { "executed" } else { "notCovered" },
        "secondInstanceWindowDeleteEvent": if window_delete_executed { "executed" } else { "notCovered" },
        "windowsNativeAcceptance": "notrun",
        "humanVerdict": "pending",
    });

    // `json!` requires every value to be `Serialize`, and `anyhow::Error` is
    // not, so the fallible live read is flattened to `Option<usize>` here.
    let live_status = read_live_status(status_file).ok();
    let accepted = live_status.as_ref().map(|status| status.accepted);
    let rejected = live_status.as_ref().map(|status| status.rejected);
    fs::write(
        output.join("coverage.json"),
        serde_json::to_vec_pretty(&serde_json::json!({
            "scenario": "shutdown",
            "platform": std::env::consts::OS,
            "exitBudgetMillis": EXIT_BUDGET.as_millis(),
            "humanVerdict": "pending",
            "machineChecks": machine_checks,
            "phases": phases,
            "gaps": gaps,
            "liveStatus": live_status,
        }))?,
    )?;
    fs::write(
        output.join("fixture-status.json"),
        serde_json::to_vec_pretty(&serde_json::json!({
            "scenario": "shutdown",
            "acceptedRequests": accepted,
            "rejectedRequests": rejected,
            "liveStatus": live_status,
        }))?,
    )?;

    // Only the happy-path phases are gated on a clean GUI log; the fault phases
    // deliberately force exit code 1, a failed runtime install or a frozen
    // provider, so their error lines are recorded as evidence, not treated as an
    // acceptance failure.
    let log_sources: [(&str, bool); 19] = [
        ("shutdown-normal-gui.log", true),
        ("shutdown-reopen-gui.log", true),
        ("shutdown-busy-first-gui.log", true),
        ("shutdown-busy-second-gui.log", false),
        ("shutdown-busy-reopen-gui.log", true),
        // The duplicate phase deliberately runs the subscription-fault
        // entrypoint (same Driver-only target as `subscription-fault`) so the
        // repeated close is genuinely delivered; its log carries that injected
        // fault on purpose and is therefore not a happy-path error source.
        ("shutdown-duplicate-gui.log", false),
        ("shutdown-hang-gui.log", false),
        ("shutdown-unavailable-gui.log", false),
        ("shutdown-storage-prime-gui.log", false),
        ("shutdown-storage-verify-gui.log", true),
        ("shutdown-service-gui.log", false),
        ("shutdown-init-close-gui.log", false),
        ("shutdown-bridge-gui.log", false),
        ("shutdown-subscription-gui.log", false),
        ("shutdown-tmp-fallback-gui.log", false),
        ("shutdown-mcp-gui.log", false),
        ("shutdown-lsp-gui.log", false),
        ("shutdown-tool-gui.log", false),
        // The concurrent-stop phase deliberately holds the exclusive writer lock
        // across the exit, so its forced exit 1 log is an expected fault source.
        ("shutdown-concurrent-gui.log", false),
    ];
    let mut strict_errors = 0usize;
    let mut gui_metrics = serde_json::Map::new();
    for (name, strict) in log_sources {
        let source = working.join(name);
        if !source.is_file() {
            continue;
        }
        let count = count_gui_errors(&source)?;
        if strict {
            strict_errors += count;
        }
        gui_metrics.insert(
            name.to_owned(),
            serde_json::json!({ "unhandledErrors": count, "strict": strict }),
        );
    }
    fs::write(
        output.join("gui-health.json"),
        serde_json::to_vec_pretty(&serde_json::json!({
            "unhandledErrors": strict_errors,
            "phases": gui_metrics,
            "humanVerdict": "pending",
        }))?,
    )?;

    let fixture_result = fixture.stop(requests_file);
    drop(fixture);
    // Re-read the strict counters after the fixture stopped. The fixture's own
    // exit status already encodes `FixtureReport::verify` (a non-zero exit when
    // any required step was left unconsumed), and this snapshot makes the
    // required-step gate explicit instead of trusting an accepted-request count.
    let final_live_status = read_live_status(status_file).ok();
    write_fixture_log(fixture_log, &output.join("fixture.log"))?;
    let requests = if requests_file.is_file() {
        sanitize_requests(requests_file, &output.join("requests.json"))?;
        serde_json::from_slice::<Vec<serde_json::Value>>(&fs::read(requests_file)?)?
    } else {
        Vec::new()
    };
    let rejected_total = requests
        .iter()
        .filter(|row| row["accepted"] == false)
        .count();
    let accepted_total = requests
        .iter()
        .filter(|row| row["accepted"] == true)
        .count();
    println!("Evidence: {} (human verdict pending)", output.display());

    journey?;

    // Every phase that ran to completion must have written a pending summary
    // with all of its own checks passing. `busy-second` is excluded because the
    // second instance is allowed to fail its startup; the coordinator judges it
    // from the Driver snapshot OR the GUI log in `capture_second_instance`.
    for summary_phase in [
        "normal",
        "reopen",
        "busy-hold",
        "busy-reopen",
        "duplicate",
        "hang",
        "runtime-unavailable",
        "storage-lock",
        "storage-verify",
        "service-unresponsive",
        "close-during-init",
        "bridge-unavailable",
        "subscription-fault",
        "dart-error-fallback",
        "mcp-reclamation",
        "lsp-reclamation",
        "tool-reclamation",
        "concurrent-stop",
    ] {
        let path = output.join(format!("shutdown-{summary_phase}-summary.json"));
        let report: serde_json::Value =
            serde_json::from_slice(&fs::read(&path).with_context(|| {
                format!("Flutter Driver did not write the shutdown {summary_phase} summary")
            })?)?;
        ensure!(
            report["scenario"] == "shutdown" && report["verdict"] == "pending",
            "shutdown {summary_phase} summary is missing its scenario or pending verdict"
        );
        ensure!(
            report["status"] == "complete",
            "shutdown {summary_phase} journey did not complete: stage={} failedChecks={}",
            report["stage"],
            report["failedChecks"],
        );
    }

    // Every *required* strict-script step must have been consumed, not merely a
    // minimum number of requests accepted: `remaining_optional` is true only
    // when nothing at the cursor is still required, so a short script (the
    // concurrent receipt continuation left unmatched, say) fails here.
    let required_steps_consumed = final_live_status
        .as_ref()
        .is_some_and(|status| status.remaining_optional && status.rejected == 0);
    ensure!(
        fixture_result.is_ok_and(|status| status.success())
            && required_steps_consumed
            && rejected_total == 0
            && strict_errors == 0,
        "shutdown fixture or GUI health incomplete; human verdict pending \
         (accepted={accepted_total}, rejected={rejected_total}, strictErrors={strict_errors}, \
         requiredStepsConsumed={required_steps_consumed}, finalLiveStatus={final_live_status:?})"
    );
    Ok(())
}

/// Path of the structured native exit report the launcher writes for [label].
fn native_report_path(working: &Path, label: &str) -> PathBuf {
    working.join(format!("shutdown-native-exit-{label}.json"))
}

/// Starts a native Driver GUI for [anywork_home].
///
/// The launcher builds the Driver bundle and launches the native artifact
/// directly (`--native-launch`), so the process it starts is the real native
/// host rather than a `cargo`/Flutter tool wrapper whose exit status is always
/// `0`. The launcher records the artifact's genuine wait status (and any child
/// tree it left behind) in the report at [`native_report_path`].
fn start_gui(
    workspace: &Path,
    anywork_home: &Path,
    log_path: &Path,
    working: &Path,
    label: &str,
) -> Result<OwnedProcess> {
    start_gui_with(
        workspace,
        anywork_home,
        log_path,
        working,
        label,
        &[],
        None,
        None,
    )
}

/// Starts a native Driver GUI with extra environment and an optional Driver-only
/// fault entrypoint / isolated `TMPDIR`.
///
/// [driver_target] selects a Driver entrypoint other than the frozen
/// `test_driver/driver_main.dart` (resolved relative to the app dir by xtask);
/// [tmpdir] isolates `Directory.systemTemp` so a temp-fallback diagnostic never
/// touches a shared machine temp directory.
#[allow(clippy::too_many_arguments)]
fn start_gui_with(
    workspace: &Path,
    anywork_home: &Path,
    log_path: &Path,
    working: &Path,
    label: &str,
    env: &[(&str, &str)],
    driver_target: Option<&str>,
    tmpdir: Option<&Path>,
) -> Result<OwnedProcess> {
    let log = File::create(log_path)?;
    let report = native_report_path(working, label);
    let mut command = Command::new("cargo");
    command
        .current_dir(workspace)
        .args([
            "xtask",
            "run-gui",
            "--driver",
            "--native-launch",
            "--native-exit-report",
        ])
        .arg(&report)
        .env("ANYWORK_HOME", anywork_home)
        .stdout(Stdio::from(log.try_clone()?))
        .stderr(Stdio::from(log));
    for (key, value) in env {
        command.env(key, value);
    }
    // A caller that pins `DISPLAY` wants the instance on a scenario-owned X
    // display. Force the X11 GDK backend and drop any `WAYLAND_DISPLAY`
    // inherited from the ambient session, or GTK could legitimately pick Wayland
    // and then own no window on the very display the delete event targets.
    if env.iter().any(|(key, _)| *key == "DISPLAY") {
        command.env("GDK_BACKEND", "x11");
        command.env_remove("WAYLAND_DISPLAY");
    }
    if let Some(target) = driver_target {
        command.arg("--native-launch-driver-target").arg(target);
    }
    if let Some(tmpdir) = tmpdir {
        command.env("TMPDIR", tmpdir);
    }
    OwnedProcess::start(&mut command, false)
}

/// Starts one shutdown journey phase against an already-running GUI.
#[allow(clippy::too_many_arguments)]
fn start_driver(
    app_dir: &Path,
    anywork_home: &Path,
    phase: &str,
    vm_url: &str,
    project: &Path,
    output: &Path,
    working: &Path,
    driver_log: &Path,
) -> Result<OwnedProcess> {
    let log = File::create(driver_log)?;
    let mut command = process::path_command("dart", &[]);
    command
        .current_dir(app_dir)
        .args(["run", "test_driver/shutdown_journey.dart", phase, vm_url])
        .arg(project)
        .arg(output)
        .arg(working)
        .env("ANYWORK_HOME", anywork_home)
        .stdout(Stdio::from(log.try_clone()?))
        .stderr(Stdio::from(log));
    OwnedProcess::start(&mut command, false)
}

/// Creates an isolated git project the Driver can open as the session project.
fn prepare_project(working: &Path, name: &str) -> Result<PathBuf> {
    let project = working.join(name);
    fs::create_dir_all(&project)?;
    ensure!(
        Command::new("git")
            .args(["init", "-q"])
            .arg(&project)
            .status()?
            .success(),
        "failed to initialize the isolated shutdown project"
    );
    Ok(project)
}

/// Waits, bounded, for the Driver to publish the append-only stage marker.
fn wait_stage(
    expected: &str,
    working: &Path,
    driver: &mut OwnedProcess,
    gui: &mut OwnedProcess,
    fixture: &mut OwnedProcess,
    interrupt: &mpsc::Receiver<()>,
    timeout: Duration,
) -> Result<()> {
    let marker = working.join(format!("shutdown-stage-{expected}"));
    let deadline = Instant::now() + timeout;
    loop {
        if marker.is_file() {
            return Ok(());
        }
        if let Some(status) = driver.child.try_wait()? {
            bail!("shutdown Driver exited before {expected}: {status}");
        }
        ensure!(!gui.exited()?, "GUI exited before {expected}");
        ensure!(
            !fixture.exited()?,
            "provider fixture exited before {expected}"
        );
        ensure!(interrupt.try_recv().is_err(), "shutdown journey cancelled");
        ensure!(
            Instant::now() < deadline,
            "shutdown Driver timed out before {expected} (last stage: {})",
            fs::read_to_string(working.join("shutdown-stage")).unwrap_or_else(|_| "connect".into())
        );
        thread::sleep(Duration::from_millis(100));
    }
}

/// Waits until the strict fixture has consumed the concurrent turn's required
/// receipt continuation.
///
/// The Driver marks `concurrent-stop_served` as soon as both independent
/// subtrees record their pids, but the runtime answers the background `exec`
/// call with a task receipt and then re-prompts the model: that continuation is
/// a *required* script step. The fixture's live counters report
/// `remaining_optional` only when every step left at its cursor is optional, so
/// this waits for that flag -- proving the continuation was really accepted --
/// while the paced reply is still streaming. A rejected request, an exited GUI
/// or fixture, a cancel, or the timeout are all hard failures so a short script
/// can never be read as a pass.
fn wait_concurrent_continuation(
    status_file: &Path,
    gui: &mut OwnedProcess,
    fixture: &mut OwnedProcess,
    interrupt: &mpsc::Receiver<()>,
    timeout: Duration,
) -> Result<pl_provider_fixture::FixtureLiveStatus> {
    let deadline = Instant::now() + timeout;
    loop {
        if let Some(status) = read_live_status(status_file)
            .ok()
            .filter(|status| status.rejected == 0 && status.remaining_optional)
        {
            return Ok(status);
        }
        ensure!(
            !gui.exited()?,
            "GUI exited before the concurrent receipt continuation was accepted"
        );
        ensure!(
            !fixture.exited()?,
            "provider fixture exited before the concurrent receipt continuation was accepted"
        );
        ensure!(interrupt.try_recv().is_err(), "shutdown journey cancelled");
        ensure!(
            Instant::now() < deadline,
            "the concurrent receipt continuation was not accepted within {timeout:?} \
             (fixture live status: {})",
            read_live_status(status_file)
                .map(|status| format!("{status:?}"))
                .unwrap_or_else(|error| format!("unavailable: {error}"))
        );
        thread::sleep(Duration::from_millis(50));
    }
}

/// Waits for the Driver process to finish, returning its real exit status.
///
/// The GUI is deliberately not checked here: in every shutdown phase the
/// journey requests the app exit, so the owned GUI may already have exited
/// before the Dart helper process finished. The OS exit of the GUI is judged
/// separately by [`wait_gui_exit`].
fn wait_driver(
    phase: &str,
    working: &Path,
    driver: &mut OwnedProcess,
    interrupt: &mpsc::Receiver<()>,
    timeout: Duration,
) -> Result<ExitStatus> {
    let deadline = Instant::now() + timeout;
    loop {
        if let Some(status) = driver.child.try_wait()? {
            driver.stopped = true;
            return Ok(status);
        }
        ensure!(
            interrupt.try_recv().is_err(),
            "shutdown {phase} journey cancelled"
        );
        ensure!(
            Instant::now() < deadline,
            "shutdown {phase} journey timed out (last stage: {})",
            fs::read_to_string(working.join("shutdown-stage")).unwrap_or_else(|_| "connect".into())
        );
        thread::sleep(Duration::from_millis(100));
    }
}

/// A native host's real termination.
///
/// The exit code comes from the launcher's structured report (its direct
/// `waitpid` of the artifact), never from the `cargo`/Flutter wrapper; the
/// disappeared process is independently confirmed by this coordinator's own
/// `/proc` poll with a start-time reuse guard. `elapsed` is measured from the
/// real native arm, not from an earlier save window nor a later Driver wait.
struct NativeExit {
    pid: u32,
    code: Option<i32>,
    signal: Option<i32>,
    elapsed: Duration,
    /// Subtree present when the native host was reaped, before the product's own
    /// supervisor had its natural reclamation window.
    descendants_at_root_exit: Vec<u32>,
    /// Subtree the harness had to force-kill after that window (empty means the
    /// product supervised its own subtree).
    reclaimed_descendants: Vec<u32>,
}

impl NativeExit {
    fn summary(&self) -> String {
        let how = match (self.code, self.signal) {
            (Some(code), _) => format!("exit code {code}"),
            (None, Some(signal)) => format!("signal {signal}"),
            _ => "unknown termination".to_owned(),
        };
        format!(
            "pid {} {how} after {:?} (reclaimed descendants: {})",
            self.pid,
            self.elapsed,
            self.reclaimed_descendants.len()
        )
    }
}

/// Reads the Driver-reported native process identity written by the journey.
///
/// The journey writes `shutdown-native-<label>.json` early (the `pid` request),
/// so the coordinator can OS-wait the real native process with a start-time
/// reuse guard instead of the `cargo` wrapper pid.
fn read_native_identity(output: &Path, label: &str) -> Result<serde_json::Value> {
    let path = output.join(format!("shutdown-native-{label}.json"));
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        if let Ok(bytes) = fs::read(&path) {
            return serde_json::from_slice(&bytes).with_context(|| {
                format!("native identity file is not valid JSON: {}", path.display())
            });
        }
        ensure!(
            Instant::now() < deadline,
            "shutdown {label} did not publish its native identity at {}",
            path.display()
        );
        thread::sleep(Duration::from_millis(50));
    }
}

/// Waits for the phase's `<label>_exit_requested` marker, then snapshots the
/// real native identity and descendant PIDs before that native host exits.
///
/// The returned start time is the PID-reuse guard for the coordinator's own
/// `/proc` poll, and the descendant PIDs are the real business processes that
/// must be observed to exit.
#[allow(clippy::too_many_arguments)]
fn arm_native(
    label: &str,
    marker: &str,
    output: &Path,
    interrupt: &mpsc::Receiver<()>,
    working: &Path,
    driver: &mut OwnedProcess,
    gui: &mut OwnedProcess,
    fixture: &mut OwnedProcess,
) -> Result<(u32, Option<u64>, Vec<u32>)> {
    wait_stage(
        marker,
        working,
        driver,
        gui,
        fixture,
        interrupt,
        Duration::from_secs(180),
    )?;
    snapshot_native(output, label)
}

/// Resolves the Driver-reported native host and its current descendant PIDs.
///
/// This never waits for an exit request, so the subtree phases can resolve the
/// real native process (and prove the business subtree is alive) *before* any
/// close is issued; only then does a later disappearance become evidence of the
/// product's own supervision instead of a race with a fast normal exit.
fn snapshot_native(output: &Path, label: &str) -> Result<(u32, Option<u64>, Vec<u32>)> {
    let identity = read_native_identity(output, label)?;
    let pid = identity["pid"]
        .as_u64()
        .filter(|pid| *pid > 0)
        .map(|pid| pid as u32)
        .with_context(|| format!("shutdown {label} Driver reported no native pid: {identity}"))?;
    Ok((pid, proc_start_ticks(pid), owned_children(pid)))
}

/// The Linux process start time (jiffies since boot) for [pid], if alive.
///
/// Field 22 of `/proc/<pid>/stat`; the executable name may contain spaces and
/// parentheses, so parsing resumes after the last `)`.
#[cfg(target_os = "linux")]
fn proc_start_ticks(pid: u32) -> Option<u64> {
    let stat = fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
    let after_name = &stat[stat.rfind(')')? + 1..];
    let fields: Vec<&str> = after_name.split_whitespace().collect();
    // After the `comm` field the collected slice starts at field 3 (state), so
    // starttime (field 22) is at index 19.
    fields.get(19).and_then(|value| value.parse::<u64>().ok())
}

#[cfg(not(target_os = "linux"))]
fn proc_start_ticks(_pid: u32) -> Option<u64> {
    None
}

/// Whether [pid] is still the same live process that owned [start_ticks].
#[cfg(target_os = "linux")]
fn proc_alive(pid: u32, start_ticks: Option<u64>) -> bool {
    if !Path::new(&format!("/proc/{pid}")).exists() {
        return false;
    }
    match (start_ticks, proc_start_ticks(pid)) {
        (Some(known), Some(current)) => current == known,
        // No start-time baseline available: presence is the only signal.
        (None, _) => true,
        // The pid exists again with a different start time: the original exited.
        (Some(_), None) => false,
    }
}

#[cfg(not(target_os = "linux"))]
fn proc_alive(_pid: u32, _start_ticks: Option<u64>) -> bool {
    false
}

/// Reads the launcher's structured native exit report within [timeout].
fn read_native_report(path: &Path, timeout: Duration) -> Result<serde_json::Value> {
    let deadline = Instant::now() + timeout;
    loop {
        if let Ok(bytes) = fs::read(path) {
            return serde_json::from_slice(&bytes).with_context(|| {
                format!("native exit report is not valid JSON: {}", path.display())
            });
        }
        ensure!(
            Instant::now() < deadline,
            "the launcher wrote no native exit report at {}",
            path.display()
        );
        thread::sleep(Duration::from_millis(50));
    }
}

/// Waits on the real native process and returns its authoritative exit status.
///
/// `arm` is the instant the native host armed its single exit budget, so the
/// reported `elapsed` spans the real shutdown only. [timeout] guards a
/// permanently wedged process and is never a success signal. The launcher's
/// wrapper process is also reaped so no zombie is left, but its exit code is
/// deliberately not used to judge the native host.
#[allow(clippy::too_many_arguments)]
fn wait_native_exit(
    label: &str,
    working: &Path,
    _output: &Path,
    pid: u32,
    start_ticks: Option<u64>,
    gui: &mut OwnedProcess,
    interrupt: &mpsc::Receiver<()>,
    timeout: Duration,
    arm: Instant,
) -> Result<NativeExit> {
    let deadline = Instant::now() + timeout;
    loop {
        if !proc_alive(pid, start_ticks) {
            break;
        }
        ensure!(interrupt.try_recv().is_err(), "shutdown {label} cancelled");
        ensure!(
            Instant::now() < deadline,
            "shutdown {label} native process {pid} never exited within {timeout:?}"
        );
        thread::sleep(Duration::from_millis(50));
    }
    let elapsed = arm.elapsed();
    // Reap the wrapper so the tree does not leak a zombie; its own status is
    // secondary and never substitutes for the native report.
    let wrapper_deadline = Instant::now() + Duration::from_secs(20);
    loop {
        if gui.child.try_wait()?.is_some() {
            gui.stopped = true;
            break;
        }
        if Instant::now() >= wrapper_deadline {
            break;
        }
        thread::sleep(Duration::from_millis(50));
    }
    let report = read_native_report(&native_report_path(working, label), Duration::from_secs(20))?;
    ensure!(
        report["pid"].as_u64() == Some(u64::from(pid)),
        "native exit report pid does not match the Driver-reported native pid {pid}: {report}"
    );
    let code = report["exitCode"].as_i64().map(|value| value as i32);
    let signal = report["signal"].as_i64().map(|value| value as i32);
    let descendants_at_root_exit = pid_list(&report["descendantsAtRootExit"]);
    let reclaimed_descendants = pid_list(&report["reclaimedDescendants"]);
    Ok(NativeExit {
        pid,
        code,
        signal,
        elapsed,
        descendants_at_root_exit,
        reclaimed_descendants,
    })
}

/// Parses a JSON array of positive integer pids, ignoring malformed entries.
fn pid_list(value: &serde_json::Value) -> Vec<u32> {
    let mut pids = Vec::new();
    if let Some(values) = value.as_array() {
        for value in values {
            if let Some(pid) = value.as_u64()
                && pid > 0
            {
                pids.push(pid as u32);
            }
        }
    }
    pids
}

/// Waits on the real OS process for [gui]; [timeout] only guards a permanent
/// wedge and is never a success signal.
fn wait_gui_exit(
    label: &str,
    gui: &mut OwnedProcess,
    interrupt: &mpsc::Receiver<()>,
    timeout: Duration,
) -> Result<ExitStatus> {
    let deadline = Instant::now() + timeout;
    loop {
        if let Some(status) = gui.child.try_wait()? {
            gui.stopped = true;
            return Ok(status);
        }
        ensure!(interrupt.try_recv().is_err(), "shutdown {label} cancelled");
        ensure!(
            Instant::now() < deadline,
            "shutdown {label} GUI never exited within {timeout:?}"
        );
        thread::sleep(Duration::from_millis(100));
    }
}

/// Captures the instance-busy evidence of a second instance on a shared home,
/// tolerating a second process that never published a usable Driver, and truly
/// closes it through a real window-manager delete event.
///
/// The second instance runs on its own isolated [display] (`Xvfb`), so the
/// delete event exercises the native GTK close hook without touching the user's
/// desktop and without the Dart `beginExit` path the Driver-only
/// `request-app-exit` covers elsewhere. The closed window's real pid (its
/// `_NET_WM_PID`) is start-time-guarded, and the launcher's structured report
/// proves the native host exited with code 0.
#[allow(clippy::too_many_arguments)]
fn capture_second_instance(
    app_dir: &Path,
    home: &Path,
    _workspace: &Path,
    second_log: &Path,
    output: &Path,
    working: &Path,
    project: &Path,
    display: &str,
    mut second: OwnedProcess,
    interrupt: &mpsc::Receiver<()>,
    driver_evidence: &mut Vec<DriverEvidence>,
) -> Result<serde_json::Value> {
    // Give the second instance a bounded window to publish its VM service. A
    // startup failure is allowed: the log-based evidence below still counts.
    let vm_url = try_wait_for_vm(second_log, &mut second, Duration::from_secs(90));
    let mut snapshot_busy = false;
    let mut driver_observed = false;
    if let Some(vm_url) = vm_url {
        let driver_log = working.join("shutdown-busy-second-driver.log");
        match start_driver(
            app_dir,
            home,
            "busy-second",
            &vm_url,
            project,
            output,
            working,
            &driver_log,
        ) {
            Ok(mut driver) => {
                let exit = wait_driver(
                    "busy-second",
                    working,
                    &mut driver,
                    interrupt,
                    Duration::from_secs(180),
                )?;
                let summary = output.join("shutdown-busy-second-summary.json");
                if summary.is_file() {
                    let report: serde_json::Value = serde_json::from_slice(&fs::read(&summary)?)?;
                    snapshot_busy = report["checks"]["busySecondInstanceBusy"] == true;
                    driver_observed = true;
                }
                driver_evidence.push(DriverEvidence {
                    log: driver_log,
                    stem: "shutdown-busy-second".into(),
                    exit: Some(exit),
                });
            }
            Err(error) => {
                driver_evidence.push(DriverEvidence {
                    log: driver_log,
                    stem: "shutdown-busy-second".into(),
                    exit: None,
                });
                eprintln!("shutdown busy-second driver did not start: {error}");
            }
        }
    }
    let log_text = fs::read_to_string(second_log).unwrap_or_default();
    let log_busy = log_text.contains("instanceBusy")
        || log_text.contains("已有实例")
        || log_text.contains("instance busy");
    // The ArgumentError the previous revision hit must never reappear in the
    // second instance's real log; the regression is recorded, not hidden.
    let argument_error = log_text.contains("ArgumentError");
    // Close the second instance through a real window-manager delete event. This
    // exercises the native GTK close hook, which the Driver-only
    // `request-app-exit` (Dart `beginExit`) cannot.
    //
    // The delete is targeted at the exact native host: the sender only fires on
    // the window whose `_NET_WM_PID` equals the Driver-reported native pid (the
    // journey writes it early in `shutdown-native-busy-second.json`), and it
    // reads that host's real `DISPLAY` from `/proc/<pid>/environ` so a backend or
    // environment fallback can neither close an unrelated window nor let the
    // phase pass with no delivery. When the second instance never published a
    // usable Driver (allowed), the pid is unknown and the sender falls back to
    // the only window advertising the delete protocol, which is still proven
    // from the window itself and start-time-guarded below.
    let expected_native_pid = fs::read(output.join("shutdown-native-busy-second.json"))
        .ok()
        .and_then(|bytes| serde_json::from_slice::<serde_json::Value>(&bytes).ok())
        .and_then(|identity| identity["pid"].as_u64())
        .and_then(|pid| u32::try_from(pid).ok())
        .unwrap_or(0);
    let delete_event = send_window_delete_event(
        working,
        &output.join("shutdown-busy-second-x11.json"),
        &X11DeleteTarget {
            native_pid: expected_native_pid,
            display_hint: display,
        },
        Duration::from_secs(30),
    )?;
    let delete_event_pid = Some(delete_event.pid);
    let delete_event_ticks = proc_start_ticks(delete_event.pid);
    // Wait for the native host to exit on its own after the delete event; only a
    // process that neither exited nor answered is force-closed so the first
    // instance can proceed. A forced close is recorded and fails the check.
    let closed = if second.exited()?
        || wait_gui_exit("busy-second", &mut second, interrupt, EXIT_GRACE).is_ok()
    {
        true
    } else {
        force_close(&mut second)
    };
    // The delete-event pid must be genuinely gone (start-time guarded) before
    // the first instance is allowed to continue.
    let delete_event_native_exited = match (delete_event_pid, delete_event_ticks) {
        (Some(pid), _) => !proc_alive(pid, delete_event_ticks),
        (None, _) => false,
    };
    // The launcher's structured report carries the second native host's real
    // wait status, so "2nd OS exit" is proven by an OS process wait, not by a
    // wrapper exit code.
    let second_native = read_native_report(
        &native_report_path(working, "busy-second"),
        if closed {
            Duration::from_secs(20)
        } else {
            Duration::from_secs(5)
        },
    )
    .ok();
    let second_native_exit_code = second_native
        .as_ref()
        .and_then(|report| report["exitCode"].as_i64());
    let second_native_pid = second_native
        .as_ref()
        .and_then(|report| report["pid"].as_u64());
    // The delete event must have closed the same native host the launcher
    // reports, so the evidence ties the window protocol to the OS exit.
    let delete_event_matches_native = match (delete_event_pid, second_native_pid) {
        (Some(pid), Some(native)) => u64::from(pid) == native,
        _ => false,
    };
    // `second` is dropped at the end of this function either way; its own
    // `Drop` is the backstop that reaps any still-running descendant.
    Ok(serde_json::json!({
        "instanceBusyEvidence": snapshot_busy || log_busy,
        "snapshotBusy": snapshot_busy,
        "logBusy": log_busy,
        "argumentError": argument_error,
        "driverObserved": driver_observed,
        "closed": closed,
        "deleteEventPid": delete_event_pid,
        "deleteEventWindow": delete_event.window_id,
        "deleteEventDisplay": delete_event.display,
        "deleteEventX11Report": output
            .join("shutdown-busy-second-x11.json")
            .to_string_lossy()
            .into_owned(),
        "deleteEventStartTicks": delete_event_ticks,
        "deleteEventNativeExited": delete_event_native_exited,
        "deleteEventMatchesNative": delete_event_matches_native,
        "secondNativeExitCode": second_native_exit_code,
        "secondNativeReport": second_native,
    }))
}

/// Starts an isolated `Xvfb` the second instance renders into.
///
/// The display is scenario-owned (never the user's desktop), so a real
/// `WM_DELETE_WINDOW` can be delivered to the second instance's window without
/// disturbing anything outside this run. Returns the owned Xvfb and its
/// `DISPLAY` string.
fn start_isolated_xvfb(working: &Path) -> Result<(OwnedProcess, String)> {
    let mut last_error = String::new();
    for number in 90u32..120 {
        let lock = PathBuf::from(format!("/tmp/.X{number}-lock"));
        if lock.exists() {
            continue;
        }
        let display = format!(":{number}");
        let log = File::create(working.join(format!("shutdown-xvfb-{number}.log")))?;
        let mut command = Command::new("Xvfb");
        command
            .arg(&display)
            .args(["-screen", "0", "1280x1024x24", "-nolisten", "tcp"])
            .env("DISPLAY", &display)
            .stdout(Stdio::from(log.try_clone()?))
            .stderr(Stdio::from(log));
        let mut xvfb = OwnedProcess::start(&mut command, false)?;
        let socket = PathBuf::from(format!("/tmp/.X11-unix/X{number}"));
        let deadline = Instant::now() + Duration::from_secs(20);
        loop {
            if socket.exists() {
                return Ok((xvfb, display));
            }
            if xvfb.exited()? {
                last_error = format!(
                    "Xvfb {display} exited before publishing its socket: {}",
                    fs::read_to_string(working.join(format!("shutdown-xvfb-{number}.log")))
                        .unwrap_or_default()
                );
                break;
            }
            ensure!(
                Instant::now() < deadline,
                "Xvfb {display} did not publish /tmp/.X11-unix/X{number} within 20s"
            );
            thread::sleep(Duration::from_millis(100));
        }
    }
    bail!("no isolated X display could be started for the second instance: {last_error}")
}

/// The exact native target of a real `WM_DELETE_WINDOW` delivery.
///
/// [display_hint] is where the second instance was *meant* to render (the
/// scenario-owned `Xvfb`); the sender treats it as a fallback only and prefers
/// the host's own `DISPLAY` read from `/proc/<pid>/environ`. [native_pid] is the
/// Driver-reported host the delete must reach; `0` means the second instance
/// never published a usable Driver, so the sender targets the only window
/// advertising the delete protocol and the pid is proven from the window.
struct X11DeleteTarget<'a> {
    native_pid: u32,
    display_hint: &'a str,
}

/// A delivered `WM_DELETE_WINDOW`: the window's real `_NET_WM_PID`, its window
/// id and the display it was delivered on, for start-time guarding and evidence.
struct X11DeleteOutcome {
    pid: u32,
    window_id: String,
    display: String,
}

/// Delivers a real `WM_DELETE_WINDOW` to [target]'s native host window.
///
/// The sender walks the window tree on the host's real display (with
/// [X11DeleteTarget::display_hint] as fallback), keeps only the windows whose
/// `_NET_WM_PID` matches [X11DeleteTarget::native_pid] and that advertise
/// `WM_DELETE_WINDOW`, and deletes the largest of them. It always writes a rich
/// JSON report to [evidence] (candidate windows, their pids/protocols/names,
/// the displays tried and the reason) so a failure is diagnosable instead of an
/// empty stderr. A sender failure is an error: the delete-event evidence must
/// not be fabricated, and "no window" is never a pass.
fn send_window_delete_event(
    working: &Path,
    evidence: &Path,
    target: &X11DeleteTarget<'_>,
    timeout: Duration,
) -> Result<X11DeleteOutcome> {
    let script = working.join("shutdown-x11-delete-window.py");
    fs::write(&script, X11_DELETE_WINDOW_SCRIPT)?;
    // Drop any stale report so a sender that dies before writing cannot be
    // misread as this run's evidence.
    let _ = fs::remove_file(evidence);
    let sender = Command::new("python3")
        .arg(&script)
        .arg(target.display_hint)
        .arg(target.native_pid.to_string())
        .arg(timeout.as_secs().to_string())
        .arg(evidence)
        .output()
        .context("failed to run the X11 delete-event sender")?;
    let stdout = String::from_utf8_lossy(&sender.stdout);
    let stderr = String::from_utf8_lossy(&sender.stderr);
    let report = fs::read(evidence)
        .ok()
        .and_then(|bytes| serde_json::from_slice::<serde_json::Value>(&bytes).ok());
    // Re-emit the evidence with the sender's own streams and status alongside
    // the script's window report, so a failed run keeps stdout/stderr and the
    // candidate windows instead of an empty message.
    let evidence_value = serde_json::json!({
        "schema": "anywork-x11-delete/1",
        "status": sender.status.code(),
        "success": sender.status.success(),
        "stdout": stdout.trim(),
        "stderr": stderr.trim(),
        "senderReport": report.clone().unwrap_or(serde_json::Value::Null),
    });
    if let Ok(bytes) = serde_json::to_vec_pretty(&evidence_value) {
        let _ = fs::write(evidence, bytes);
    }
    let reason = report
        .as_ref()
        .and_then(|value| value["reason"].as_str())
        .filter(|reason| !reason.is_empty())
        .unwrap_or("the sender reported no reason");
    let first = stdout.lines().next().unwrap_or_default().trim();
    ensure!(
        sender.status.success() && !first.is_empty() && first != "none",
        "the X11 delete-event sender did not deliver WM_DELETE_WINDOW to native \
         pid {} (status {:?}, stdout {:?}, stderr {:?}, report {}): {reason}",
        target.native_pid,
        sender.status.code(),
        stdout.trim(),
        stderr.trim(),
        evidence.display()
    );
    let pid = first
        .parse::<u32>()
        .with_context(|| format!("the X11 sender printed an unexpected pid: {first:?}"))?;
    let window_id = report
        .as_ref()
        .and_then(|value| value["delivered"]["window"].as_str())
        .unwrap_or_default()
        .to_owned();
    let display = report
        .as_ref()
        .and_then(|value| value["delivered"]["display"].as_str())
        .unwrap_or(target.display_hint)
        .to_owned();
    Ok(X11DeleteOutcome {
        pid,
        window_id,
        display,
    })
}

/// Sends a real `WM_DELETE_WINDOW` `ClientMessage` through libX11 (ctypes).
///
/// Flutter Driver cannot deliver a window-manager protocol event, so this small
/// engineering script does it directly through libX11 (ctypes). It reads the
/// target host's own `DISPLAY` (and `XAUTHORITY`) from `/proc/<pid>/environ`,
/// walking the given display only as a fallback; it then walks the window tree,
/// keeps the windows whose `_NET_WM_PID` equals the expected pid and that
/// advertise `WM_DELETE_WINDOW`, deletes the largest of them and prints that
/// pid. It always writes a JSON report (candidate windows with pid, protocols,
/// name and geometry, the displays tried and the failure reason) so a failure
/// is diagnosable. Everything is bounded and a failure exits non-zero.
const X11_DELETE_WINDOW_SCRIPT: &str = r##"#!/usr/bin/env python3
import ctypes
import json
import sys
import time


class XClientMessageEvent(ctypes.Structure):
    _fields_ = [
        ("type", ctypes.c_int),
        ("serial", ctypes.c_ulong),
        ("send_event", ctypes.c_int),
        ("display", ctypes.c_void_p),
        ("window", ctypes.c_ulong),
        ("message_type", ctypes.c_ulong),
        ("format", ctypes.c_int),
        ("data", ctypes.c_long * 5),
    ]


class XEvent(ctypes.Union):
    _fields_ = [
        ("xclient", XClientMessageEvent),
        ("pad", ctypes.c_long * 24),
    ]


def load_x11():
    x11 = ctypes.CDLL("libX11.so.6")
    x11.XOpenDisplay.restype = ctypes.c_void_p
    x11.XOpenDisplay.argtypes = [ctypes.c_char_p]
    x11.XDefaultRootWindow.restype = ctypes.c_ulong
    x11.XDefaultRootWindow.argtypes = [ctypes.c_void_p]
    x11.XInternAtom.restype = ctypes.c_ulong
    x11.XInternAtom.argtypes = [ctypes.c_void_p, ctypes.c_char_p, ctypes.c_int]
    x11.XGetAtomName.restype = ctypes.c_void_p
    x11.XGetAtomName.argtypes = [ctypes.c_void_p, ctypes.c_ulong]
    x11.XQueryTree.restype = ctypes.c_int
    x11.XQueryTree.argtypes = [
        ctypes.c_void_p,
        ctypes.c_ulong,
        ctypes.POINTER(ctypes.c_ulong),
        ctypes.POINTER(ctypes.c_ulong),
        ctypes.POINTER(ctypes.POINTER(ctypes.c_ulong)),
        ctypes.POINTER(ctypes.c_uint),
    ]
    x11.XGetWindowProperty.restype = ctypes.c_int
    x11.XGetWindowProperty.argtypes = [
        ctypes.c_void_p,
        ctypes.c_ulong,
        ctypes.c_ulong,
        ctypes.c_long,
        ctypes.c_long,
        ctypes.c_int,
        ctypes.c_ulong,
        ctypes.POINTER(ctypes.c_ulong),
        ctypes.POINTER(ctypes.c_int),
        ctypes.POINTER(ctypes.c_ulong),
        ctypes.POINTER(ctypes.c_ulong),
        ctypes.POINTER(ctypes.POINTER(ctypes.c_ubyte)),
    ]
    x11.XGetGeometry.restype = ctypes.c_int
    x11.XGetGeometry.argtypes = [
        ctypes.c_void_p,
        ctypes.c_ulong,
        ctypes.POINTER(ctypes.c_ulong),
        ctypes.POINTER(ctypes.c_int),
        ctypes.POINTER(ctypes.c_int),
        ctypes.POINTER(ctypes.c_uint),
        ctypes.POINTER(ctypes.c_uint),
        ctypes.POINTER(ctypes.c_uint),
        ctypes.POINTER(ctypes.c_uint),
    ]
    x11.XFetchName.restype = ctypes.c_int
    x11.XFetchName.argtypes = [
        ctypes.c_void_p,
        ctypes.c_ulong,
        ctypes.POINTER(ctypes.c_char_p),
    ]
    x11.XSendEvent.restype = ctypes.c_int
    x11.XSendEvent.argtypes = [
        ctypes.c_void_p,
        ctypes.c_ulong,
        ctypes.c_int,
        ctypes.c_long,
        ctypes.POINTER(XEvent),
    ]
    x11.XFlush.argtypes = [ctypes.c_void_p]
    x11.XSync.restype = ctypes.c_int
    x11.XSync.argtypes = [ctypes.c_void_p, ctypes.c_int]
    x11.XFree.argtypes = [ctypes.c_void_p]
    return x11


def read_environ(pid):
    try:
        with open("/proc/{}/environ".format(pid), "rb") as handle:
            raw = handle.read()
    except OSError:
        return {}
    env = {}
    for chunk in raw.split(b"\0"):
        if not chunk:
            continue
        key, _, value = chunk.partition(b"=")
        env[key.decode("utf-8", "replace")] = value.decode("utf-8", "replace")
    return env


def write_report(path, report):
    if not path:
        return
    try:
        with open(path, "w") as handle:
            json.dump(report, handle, indent=2, sort_keys=True)
    except OSError:
        pass


def sanitize(session):
    return {
        "display": session.get("display"),
        "opened": session.get("opened", False),
        "root": session.get("root"),
        "error": session.get("error"),
        "windows": session.get("windows") or [],
    }


def main():
    display_hint = sys.argv[1]
    expected_pid = int(sys.argv[2]) if len(sys.argv) > 2 else 0
    budget = float(sys.argv[3]) if len(sys.argv) > 3 else 30.0
    report_path = sys.argv[4] if len(sys.argv) > 4 else None
    deadline = time.time() + budget
    env = read_environ(expected_pid) if expected_pid else {}
    process_display = env.get("DISPLAY") or None
    displays = []
    for candidate in (process_display, display_hint):
        if candidate and candidate not in displays:
            displays.append(candidate)
    if not displays:
        displays = [":0"]
    report = {
        "nativePid": expected_pid,
        "displayHint": display_hint,
        "processDisplay": process_display,
        "waylandDisplay": env.get("WAYLAND_DISPLAY") or None,
        "xauthority": env.get("XAUTHORITY") or None,
        "displaysTried": displays,
        "reason": None,
        "delivered": None,
        "windows": [],
    }
    try:
        x11 = load_x11()
    except OSError as error:
        report["reason"] = "libX11 could not be loaded: {}".format(error)
        write_report(report_path, report)
        print("none")
        return 2

    def atom_name(dpy, atom):
        pointer = x11.XGetAtomName(dpy, atom)
        if not pointer:
            return None
        name = ctypes.cast(pointer, ctypes.c_char_p).value
        x11.XFree(pointer)
        return name.decode("utf-8", "replace") if name else None

    def children(dpy, win):
        r = ctypes.c_ulong()
        p = ctypes.c_ulong()
        kids = ctypes.POINTER(ctypes.c_ulong)()
        n = ctypes.c_uint()
        ok = x11.XQueryTree(
            dpy,
            win,
            ctypes.byref(r),
            ctypes.byref(p),
            ctypes.byref(kids),
            ctypes.byref(n),
        )
        if not ok:
            return []
        out = [kids[i] for i in range(n.value)]
        if kids:
            x11.XFree(kids)
        return out

    def prop(dpy, win, atom):
        actual = ctypes.c_ulong()
        fmt = ctypes.c_int()
        nitems = ctypes.c_ulong()
        after = ctypes.c_ulong()
        data = ctypes.POINTER(ctypes.c_ubyte)()
        status = x11.XGetWindowProperty(
            dpy,
            win,
            atom,
            0,
            1024,
            0,
            0,
            ctypes.byref(actual),
            ctypes.byref(fmt),
            ctypes.byref(nitems),
            ctypes.byref(after),
            ctypes.byref(data),
        )
        if status != 0 or not data:
            return None, None
        values = []
        if fmt.value == 32:
            longs = ctypes.cast(data, ctypes.POINTER(ctypes.c_ulong))
            values = [longs[i] for i in range(nitems.value)]
        x11.XFree(data)
        return values, fmt.value

    def geometry(dpy, win):
        root = ctypes.c_ulong()
        x = ctypes.c_int()
        y = ctypes.c_int()
        width = ctypes.c_uint()
        height = ctypes.c_uint()
        border = ctypes.c_uint()
        depth = ctypes.c_uint()
        ok = x11.XGetGeometry(
            dpy,
            win,
            ctypes.byref(root),
            ctypes.byref(x),
            ctypes.byref(y),
            ctypes.byref(width),
            ctypes.byref(height),
            ctypes.byref(border),
            ctypes.byref(depth),
        )
        if not ok:
            return None
        return {"width": width.value, "height": height.value}

    def window_name(dpy, win):
        name = ctypes.c_char_p()
        if not x11.XFetchName(dpy, win, ctypes.byref(name)):
            return None
        # Read the value before freeing the X-allocated string.
        value = name.value
        if name:
            x11.XFree(name)
        return value.decode("utf-8", "replace") if value else None

    def send_delete(dpy, win, atom_protocols, atom_delete):
        event = XEvent()
        event.xclient.type = 33
        event.xclient.serial = 0
        event.xclient.send_event = 0
        event.xclient.display = dpy
        event.xclient.window = win
        event.xclient.message_type = atom_protocols
        event.xclient.format = 32
        event.xclient.data[0] = atom_delete
        return x11.XSendEvent(dpy, win, 0, 0, ctypes.byref(event))

    sessions = []
    for display in displays:
        session = {"display": display, "opened": False, "error": None, "windows": []}
        dpy = x11.XOpenDisplay(display.encode())
        if not dpy:
            session["error"] = "XOpenDisplay failed"
            sessions.append(session)
            continue
        session["opened"] = True
        session["root"] = hex(x11.XDefaultRootWindow(dpy))
        session["_dpy"] = dpy
        session["_atoms"] = (
            x11.XInternAtom(dpy, b"WM_PROTOCOLS", 0),
            x11.XInternAtom(dpy, b"WM_DELETE_WINDOW", 0),
            x11.XInternAtom(dpy, b"_NET_WM_PID", 0),
        )
        sessions.append(session)

    def refresh(sessions):
        report["displays"] = [sanitize(session) for session in sessions]

    while time.time() < deadline:
        for session in sessions:
            if not session["opened"]:
                continue
            dpy = session["_dpy"]
            atom_protocols, atom_delete, atom_pid = session["_atoms"]
            root = x11.XDefaultRootWindow(dpy)
            windows = []
            stack = [root]
            while stack:
                win = stack.pop()
                stack.extend(children(dpy, win))
                if win == root:
                    continue
                protocols, _ = prop(dpy, win, atom_protocols)
                pids, _ = prop(dpy, win, atom_pid)
                windows.append(
                    {
                        "window": hex(win),
                        "pid": pids[0] if pids else None,
                        "protocols": [
                            atom_name(dpy, atom) for atom in protocols
                        ]
                        if protocols
                        else [],
                        "name": window_name(dpy, win),
                        "geometry": geometry(dpy, win),
                    }
                )
            session["windows"] = windows
            matches = [
                window
                for window in windows
                if window["pid"] is not None
                and (expected_pid == 0 or window["pid"] == expected_pid)
                and "WM_DELETE_WINDOW" in (window["protocols"] or [])
            ]
            if not matches:
                continue
            target = max(
                matches,
                key=lambda window: (window["geometry"] or {}).get("width", 0)
                * (window["geometry"] or {}).get("height", 0),
            )
            win = int(target["window"], 16)
            if not send_delete(dpy, win, atom_protocols, atom_delete):
                session["error"] = "XSendEvent failed for window {}".format(
                    target["window"]
                )
                report["reason"] = "XSendEvent failed for window {} on {}".format(
                    target["window"], session["display"]
                )
                report["delivered"] = {
                    "display": session["display"],
                    "window": target["window"],
                    "pid": target["pid"],
                    "sendFailed": True,
                }
                refresh(sessions)
                write_report(report_path, report)
                print("none")
                return 3
            x11.XSync(dpy, 0)
            report["delivered"] = {
                "display": session["display"],
                "window": target["window"],
                "pid": target["pid"],
                "sendFailed": False,
            }
            refresh(sessions)
            write_report(report_path, report)
            print(target["pid"])
            return 0
        time.sleep(0.2)

    reasons = []
    for session in sessions:
        if not session["opened"]:
            reasons.append(
                "{}: not opened ({})".format(session["display"], session["error"])
            )
            continue
        windows = session["windows"] or []
        if not windows:
            reasons.append("{}: no client windows found".format(session["display"]))
            continue
        if expected_pid == 0:
            reasons.append(
                "{}: {} window(s), none has a readable _NET_WM_PID and "
                "WM_DELETE_WINDOW".format(session["display"], len(windows))
            )
            continue
        owned = [window for window in windows if window["pid"] == expected_pid]
        if not owned:
            reasons.append(
                "{}: {} window(s), none reports _NET_WM_PID {}".format(
                    session["display"], len(windows), expected_pid
                )
            )
        else:
            reasons.append(
                "{}: {} window(s) report pid {} but none advertises "
                "WM_DELETE_WINDOW".format(
                    session["display"], len(owned), expected_pid
                )
            )
    report["windows"] = [
        window for session in sessions for window in session["windows"] or []
    ]
    report["reason"] = "; ".join(reasons) if reasons else "no display could be tried"
    refresh(sessions)
    write_report(report_path, report)
    print("none")
    return 1


if __name__ == "__main__":
    sys.exit(main())
"##;

/// Reads the fixture live strict-script counters, when the watcher has written.
fn read_live_status(path: &Path) -> Result<pl_provider_fixture::FixtureLiveStatus> {
    serde_json::from_slice(
        &fs::read(path)
            .with_context(|| format!("fixture live status missing: {}", path.display()))?,
    )
    .context("fixture live status is not a valid counter snapshot")
}

/// Copies the canonical `ANYWORK_HOME/studio/logs` diagnostics into evidence.
///
/// The acceptance must judge the PID/stage/elapsed/error-code/correlation/
/// persistence facts from the product's canonical diagnostics directory, not
/// only from the captured stdout log, so the files are copied beside the rest
/// of the run's evidence. Returns how many files were copied.
fn copy_home_diagnostics(home: &Path, output: &Path, label: &str) -> Result<usize> {
    let source = home.join("studio").join("logs");
    let destination = output.join("diagnostics").join(label);
    let mut copied = 0usize;
    if !source.is_dir() {
        return Ok(copied);
    }
    fs::create_dir_all(&destination)?;
    for entry in fs::read_dir(&source)? {
        let entry = entry?;
        if entry.file_type()?.is_file() {
            fs::copy(entry.path(), destination.join(entry.file_name()))?;
            copied += 1;
        }
    }
    Ok(copied)
}

/// Resolves the `pl-provider-fixture` bin the shutdown MCP server runs.
///
/// Uses the same no-op `cargo build --message-format=json` discovery the xtask
/// acceptance launcher uses, so an explicit `CARGO_TARGET_DIR` or configured
/// build target is honored instead of assuming a bare `target/debug` path.
fn resolve_fixture_binary(workspace: &Path) -> Result<PathBuf> {
    let mut build = process::path_command("cargo", &[]);
    build.current_dir(workspace).args([
        "build",
        "-p",
        "pl-provider-fixture",
        "--message-format=json",
    ]);
    let output = build
        .output()
        .context("failed to run cargo build discovery")?;
    ensure!(
        output.status.success(),
        "failed to build pl-provider-fixture: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    for line in String::from_utf8_lossy(&output.stdout).lines() {
        let Ok(record) = serde_json::from_str::<serde_json::Value>(line) else {
            continue;
        };
        if record["reason"] == "compiler-artifact"
            && record["target"]["name"] == "pl-provider-fixture"
            && record["target"]["kind"]
                .as_array()
                .is_some_and(|kinds| kinds.iter().any(|kind| kind == "bin"))
            && let Some(executable) = record["executable"].as_str()
        {
            return Ok(PathBuf::from(executable));
        }
    }
    bail!("cargo reported no pl-provider-fixture bin executable")
}

/// Reads a small JSON evidence file written by the Driver journey.
fn read_phase_json(path: &Path) -> Result<serde_json::Value> {
    serde_json::from_slice(
        &fs::read(path).with_context(|| format!("missing phase evidence {}", path.display()))?,
    )
    .with_context(|| format!("invalid phase evidence {}", path.display()))
}

/// Reads the native host's canonical exit diagnostics for [home].
///
/// The native host (`runner_common/studio_host_lifecycle.cc`) writes merged
/// `anywork-exit` lines to `ANYWORK_HOME/studio/logs/anywork-exit-diagnostics.log`
/// carrying `stage`/`code`/`reason`/`correlation`/`pending`/`elapsed_ms`. The
/// last line is the authoritative terminal snapshot: a `finish` event is a
/// coordinated `finishExit`, while a `final` snapshot with `exitDeadlineImminent`
/// is the 30-second watchdog. A missing or empty log is a hard error because the
/// concurrent phase must confirm the blocked save never completed, not assume it.
fn read_native_exit_diagnostics(home: &Path) -> Result<serde_json::Value> {
    let path = home
        .join("studio")
        .join("logs")
        .join("anywork-exit-diagnostics.log");
    let text = fs::read_to_string(&path)
        .with_context(|| format!("missing native exit diagnostics {}", path.display()))?;
    let mut last: std::collections::BTreeMap<String, String> = std::collections::BTreeMap::new();
    let mut coordinated_finish = false;
    let mut lines = 0usize;
    for line in text.lines() {
        let Some(rest) = line.strip_prefix("anywork-exit ") else {
            continue;
        };
        let mut fields: std::collections::BTreeMap<String, String> =
            std::collections::BTreeMap::new();
        for token in rest.split_whitespace() {
            if let Some((key, value)) = token.split_once('=') {
                fields.insert(key.to_owned(), value.to_owned());
            }
        }
        if fields.is_empty() {
            continue;
        }
        if fields.get("event").is_some_and(|event| event == "finish") {
            coordinated_finish = true;
        }
        last = fields;
        lines += 1;
    }
    ensure!(
        lines > 0,
        "native exit diagnostics had no anywork-exit line: {}",
        path.display()
    );
    let code = last.get("code").cloned().unwrap_or_default();
    let pending = last.get("pending").cloned().unwrap_or_default();
    Ok(serde_json::json!({
        "log": path.display().to_string(),
        "lines": lines,
        "event": last.get("event").cloned().unwrap_or_default(),
        "stage": last.get("stage").cloned().unwrap_or_default(),
        "code": code,
        "reason": last.get("reason").cloned().unwrap_or_default(),
        "correlation": last.get("correlation").cloned().unwrap_or_default(),
        "pending": pending,
        "pendingUnknown": pending == "unknown",
        "elapsedMillis": last.get("elapsed_ms").and_then(|value| value.parse::<u64>().ok()),
        "coordinatedFinish": coordinated_finish,
        "deadlineImminent": last.get("code").is_some_and(|value| value == "exitDeadlineImminent"),
    }))
}

/// Machine-validates a Dart error log directory (or a single fallback log dir).
///
/// The Dart logger writes entries with `stage=`, `correlation=` and a stack, but
/// no pid field; the caller records the OS pid alongside. Returns the actual
/// per-field findings so the acceptance never claims a copy it did not check.
fn validate_dart_error_dir(dir: &Path, expect_stage: &str) -> Result<serde_json::Value> {
    let mut files = Vec::new();
    let mut contains_stage = false;
    let mut contains_correlation = false;
    let mut contains_stack = false;
    let mut bytes = 0usize;
    if dir.is_dir() {
        for entry in fs::read_dir(dir)? {
            let entry = entry?;
            if !entry.file_type()?.is_file() {
                continue;
            }
            let name = entry.file_name().to_string_lossy().into_owned();
            if !name.contains("dart-error") {
                continue;
            }
            let text = fs::read_to_string(entry.path()).unwrap_or_default();
            bytes += text.len();
            contains_stage |= text.contains(&format!("stage={expect_stage}"));
            contains_correlation |= text.contains("correlation=corr-");
            contains_stack |=
                text.contains("#0") || text.contains(".dart:") || text.contains(" at ");
            files.push(name);
        }
    }
    Ok(serde_json::json!({
        "directory": dir.display().to_string(),
        "files": files,
        "bytes": bytes,
        "containsStage": contains_stage,
        "containsCorrelation": contains_correlation,
        "containsStack": contains_stack,
    }))
}

/// Polls the real business PIDs of a supervised subtree to prove they exited.
///
/// Started before the coordinator OS-waits the native host, the monitor records
/// the wall-clock when each pid disappeared (with a start-time reuse guard) and
/// any pid still alive at the deadline, so the reclamation is judged from real
/// OS facts rather than the harness's own force-kill.
fn monitor_business_pids(
    pids: Vec<(u32, Option<u64>)>,
    timeout: Duration,
) -> thread::JoinHandle<serde_json::Value> {
    thread::spawn(move || {
        let started = Instant::now();
        let deadline = started + timeout;
        let mut remaining = pids;
        let mut observed = Vec::new();
        loop {
            remaining.retain(|(pid, ticks)| {
                if proc_alive(*pid, *ticks) {
                    true
                } else {
                    observed.push(serde_json::json!({
                        "pid": pid,
                        "exitedAtMillis": started.elapsed().as_millis(),
                    }));
                    false
                }
            });
            if remaining.is_empty() || Instant::now() >= deadline {
                break;
            }
            thread::sleep(Duration::from_millis(50));
        }
        serde_json::json!({
            "observed": observed,
            "stillAlive": remaining.iter().map(|(pid, _)| *pid).collect::<Vec<u32>>(),
            "elapsedMillis": started.elapsed().as_millis(),
        })
    })
}

/// Monitors the `concurrent-stop` business subtrees and the native host on one
/// shared clock.
///
/// Unlike [`monitor_business_pids`] this also records the moment the native
/// process itself disappeared, so each resource's stop time can be compared
/// directly against the real OS exit rather than two unrelated clocks. A
/// resource that only vanished with (or after) the native force-out is therefore
/// a serialized stop, never a concurrent one.
fn monitor_concurrent_stop(
    native: (u32, Option<u64>),
    business: Vec<(u32, Option<u64>)>,
    timeout: Duration,
) -> thread::JoinHandle<serde_json::Value> {
    thread::spawn(move || {
        let started = Instant::now();
        let deadline = started + timeout;
        let (native_pid, native_ticks) = native;
        let mut native_exited_at: Option<u128> = None;
        let mut remaining = business;
        let mut observed = Vec::new();
        loop {
            if native_exited_at.is_none() && !proc_alive(native_pid, native_ticks) {
                native_exited_at = Some(started.elapsed().as_millis());
            }
            remaining.retain(|(pid, ticks)| {
                if proc_alive(*pid, *ticks) {
                    true
                } else {
                    observed.push(serde_json::json!({
                        "pid": pid,
                        "exitedAtMillis": started.elapsed().as_millis(),
                    }));
                    false
                }
            });
            if (remaining.is_empty() && native_exited_at.is_some()) || Instant::now() >= deadline {
                break;
            }
            thread::sleep(Duration::from_millis(50));
        }
        serde_json::json!({
            "observed": observed,
            "stillAlive": remaining.iter().map(|(pid, _)| *pid).collect::<Vec<u32>>(),
            "nativeExitedAtMillis": native_exited_at,
            "elapsedMillis": started.elapsed().as_millis(),
        })
    })
}

/// The VM-service URL the Flutter engine published on [log], if any.
fn find_vm_url(log: &Path) -> Option<String> {
    let text = fs::read_to_string(log).ok()?;
    text.lines()
        .filter_map(|line| {
            line.split("available at: http://127.0.0.1:")
                .nth(1)
                .or_else(|| line.split("listening on http://127.0.0.1:").nth(1))
                .map(|tail| {
                    format!(
                        "http://127.0.0.1:{}",
                        tail.split_whitespace().next().unwrap_or("")
                    )
                })
        })
        .next_back()
}

/// A bounded, non-fatal VM-service wait used only for the catchable second
/// instance, which is allowed to fail its startup.
fn try_wait_for_vm(log: &Path, gui: &mut OwnedProcess, timeout: Duration) -> Option<String> {
    let deadline = Instant::now() + timeout;
    loop {
        if let Some(url) = find_vm_url(log) {
            return Some(url);
        }
        if gui.exited().unwrap_or(true) {
            return None;
        }
        if Instant::now() >= deadline {
            return None;
        }
        thread::sleep(Duration::from_millis(200));
    }
}

/// Enumerates the real descendant PIDs of [root] on Linux by walking only
/// `/proc/<pid>/task/*/children`; never a global scan.
#[cfg(target_os = "linux")]
fn owned_children(root: u32) -> Vec<u32> {
    use std::collections::HashSet;

    let mut found = Vec::new();
    let mut stack = vec![root];
    let mut seen: HashSet<u32> = HashSet::new();
    seen.insert(root);
    while let Some(pid) = stack.pop() {
        let task_dir = format!("/proc/{pid}/task");
        let Ok(entries) = fs::read_dir(&task_dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let children = entry.path().join("children");
            let Ok(text) = fs::read_to_string(&children) else {
                continue;
            };
            for token in text.split_whitespace() {
                if let Ok(child) = token.parse::<u32>()
                    && seen.insert(child)
                {
                    found.push(child);
                    stack.push(child);
                }
            }
        }
    }
    found.sort_unstable();
    found
}

#[cfg(not(target_os = "linux"))]
fn owned_children(_root: u32) -> Vec<u32> {
    Vec::new()
}

/// Freezes the owned provider-fixture process group (SIGSTOP) so the service is
/// genuinely unresponsive.
#[cfg(unix)]
fn freeze_fixture(fixture: &OwnedProcess) -> Result<()> {
    signal_fixture(fixture, "STOP")
}

/// Resumes the frozen provider fixture (SIGCONT).
#[cfg(unix)]
fn resume_fixture(fixture: &OwnedProcess) -> Result<()> {
    signal_fixture(fixture, "CONT")
}

#[cfg(not(unix))]
fn freeze_fixture(_fixture: &OwnedProcess) -> Result<()> {
    bail!("service-unresponsive phase requires a POSIX signal host")
}

#[cfg(not(unix))]
fn resume_fixture(_fixture: &OwnedProcess) -> Result<()> {
    Ok(())
}

#[cfg(unix)]
fn signal_fixture(fixture: &OwnedProcess, signal: &str) -> Result<()> {
    signal_group(fixture.child.id(), signal)
}

/// Sends [signal] to the whole process group [pid] owns (never a bare PID), so
/// descendants are covered without a global scan.
#[cfg(unix)]
fn signal_group(pid: u32, signal: &str) -> Result<()> {
    let status = Command::new("kill")
        .arg(format!("-{signal}"))
        .arg("--")
        .arg(format!("-{pid}"))
        .status()?;
    ensure!(
        status.success(),
        "failed to send {signal} to process group {pid}: {status}"
    );
    Ok(())
}

/// Force-closes an owned GUI whose Driver never became usable and reports
/// whether its OS process is really gone.
///
/// [`OwnedProcess::stop`] is fixture-specific (it waits for a requests-file a
/// GUI never writes), so this sends the group `TERM`, then `KILL`, and reaps the
/// direct child between escalations. It is only the fallback for a second
/// instance that neither exited on its own nor answered the graceful Driver
/// request.
#[cfg(unix)]
fn force_close(process: &mut OwnedProcess) -> bool {
    // A process that already reaped itself needs no signal; report success so
    // the caller records a real exit rather than a spurious failure.
    if matches!(process.child.try_wait(), Ok(Some(_))) {
        process.stopped = true;
        return true;
    }
    for signal in ["TERM", "KILL"] {
        if signal_group(process.child.id(), signal).is_err() {
            return false;
        }
        if wait_reaped(process, Duration::from_secs(10)) {
            return true;
        }
    }
    false
}

/// Force-closes an owned GUI on Windows by killing the whole process tree.
#[cfg(windows)]
fn force_close(process: &mut OwnedProcess) -> bool {
    let _ = Command::new("taskkill")
        .args(["/T", "/F", "/PID"])
        .arg(process.child.id().to_string())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
    wait_reaped(process, Duration::from_secs(10))
}

/// Polls the direct child for a real reaped status within [timeout].
fn wait_reaped(process: &mut OwnedProcess, timeout: Duration) -> bool {
    let deadline = Instant::now() + timeout;
    loop {
        if matches!(process.child.try_wait(), Ok(Some(_))) {
            process.stopped = true;
            return true;
        }
        if Instant::now() >= deadline {
            return false;
        }
        thread::sleep(Duration::from_millis(100));
    }
}
