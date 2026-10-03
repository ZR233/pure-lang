//! Evidence for recovery through the real Thread, projection, bridge and native GUI.
use super::context_replay_recovery::{wait_driver, wait_stage};
use super::*;
use sea_orm::sqlx::Row;
use serde_json::{Value, json};

pub(super) fn run(
    context: &RecoveryScenario<'_>,
    mut fixture: OwnedProcess,
    ready: &FixtureReady,
) -> Result<()> {
    let native = ready.scenario == "call-lifecycle-recovery";
    let project = context.working.join("websocket-project");
    fs::create_dir(&project)?;
    ensure!(
        Command::new("git")
            .args(["init", "-q"])
            .arg(&project)
            .status()?
            .success(),
        "failed to initialize isolated WebSocket project"
    );
    let mut logs = Vec::new();
    let mut lifecycles = Vec::new();
    let journey = (|| -> Result<()> {
        let mut prior_history = None;
        for phase in ["first", "restart"] {
            let gui_log = context.working.join(format!("{phase}-gui.log"));
            let driver_log = context.working.join(format!("{phase}-driver.log"));
            logs.push((
                gui_log.clone(),
                context.output.join(format!("{phase}-gui.log")),
            ));
            logs.push((
                driver_log.clone(),
                context.output.join(format!("{phase}-driver.log")),
            ));
            let mut gui = start_recovery_gui(context.workspace, context.home, &gui_log)?;
            let vm = wait_for_vm(&gui_log, &mut gui, &mut fixture, context.interrupt)?;
            let log = File::create(&driver_log)?;
            let mut command = process::path_command("dart", &[]);
            command
                .current_dir(context.app_dir)
                .args([
                    "run",
                    if native {
                        "test_driver/call_lifecycle_recovery_journey.dart"
                    } else {
                        "test_driver/websocket_recovery_journey.dart"
                    },
                    phase,
                    "ws",
                    &vm,
                ])
                .arg(&project)
                .arg(context.output)
                .arg(context.working)
                .stdout(Stdio::from(log.try_clone()?))
                .stderr(Stdio::from(log));
            if native {
                command.arg(context.home);
            }
            let mut driver = OwnedProcess::start(&mut command, false)?;
            let gui_pid = gui.child.id();
            let driver_pid = driver.child.id();
            let stage = if phase == "first" {
                "cancel"
            } else {
                "restart"
            };
            wait_stage(
                &context.working.join(format!("{stage}-opened")),
                &mut driver,
                &mut gui,
                &mut fixture,
                context.interrupt,
            )?;
            let before = plan_recovery_settled_baseline(context.status_file)?;
            if phase == "restart" {
                let restored = read_history(context.home, &observed_thread(context.working)?)?;
                ensure!(
                    Some(&restored) == prior_history.as_ref(),
                    "reopen changed canonical history or frozen context"
                );
            }
            let quiet_duration = if phase == "first" { 31 } else { 2 };
            let deadline = Instant::now() + Duration::from_secs(quiet_duration);
            while Instant::now() < deadline {
                ensure!(
                    !gui.exited()? && !fixture.exited()? && driver.child.try_wait()?.is_none(),
                    "WebSocket evidence process exited during quiet window"
                );
                ensure!(
                    context.interrupt.try_recv().is_err(),
                    "WebSocket journey cancelled"
                );
                thread::sleep(Duration::from_millis(100));
            }
            let after = plan_recovery_read_status(context.status_file)?;
            ensure!(
                before == after && after.rejected == 0,
                "{stage} performed unexpected provider work: {before:?} -> {after:?}"
            );
            fs::write(
                context.output.join(format!("{stage}-quiet.json")),
                serde_json::to_vec_pretty(
                    &json!({"before":before,"after":after,"seconds":quiet_duration}),
                )?,
            )?;
            fs::write(context.working.join(format!("{stage}-quiet")), "verified")?;
            wait_driver(&mut driver, &mut gui, &mut fixture, context.interrupt)?;
            let summary: Value = serde_json::from_slice(&fs::read(
                context.output.join(format!("{phase}-summary.json")),
            )?)?;
            ensure!(
                summary["status"] == "complete" && summary["shutdown"] == "completed",
                "{phase} did not complete native shutdown"
            );
            drop(driver);
            drop(gui);
            lifecycles.push(json!({"phase":phase,"guiLauncherPid":gui_pid,"driverLauncherPid":driver_pid,
                "shutdown":"completed","ownersReapedAt":SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs()}));
            fs::write(
                context.output.join("lifecycle.json"),
                serde_json::to_vec_pretty(&lifecycles)?,
            )?;
            let history = read_history(context.home, &observed_thread(context.working)?)?;
            validate_history(&history, native)?;
            if phase == "restart" {
                ensure!(
                    Some(&history) == prior_history.as_ref(),
                    "restart appended or changed saved history"
                );
            }
            fs::write(
                context.output.join(format!("{phase}-history.json")),
                serde_json::to_vec_pretty(&history)?,
            )?;
            prior_history = Some(history);
        }
        Ok(())
    })();
    for (source, destination) in &logs {
        if source.is_file() {
            write_sanitized_log(source, destination)?;
        }
    }
    let fixture_exit = fixture.stop(context.requests_file);
    drop(fixture);
    write_fixture_log(context.fixture_log, &context.output.join("fixture.log"))?;
    let requests = if context.requests_file.is_file() {
        sanitize_requests(context.requests_file, &context.output.join("requests.json"))?;
        serde_json::from_slice::<Vec<Value>>(&fs::read(context.requests_file)?)?
    } else {
        Vec::new()
    };
    // Synthetic isolated prompts only; headers and credentials are not captured.
    fs::write(
        context.output.join("recovery-requests.json"),
        serde_json::to_vec_pretty(&requests)?,
    )?;
    let gui_errors = logs
        .iter()
        .filter(|(source, _)| {
            source
                .file_name()
                .is_some_and(|name| name.to_string_lossy().ends_with("gui.log"))
        })
        .map(|(source, _)| {
            if source.is_file() {
                count_gui_errors(source)
            } else {
                Ok(0)
            }
        })
        .collect::<Result<Vec<_>>>()?
        .into_iter()
        .sum::<usize>();
    let wire = if native {
        validate_native_requests(&requests)
    } else {
        validate_requests(&requests)
    };
    fs::write(
        context.output.join("collection.json"),
        serde_json::to_vec_pretty(&json!({
            "scenario":ready.scenario,"platform":std::env::consts::OS,"protocol":if native {"responsesHttp"} else {"responsesWebSocket"},
            "guiLifecycles":lifecycles.len(),"plannedGuiLifecycles":2,
            "status":if journey.is_ok() && wire.is_ok() {"complete"} else {"failed"},
            "error":journey.as_ref().err().or(wire.as_ref().err()).map(ToString::to_string),
            "requests":requests.len(),"guiErrors":gui_errors,"humanVerdict":"pending"
        }))?,
    )?;
    journey?;
    wire?;
    ensure!(
        fixture_exit.is_ok_and(|status| status.success()) && gui_errors == 0,
        "fixture shutdown or GUI health incomplete"
    );
    println!(
        "Evidence: {} (human verdict pending)",
        context.output.display()
    );
    Ok(())
}

fn observed_thread(working: &Path) -> Result<String> {
    let observed: Value = serde_json::from_slice(&fs::read(working.join("observed.json"))?)?;
    Ok(observed["threadId"]
        .as_str()
        .context("observed Thread id missing")?
        .to_owned())
}

fn read_history(home: &Path, thread_id: &str) -> Result<Value> {
    let path = history_lock_database(home)?;
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?
        .block_on(async {
            let mut connection = SqliteConnection::connect_with(
                &SqliteConnectOptions::new().filename(path).read_only(true),
            )
            .await?;
            let owner: String =
                sea_orm::sqlx::query("SELECT thread_id FROM history_meta WHERE id=1")
                    .fetch_one(&mut connection)
                    .await?
                    .try_get("thread_id")?;
            ensure!(
                owner == thread_id,
                "history owner differs from observed Thread"
            );
            let mut result = serde_json::Map::new();
            for (key, sql) in [
                (
                    "items",
                    "SELECT payload FROM history_items ORDER BY ordinal",
                ),
                (
                    "context",
                    "SELECT payload FROM current_context ORDER BY ordinal",
                ),
            ] {
                let rows = sea_orm::sqlx::query(sql).fetch_all(&mut connection).await?;
                let values = rows
                    .into_iter()
                    .map(|row| -> Result<Value> {
                        let payload: String = row.try_get("payload")?;
                        Ok(serde_json::from_str(&payload)?)
                    })
                    .collect::<Result<Vec<_>>>()?;
                result.insert(key.to_owned(), json!(values));
            }
            connection.close().await?;
            Ok(Value::Object(result))
        })
}

fn validate_history(history: &Value, native: bool) -> Result<()> {
    let items = history["items"]
        .as_array()
        .context("canonical history items missing")?;
    let mut ids = std::collections::BTreeSet::new();
    for item in items {
        ensure!(
            ids.insert(item["id"].as_str().context("history item id missing")?),
            "duplicate history identity"
        );
    }
    if native {
        for body in ["Lifecycle seed answer", "Lifecycle next answer"] {
            ensure!(
                items
                    .iter()
                    .filter(|item| item["state"]["data"]["text"] == body)
                    .count()
                    == 1,
                "native history lost or duplicated {body}"
            );
        }
        ensure!(
            history["context"]
                .to_string()
                .contains("pl.model.compaction"),
            "native checkpoint lost"
        );
        return Ok(());
    }
    for (body, lifecycle) in [
        ("WS abandoned fragment", "failed"),
        ("WS cancelled fragment", "failed"),
        ("WS recovered answer", "completed"),
        ("WS next turn answer", "completed"),
    ] {
        let matching = items
            .iter()
            .filter(|item| item["state"]["kind"] == "text" && item["state"]["data"]["text"] == body)
            .collect::<Vec<_>>();
        ensure!(
            matching.len() == 1 && matching[0]["state"]["data"]["lifecycle"]["kind"] == lifecycle,
            "{body} lost its independent {lifecycle} lifecycle"
        );
    }
    let mut assistant_frames = 0;
    for record in history["context"]
        .as_array()
        .context("saved context missing")?
    {
        if record["source"]["kind"] != "assistant" {
            continue;
        }
        for part in record["content"]
            .as_array()
            .context("assistant content missing")?
        {
            if part["payload"]["format"] != "pl.model.assistant" {
                continue;
            }
            let frame: Value = serde_json::from_str(
                part["payload"]["content"]
                    .as_str()
                    .context("assistant frame missing")?,
            )?;
            let response = &frame["receipt"]["response"];
            ensure!(
                !response["replay"].is_null(),
                "successful assistant lost native replay"
            );
            for field in ["content", "replay", "presentationItems", "toolCalls"] {
                let text = response[field].to_string();
                ensure!(
                    !text.contains("WS abandoned fragment")
                        && !text.contains("WS cancelled fragment"),
                    "failed observation contaminated canonical {field}"
                );
            }
            assistant_frames += 1;
        }
    }
    ensure!(
        assistant_frames == 2,
        "expected exactly two successful assistant frames, got {assistant_frames}"
    );
    Ok(())
}

fn validate_native_requests(requests: &[Value]) -> Result<()> {
    ensure!(
        requests
            .iter()
            .all(|r| r["accepted"] == true && r["method"] == "POST"),
        "native request rejected"
    );
    let compact = requests
        .iter()
        .filter(|r| {
            r["body"]["input"]
                .as_array()
                .and_then(|items| items.last())
                .is_some_and(|item| item["type"] == "compaction_trigger")
        })
        .collect::<Vec<_>>();
    ensure!(
        compact.len() == 8,
        "expected six failed and two successful compactions, got {}",
        compact.len()
    );
    ensure!(
        compact[..7]
            .iter()
            .all(|r| r["body"]["input"] == compact[0]["body"]["input"]),
        "failed compaction or next input changed frozen context"
    );
    ensure!(
        !compact[..7]
            .iter()
            .any(|r| r["body"]["input"].to_string().contains("Lifecycle failed")),
        "preparation failure admitted input before successful preparation"
    );
    let admitted = requests
        .iter()
        .filter(|r| {
            r["body"]["input"].as_array().is_some_and(|items| {
                items.last().is_some_and(|item| {
                    item["role"] == "user"
                        && item["content"].as_array().is_some_and(|content| {
                            content
                                .iter()
                                .filter(|part| part["text"] == "Lifecycle failed")
                                .count()
                                == 1
                                && content
                                    .iter()
                                    .filter(|part| part["text"] == "Lifecycle wait")
                                    .count()
                                    == 1
                        })
                })
            })
        })
        .count();
    ensure!(
        admitted == 1,
        "queued input was lost or admitted more than once"
    );
    Ok(())
}

fn validate_requests(requests: &[Value]) -> Result<()> {
    ensure!(
        !requests.is_empty()
            && requests.iter().all(|request| request["accepted"] == true
                && request["method"] == "WS"
                && request["path"] == "/v1/responses"),
        "request rejected or not Responses WebSocket"
    );
    // Match the last user entry: earlier prompts in a full history do not identify new work.
    let user_requests = |prompt: &str| {
        requests
            .iter()
            .filter(|request| {
                request["body"]["input"]
                    .as_array()
                    .and_then(|input| input.iter().rev().find(|item| item["role"] == "user"))
                    .and_then(|item| {
                        item["content"]
                            .as_str()
                            .or_else(|| item["content"].as_array()?.first()?["text"].as_str())
                    })
                    == Some(prompt)
            })
            .collect::<Vec<_>>()
    };
    let recover = user_requests("WebSocket recover");
    ensure!(
        recover.len() == 2
            && recover[0]["body"]["input"] == recover[1]["body"]["input"]
            && recover[1]["body"]["previous_response_id"].is_null(),
        "recovery did not resend frozen full input"
    );
    let cancel = user_requests("WebSocket cancel");
    let next = user_requests("WebSocket next");
    ensure!(
        next.len() == 1 && cancel.len() == 1,
        "cancel or next Turn issued extra requests"
    );
    for request in requests {
        let input = request["body"]["input"].to_string();
        ensure!(
            !input.contains("WS abandoned fragment")
                && !input.contains("WS cancelled fragment")
                && !input.contains("连接中断，正在重试"),
            "failed body or recovery notice entered model input"
        );
    }
    Ok(())
}
