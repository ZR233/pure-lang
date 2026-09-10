//! Assertions over real durable actor timelines, never simulated model verdicts.
use super::{Actor, Call, WorkspaceKind};
use anyhow::{Context, Result, ensure};
use serde_json::Value;
use std::{collections::BTreeMap, fs, path::Path};

fn field<'a>(value: &'a Value, key: &str) -> Result<&'a str> {
    value
        .get(key)
        .and_then(Value::as_str)
        .with_context(|| format!("missing {key}: {value}"))
}

fn output(call: &Call) -> Result<Value> {
    serde_json::from_str(&call.output).with_context(|| format!("{} has no JSON receipt", call.name))
}

fn root<'a>(actors: &'a BTreeMap<String, Actor>, id: &str) -> Result<&'a Actor> {
    actors.get(id).context("root timeline missing")
}

fn successful_spawn(call: &Call) -> bool {
    call.name == "spawn_agent"
        && output(call)
            .ok()
            .is_some_and(|receipt| receipt.get("agentId").and_then(Value::as_str).is_some())
}

pub(super) fn validate_checkpoint(actors: &BTreeMap<String, Actor>, root_id: &str) -> Result<()> {
    let root = root(actors, root_id)?;
    ensure!(
        !root.calls.iter().any(|call| successful_spawn(call)
            && call.arguments.get("profileId").and_then(Value::as_str) == Some("reviewer")),
        "reviewer started before injection checkpoint"
    );
    let executors = root
        .calls
        .iter()
        .filter(|call| {
            successful_spawn(call)
                && matches!(
                    call.arguments.get("profileId").and_then(Value::as_str),
                    Some("executor" | "worktree_executor")
                )
        })
        .collect::<Vec<_>>();
    ensure!(
        !executors.is_empty(),
        "no implementation owner before checkpoint"
    );
    for spawn in executors {
        let receipt = output(spawn)?;
        let id = field(&receipt, "agentId")?;
        let actor = actors.get(id).context("executor timeline missing")?;
        let latest_turn_id = actor
            .calls
            .last()
            .context("executor has no tool evidence")?
            .turn_id
            .as_str();
        let last_dispatch = root
            .calls
            .iter()
            .enumerate()
            .filter(|(_, call)| {
                (call.name == "send_message"
                    && call.arguments.get("target").and_then(Value::as_str) == Some(id))
                    || (call.name == "spawn_agent"
                        && output(call).ok().is_some_and(|receipt| {
                            receipt.get("agentId").and_then(Value::as_str) == Some(id)
                        }))
            })
            .map(|(index, _)| index)
            .max()
            .context("executor has no dispatch receipt")?;
        ensure!(
            actor.snapshot.active_turn.is_none()
                && actor.snapshot.thread.status == pl_protocol::ThreadStatus::Idle
                && root.calls[last_dispatch + 1..]
                    .iter()
                    .any(|call| { consumed_completion_before(root, id, latest_turn_id, call) }),
            "cannot inject without receipt-bound executor completion: {id}"
        );
        ensure!(
            actor.calls.iter().any(|call| call.name == "report_progress"
                && call.arguments.to_string().contains("CHILD_DELIVERY_READY")),
            "executor {id} has no initial delivery"
        );
        ensure!(
            root.calls
                .iter()
                .any(|call| call.name == "read_agent_submissions"
                    && call.arguments.get("target").and_then(Value::as_str) == Some(id)),
            "initial delivery was not consumed"
        );
        ensure!(
            !root.calls.iter().any(|call| call.name == "close_agent"
                && call.arguments.get("target").and_then(Value::as_str) == Some(id)),
            "executor closed before review checkpoint"
        );
    }
    Ok(())
}

pub(super) fn validate(
    actors: &BTreeMap<String, Actor>,
    root_id: &str,
    kind: WorkspaceKind,
    artifacts: &Path,
) -> Result<()> {
    for actor in actors.values().filter(|actor| actor.role == "reviewer") {
        for call in &actor.calls {
            ensure!(
                matches!(
                    call.name.as_str(),
                    "read_file"
                        | "list_files"
                        | "stat_path"
                        | "lsp_capabilities"
                        | "lsp_query"
                        | "git_status"
                        | "git_diff"
                        | "git_workspace_info"
                        | "read_session_note"
                        | "search_session_note"
                        | "report_progress"
                ),
                "reviewer {} used a tool outside its read-only boundary: {}",
                actor.id,
                call.name
            );
        }
    }
    let root = root(actors, root_id)?;
    ensure!(
        root.snapshot
            .runtime
            .as_ref()
            .and_then(|runtime| runtime.workflow.as_ref())
            .and_then(|workflow| workflow.current_run.as_ref())
            .is_some_and(|run| run.current_state_id == "completed"),
        "rework workflow did not reach completed"
    );
    let reviewer_spawns = root
        .calls
        .iter()
        .enumerate()
        .filter(|(_, call)| {
            successful_spawn(call)
                && call.arguments.get("profileId").and_then(Value::as_str) == Some("reviewer")
        })
        .collect::<Vec<_>>();
    ensure!(
        reviewer_spawns.len() >= 2,
        "real finding did not lead to a fresh reviewer"
    );
    let finding_index = root
        .calls
        .iter()
        .position(|call| {
            call.name == "read_agent_submissions" && submission_has_marker(call, "REVIEWER_FINDING")
        })
        .context("root never read durable finding")?;
    let first_id = field(&root.calls[finding_index].arguments, "target")?;
    let initial_reviewer = actors
        .get(first_id)
        .context("finding reviewer timeline missing")?;
    ensure!(
        initial_reviewer.role == "reviewer"
            && initial_reviewer
                .calls
                .iter()
                .any(|call| call.name == "report_progress"
                    && call
                        .arguments
                        .get("summary")
                        .and_then(Value::as_str)
                        .is_some_and(|text| text.starts_with("REVIEWER_FINDING"))),
        "real reviewer did not publish the finding"
    );
    let (repair_index, repair) = root
        .calls
        .iter()
        .enumerate()
        .find(|(i, call)| {
            *i > finding_index
                && call.name == "send_message"
                && call
                    .arguments
                    .get("target")
                    .and_then(Value::as_str)
                    .and_then(|id| actors.get(id))
                    .is_some_and(|actor| {
                        actor.calls.iter().any(|action| {
                            action.completed_at < call.completed_at
                                && matches!(
                                    action.name.as_str(),
                                    "write_file" | "edit_file" | "apply_patch" | "exec"
                                )
                                && action.arguments.to_string().contains("normalize.rs")
                        })
                    })
        })
        .context("finding did not resume the normalization executor")?;
    let repair_receipt = output(repair)?;
    let owner = field(&repair_receipt, "target")?;

    let owner_spawn = root.calls[..reviewer_spawns[0].0]
        .iter()
        .find(|call| {
            successful_spawn(call)
                && output(call).ok().is_some_and(|value| {
                    value.get("agentId").and_then(Value::as_str) == Some(owner)
                })
        })
        .context("repair was assigned to a new executor")?;
    let profile = field(&owner_spawn.arguments, "profileId")?;
    let expected_profile = match kind {
        WorkspaceKind::Directory => "executor",
        WorkspaceKind::Worktree => "worktree_executor",
    };
    ensure!(
        profile == expected_profile,
        "wrong owner isolation: {profile}"
    );
    let executor = actors
        .get(owner)
        .context("original owner timeline missing")?;
    let turn = resumed_message_turn(executor, &repair_receipt)?;
    // The normalization implementation and injected regression belong to this same owner.
    ensure!(
        executor.calls.iter().any(|call| call.turn_id != turn
            && call.arguments.to_string().contains("normalize.rs")
            && matches!(
                call.name.as_str(),
                "write_file" | "edit_file" | "apply_patch" | "exec"
            )),
        "resumed executor did not own the defective implementation"
    );
    let delivery = executor
        .calls
        .iter()
        .find(|call| {
            call.turn_id == turn
                && call.name == "report_progress"
                && call.arguments.to_string().contains("CHILD_DELIVERY_READY")
        })
        .context("resumed turn has no new durable delivery")?;
    let delivery_receipt = output(delivery)?;
    let delivery_revision = delivery_receipt.get("revision").and_then(Value::as_u64);
    let (read_index, read) = root
        .calls
        .iter()
        .enumerate()
        .find(|(i, call)| {
            *i > repair_index
                && call.name == "read_agent_submissions"
                && call.arguments.get("target").and_then(Value::as_str) == Some(owner)
        })
        .context("root never read repaired delivery")?;
    let page = output(read)?;
    let summary = field(&delivery.arguments, "summary")?;
    ensure!(
        page.get("items")
            .and_then(Value::as_array)
            .is_some_and(|items| items.iter().any(|item| {
                item.get("summary").and_then(Value::as_str) == Some(summary)
                    && delivery_revision.is_none_or(|revision| {
                        item.get("revision").and_then(Value::as_u64) == Some(revision)
                    })
            })),
        "root consumed an old submission instead of resumed delivery"
    );
    ensure!(
        consumed_completion_before(root, owner, turn, read),
        "root lacks turn-bound terminal receipt before repaired delivery read"
    );
    let last_spawn = reviewer_spawns.last().context("final reviewer missing")?;
    let last_receipt = output(last_spawn.1)?;
    let final_id = field(&last_receipt, "agentId")?;
    ensure!(
        first_id != final_id && last_spawn.0 > read_index,
        "repair was not followed by fresh-context review"
    );
    let mut approval_index = 0;
    // Every reviewer created after the last repair belongs to the final wave.
    let last_repair_index = root
        .calls
        .iter()
        .enumerate()
        .filter(|(_, call)| {
            call.name == "send_message"
                && call
                    .arguments
                    .get("target")
                    .and_then(Value::as_str)
                    .and_then(|id| actors.get(id))
                    .is_some_and(|actor| {
                        matches!(actor.role.as_str(), "executor" | "worktree_executor")
                    })
        })
        .map(|(index, _)| index)
        .max()
        .context("repair message missing")?;
    for (spawn_index, spawn) in reviewer_spawns
        .iter()
        .filter(|(index, _)| *index > last_repair_index)
    {
        let receipt = output(spawn)?;
        let id = field(&receipt, "agentId")?;
        let approval = root
            .calls
            .iter()
            .enumerate()
            .find(|(index, call)| {
                *index > *spawn_index
                    && call.name == "read_agent_submissions"
                    && call.arguments.get("target").and_then(Value::as_str) == Some(id)
                    && submission_has_marker(call, "REVIEWER_READ_ONLY_APPROVED")
            })
            .map(|(index, _)| index)
            .context("a final-wave reviewer lacks durable approval")?;
        ensure!(
            !root.calls[*spawn_index + 1..]
                .iter()
                .any(|call| call.name == "read_agent_submissions"
                    && call.arguments.get("target").and_then(Value::as_str) == Some(id)
                    && submission_has_marker(call, "REVIEWER_FINDING")),
            "unresolved final-wave finding"
        );
        approval_index = approval_index.max(approval);
    }
    ensure!(
        approval_index > last_repair_index,
        "final review wave missing"
    );
    // Integration is verified against the final workspace, not a prescribed Git command.
    let integration_index = read_index;
    // Review is read-only. Successful checks of this final integrated tree may
    // precede approval; another identical execution is not a completion requirement.
    let final_checks = root.calls[integration_index + 1..]
        .iter()
        .filter(|call| is_test(call))
        .filter_map(|call| {
            terminal_test_call(root, call)
                .ok()
                .map(|terminal| (call, terminal))
        })
        .filter(|(_, terminal)| {
            output(terminal).ok().is_some_and(|value| {
                value
                    .pointer("/state/data/result/kind")
                    .and_then(Value::as_str)
                    == Some("succeeded")
            })
        })
        .collect::<Vec<_>>();
    ensure!(
        final_checks.iter().any(|(call, _)| is_full_test(call)),
        "final integrated tree lacks successful tests evidence"
    );
    if matches!(kind, WorkspaceKind::Worktree) {
        let (cleanup_index, cleanup) = root
            .calls
            .iter()
            .enumerate()
            .find(|(_, call)| {
                call.name == "close_agent"
                    && call.arguments.get("target").and_then(Value::as_str) == Some(owner)
                    && call
                        .arguments
                        .get("workspaceDisposition")
                        .and_then(Value::as_str)
                        == Some("cleanup")
            })
            .context("retained worktree was never cleaned")?;
        ensure!(
            cleanup_index > approval_index
                && output(cleanup)?.get("lifecycle").and_then(Value::as_str) == Some("closed")
                && output(cleanup)?
                    .get("workspaceDisposition")
                    .and_then(Value::as_str)
                    == Some("cleanup")
                && final_checks
                    .iter()
                    .all(
                        |(_, terminal)| terminal.completed_at <= cleanup.completed_at
                            && root.calls[..cleanup_index]
                                .iter()
                                .any(|call| call.id == terminal.id)
                    ),
            "cleanup preceded final approval or terminal verification evidence"
        );
    }
    ensure!(
        executor.calls.iter().any(|call| call.turn_id == turn
            && covers_normalization(call)
            && terminal_test_call(executor, call)
                .ok()
                .is_some_and(|terminal| output(terminal).ok().is_some_and(|value| value
                    .pointer("/state/data/result/kind")
                    .and_then(Value::as_str)
                    == Some("succeeded")))),
        "repair reused pre-injection evidence without successfully testing changed normalization"
    );
    write_verification_report(actors, artifacts)?;
    fs::write(
        artifacts.join("rework-receipt.json"),
        serde_json::to_vec_pretty(&serde_json::json!({
            "originalExecutorAgentId":owner, "repairTurnId":turn, "firstReviewerAgentId":first_id,
            "finalReviewerAgentId":final_id, "sameExecutor":true, "freshReviewer":true,
            "repairMessage":repair.arguments, "delivery":delivery.arguments,
            "cacheUsageSource":"cache-usage.json contains raw provider counters; missing counters are not observable"
        }))?,
    )?;
    Ok(())
}

fn submission_has_marker(call: &Call, marker: &str) -> bool {
    output(call)
        .ok()
        .and_then(|value| value.get("items").and_then(Value::as_array).cloned())
        .is_some_and(|items| {
            items.iter().any(|item| {
                ["summary", "detail"].iter().any(|key| {
                    item.get(key)
                        .and_then(Value::as_str)
                        .is_some_and(|text| text.trim_start().starts_with(marker))
                })
            })
        })
}

fn command_words(call: &Call) -> Option<Vec<&str>> {
    if call.name != "exec" {
        return None;
    }
    let command = call.arguments.get("command")?.as_str()?;
    if command.contains([';', '|', '&', '\n', '>', '<']) {
        return None;
    }
    let words = command.split_whitespace().collect::<Vec<_>>();
    matches!(words.first().copied(), Some("cargo" | "cargo.exe")).then_some(words)
}

fn is_test(call: &Call) -> bool {
    command_words(call).is_some_and(|words| words.get(1) == Some(&"test")) || is_verifier(call)
}

fn is_full_test(call: &Call) -> bool {
    command_words(call).is_some_and(|words| {
        words.get(1) == Some(&"test")
            && words[2..].iter().all(|word| {
                matches!(
                    *word,
                    "--workspace" | "--all-targets" | "--quiet" | "-q" | "--locked" | "--offline"
                )
            })
    })
}

fn is_verifier(call: &Call) -> bool {
    command_words(call).is_some_and(|words| {
        words.get(1) == Some(&"run")
            && words
                .windows(2)
                .any(|pair| pair == ["--bin", "fixture_verify"])
            && words[2..].iter().all(|word| {
                matches!(
                    *word,
                    "--bin" | "fixture_verify" | "--quiet" | "-q" | "--locked" | "--offline"
                )
            })
    })
}

fn covers_normalization(call: &Call) -> bool {
    is_test(call)
        && call
            .arguments
            .get("command")
            .and_then(Value::as_str)
            .is_some_and(|command| {
                command.contains("normalize")
                    || command.contains("review_checkpoint_separator_regression")
                    || matches!(
                        command.trim(),
                        "cargo test" | "cargo test --workspace" | "cargo test --all-targets"
                    )
            })
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct ChildCompletion {
    child_id: String,
    turn: Option<pl_core::thread::TurnRecord>,
}

fn resumed_message_turn<'a>(executor: &'a Actor, receipt: &Value) -> Result<&'a str> {
    ensure!(
        field(receipt, "target")? == executor.id,
        "repair receipt targets another executor"
    );
    let message_id = field(receipt, "messageId")?;
    let sequence = receipt
        .get("sequence")
        .and_then(Value::as_u64)
        .context("repair receipt has no inbox sequence")?;
    let mut admitted = false;
    let mut consumed = 0;
    for commit in &executor.journal {
        for record in commit.inbox.iter() {
            if record.sequence == sequence && record.message.id == message_id {
                admitted = true;
            }
        }
        if let Some(watermark) = commit.consumed_messages {
            if consumed < sequence && sequence <= watermark {
                ensure!(
                    admitted,
                    "repair consumption has no matching admitted message identity"
                );
                let records = match &commit.context {
                    Some(pl_core::thread::journal::ContextChange::Append { records, .. }) => {
                        records
                    }
                    Some(pl_core::thread::journal::ContextChange::Replace(snapshot)) => {
                        &snapshot.records
                    }
                    None => anyhow::bail!("repair consumption has no committed context binding"),
                };
                let record = records
                    .iter()
                    .find(|record| record.id == format!("inbox:{sequence}"))
                    .context("repair consumption did not bind the received inbox record")?;
                return record
                    .turn_id
                    .as_deref()
                    .context("repair inbox context has no Turn identity");
            }
            consumed = watermark;
        }
    }
    anyhow::bail!("repair message was not consumed by an executor Turn")
}

fn consumed_completion_before(root: &Actor, child: &str, turn: &str, call: &Call) -> bool {
    let mut consumed = 0;
    let mut notification = None;
    for commit in &root.journal {
        for record in commit.inbox.iter() {
            if record.message.payload.format() != "pl.studio.child-notification"
                || record.message.payload.version() != 1
                || record.message.source_id != format!("studio.child:{child}")
            {
                continue;
            }
            if let Ok(message) =
                serde_json::from_str::<ChildCompletion>(record.message.payload.content())
                && message.child_id == child
                && message.turn.as_ref().is_some_and(|value| {
                    value.turn_id == turn
                        && value.state
                            == pl_core::thread::TurnState::Finished(
                                pl_core::thread::TurnOutcome::Completed,
                            )
                })
            {
                notification = Some(record.sequence);
            }
        }
        if let Some(watermark) = commit.consumed_messages {
            consumed = watermark;
        }
        if let Some(attempt) = &commit.attempt
            && let pl_core::thread::AttemptOutcome::Committed(output) = &attempt.outcome
            && output.tool_calls.iter().any(|tool| tool.call_id == call.id)
        {
            return notification.is_some_and(|sequence| sequence <= consumed);
        }
    }
    false
}

fn terminal_test_call<'a>(_actor: &'a Actor, call: &'a Call) -> Result<&'a Call> {
    let receipt = output(call)?;
    ensure!(
        receipt.pointer("/state/kind").and_then(Value::as_str) == Some("final"),
        "test command has no terminal result for its own call identity"
    );
    Ok(call)
}

fn verification_cwd(
    actors: &BTreeMap<String, Actor>,
    actor: &Actor,
    call: &Call,
) -> Result<String> {
    let cwd = call
        .arguments
        .get("cwd")
        .and_then(Value::as_str)
        .unwrap_or(".");
    let path = std::path::Path::new(cwd);
    if path.is_absolute() {
        return Ok(path
            .components()
            .collect::<std::path::PathBuf>()
            .display()
            .to_string());
    }
    let workspace = actors
        .values()
        .flat_map(|actor| &actor.calls)
        .filter(|call| call.name == "spawn_agent")
        .filter_map(|call| output(call).ok())
        .find_map(|receipt| {
            if receipt.get("agentId").and_then(Value::as_str) == Some(&actor.id) {
                receipt
                    .pointer("/workspace/root")
                    .and_then(Value::as_str)
                    .map(str::to_string)
            } else if actor.role == "root" {
                receipt
                    .pointer("/workspace/projectRoot")
                    .and_then(Value::as_str)
                    .map(str::to_string)
            } else {
                None
            }
        })
        .or_else(|| {
            actors.values().flat_map(|candidate| candidate.journal.iter()).flat_map(|commit| commit.extensions.iter())
                .find_map(|change| {
                    let pl_core::thread::extensions::ExtensionChange::Put { id, record } = change else { return None; };
                    if id != "studio.workspace" || record.payload.format() != "pl.studio.workspace" || record.payload.version() != 1 { return None; }
                    let assignment: pl_protocol::AgentWorkspaceAssignmentSnapshot = serde_json::from_str(record.payload.content()).ok()?;
                    if actor.role == "root" { Some(assignment.project_root) }
                    else if actor.journal.iter().any(|commit| commit.extensions.iter().any(|entry| matches!(entry, pl_core::thread::extensions::ExtensionChange::Put { record: own, .. } if own.payload == record.payload))) { Some(assignment.root) }
                    else { None }
                })
        })
        .context("verification has no bound workspace assignment")?;
    Ok(std::path::Path::new(&workspace)
        .join(path)
        .components()
        .collect::<std::path::PathBuf>()
        .display()
        .to_string())
}

pub(super) fn write_verification_report(
    actors: &BTreeMap<String, Actor>,
    artifacts: &Path,
) -> Result<()> {
    let mut report = String::from(
        "# Verification evidence for delivery review\n\nRaw invocations and submissions follow. Assess report accuracy and reuse against these facts; wording is not an automated verdict. Harness checks are separate.\n",
    );
    for actor in actors.values() {
        for call in &actor.calls {
            if matches!(call.name.as_str(), "report_progress" | "complete") {
                report.push_str(&format!(
                    "\n## {} / {} / {}\n\n{}\n",
                    actor.role, actor.id, call.turn_id, call.arguments
                ));
            }
            if is_test(call) {
                let terminal = terminal_test_call(actor, call)?;
                report.push_str(&format!(
                    "\nActual call {} / {}\ncwd: {}\n\n```json\n{}\n```\n```text\n{}\n```\n",
                    actor.id,
                    call.turn_id,
                    verification_cwd(actors, actor, call)?,
                    call.arguments,
                    terminal.output
                ));
            }
        }
    }
    fs::write(artifacts.join("verification-report.md"), report)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn actor(id: &str, time: i64, detail: &str) -> Actor {
        Actor {
            journal: Vec::new(),
            id: id.into(),
            role: "executor".into(),
            snapshot: pl_protocol::ThreadSnapshot::empty(id),
            calls: vec![
                Call {
                    id: format!("{id}-test"),
                    turn_id: id.into(),
                    completed_at: time,
                    name: "exec".into(),
                    arguments: serde_json::json!({"command":"cargo test --test normalize", "cwd":"/fixture"}),
                    output: serde_json::json!({"state":{"kind":"final","data":{"result":{"kind":"succeeded"}}},"stdout":"1 test passed"}).to_string(),
                },
                Call {
                    id: format!("{id}-delivery"),
                    turn_id: id.into(),
                    completed_at: time + 1,
                    name: "report_progress".into(),
                    arguments: serde_json::json!({"summary":"CHILD_DELIVERY_READY", "detail":detail}),
                    output: "{}".into(),
                },
            ],
        }
    }

    fn completion_journal(
        child: &str,
        turn: &str,
        call: &str,
        consumed: bool,
    ) -> Vec<pl_core::thread::journal::ThreadCommit> {
        let notification = serde_json::json!({"childId":child,"turn":{"turnId":turn,"inputId":null,"modelSteps":1,"elapsedMs":1,"state":{"kind":"finished","value":"completed"}}});
        vec![serde_json::from_value(serde_json::json!({
            "threadId":"root","sequence":1,"committedAt":1,
            "deliveries":[],"extensions":[],"interactions":[],"replacements":[],
            "inbox":[{"sequence":1,"message":{"id":"notification","sourceId":format!("studio.child:{child}"),"payload":{"format":"pl.studio.child-notification","version":1,"content":notification.to_string()},"context":[]}}],
            "consumedMessages":if consumed {1} else {0},
            "attempt":{"turnId":"root-turn","attemptId":"attempt","inputRevision":0,"tools":[],"outcome":{"kind":"committed","value":{"attemptId":"attempt","baseContextRevision":0,"content":[],"toolCalls":[{"callId":call,"toolId":"read_agent_submissions","arguments":{"format":"text/plain","version":1,"content":"request"}}],"privateContext":null,"usage":{}}}}
        })).unwrap()]
    }

    #[test]
    fn repair_turn_comes_from_consumed_message_identity_and_sequence() {
        let mut executor = actor("executor", 1, "delivery");
        let mut journal = completion_journal("child", "old", "call", false);
        let mut commit = serde_json::to_value(&journal[0]).unwrap();
        commit["threadId"] = serde_json::json!("executor");
        commit["inbox"][0]["sequence"] = serde_json::json!(7);
        commit["inbox"][0]["message"]["id"] = serde_json::json!("repair-message");
        commit["consumedMessages"] = serde_json::json!(7);
        commit["context"] = serde_json::json!({"kind":"append","value":{"revision":1,"records":[{"id":"inbox:7","turnId":"resumed-turn","source":{"kind":"runtime","sourceId":"agent:root"},"content":[],"toolCalls":[]}]}});
        journal[0] = serde_json::from_value(commit).unwrap();
        executor.journal = journal;
        let receipt =
            serde_json::json!({"target":"executor","messageId":"repair-message","sequence":7});
        assert_eq!(
            resumed_message_turn(&executor, &receipt).unwrap(),
            "resumed-turn"
        );
        let mut wrong = receipt.clone();
        wrong["messageId"] = serde_json::json!("other-message");
        assert!(resumed_message_turn(&executor, &wrong).is_err());
        wrong = receipt.clone();
        wrong["sequence"] = serde_json::json!(6);
        assert!(resumed_message_turn(&executor, &wrong).is_err());
        executor.journal[0].consumed_messages = Some(6);
        assert!(resumed_message_turn(&executor, &receipt).is_err());
    }

    #[test]
    fn parent_completion_evidence_rejects_unconsumed_and_wrong_call_notifications() {
        let mut root = actor("root", 1, "");
        let call = root.calls[0].clone();
        root.journal = completion_journal("child", "turn", &call.id, false);
        assert!(!consumed_completion_before(&root, "child", "turn", &call));
        root.journal = completion_journal("child", "turn", &call.id, true);
        assert!(consumed_completion_before(&root, "child", "turn", &call));
        assert!(!consumed_completion_before(
            &root,
            "child",
            "other-turn",
            &call
        ));
        assert!(!consumed_completion_before(
            &root,
            "other-child",
            "turn",
            &call
        ));
    }

    const REPORT: &str = "Executed: cargo test --test normalize; cwd=/fixture; baseline=abc; scope=normalize; environment=Linux; result=passed; evidence=tool result. Reused: none. Not run: full suite (owned by root).";

    #[test]
    fn checkpoint_uses_native_terminal_receipts_without_synthetic_turn_items() {
        let owner = actor("a", 1, REPORT);
        let call = |name: &str, arguments: Value, output: Value| Call {
            id: name.into(),
            turn_id: "root-turn".into(),
            completed_at: 3,
            name: name.into(),
            arguments,
            output: output.to_string(),
        };
        let root = Actor {
            journal: completion_journal("a", "a", "read_agent_submissions", true),
            id: "root".into(),
            role: "root".into(),
            snapshot: pl_protocol::ThreadSnapshot::empty("root"),
            calls: vec![
                call(
                    "spawn_agent",
                    serde_json::json!({"profileId":"executor"}),
                    serde_json::json!({"agentId":"a"}),
                ),
                call(
                    "wait",
                    serde_json::json!({}),
                    serde_json::json!({"tasks":[],"messagesReady":true,"timedOut":false}),
                ),
                call(
                    "read_agent_submissions",
                    serde_json::json!({"target":"a"}),
                    serde_json::json!({"items":[{"summary":"CHILD_DELIVERY_READY"}]}),
                ),
            ],
        };
        let mut actors = BTreeMap::from([("a".into(), owner), ("root".into(), root)]);
        let mut failed = actors["root"].calls[0].clone();
        failed.output = "TOOL_FAILED: invalid writablePaths".into();
        actors.get_mut("root").unwrap().calls.push(failed);
        validate_checkpoint(&actors, "root").unwrap();
        actors.get_mut("root").unwrap().journal =
            completion_journal("a", "old-turn", "read_agent_submissions", true);
        assert!(
            validate_checkpoint(&actors, "root").is_err(),
            "stale turn receipt accepted"
        );
    }

    #[test]
    fn verification_commands_reject_echoes_filters_and_hidden_failures() {
        let mut actor = actor("a", 1, REPORT);
        for command in [
            "echo cargo test",
            "cargo test; true",
            "cargo test --lib",
            "cargo test --test normalize",
            "cargo test --help",
        ] {
            actor.calls[0].arguments["command"] = serde_json::json!(command);
            assert!(!is_full_test(&actor.calls[0]), "accepted {command}");
        }
        for command in [
            "echo cargo run --bin fixture_verify",
            "cargo run --bin fixture_verify --help",
        ] {
            actor.calls[0].arguments["command"] = serde_json::json!(command);
            assert!(!is_verifier(&actor.calls[0]), "accepted {command}");
        }
        actor.calls[0].arguments["command"] =
            serde_json::json!("cargo run --quiet --bin fixture_verify");
        assert!(is_verifier(&actor.calls[0]));
        actor.calls[0].arguments["command"] = serde_json::json!("cargo test");
        assert!(is_full_test(&actor.calls[0]));
    }

    #[test]
    fn unrelated_test_does_not_validate_changed_normalization() {
        let mut actor = actor("a", 1, REPORT);
        assert!(covers_normalization(&actor.calls[0]));
        actor.calls[0].arguments["command"] = serde_json::json!("cargo test --test validate");
        assert!(!covers_normalization(&actor.calls[0]));
    }

    #[test]
    fn running_test_requires_its_own_terminal_delivery_and_keeps_nonzero_status() {
        let mut actor = actor("a", 1, REPORT);
        actor.calls[0].output =
            serde_json::json!({"state":{"kind":"running"},"processId":"task-a"}).to_string();
        assert!(terminal_test_call(&actor, &actor.calls[0]).is_err());
        let mut unrelated = actor.calls[0].clone();
        unrelated.id = "other-call".into();
        unrelated.output =
            serde_json::json!({"state":{"kind":"final","data":{"result":{"kind":"succeeded"}}}})
                .to_string();
        actor.calls.push(unrelated);
        assert!(terminal_test_call(&actor, &actor.calls[0]).is_err());
        actor.calls[0].output = serde_json::json!({"state":{"kind":"final","data":{"result":{"kind":"failed","data":{"failure":{"kind":"exited","data":{"exit_code":101}}}}}}}).to_string();
        let terminal = terminal_test_call(&actor, &actor.calls[0]).unwrap();
        assert_eq!(
            output(terminal)
                .unwrap()
                .pointer("/state/data/result/kind")
                .unwrap(),
            "failed"
        );
        assert_eq!(
            output(terminal)
                .unwrap()
                .pointer("/state/data/result/data/failure/data/exit_code")
                .unwrap(),
            101
        );
    }
}
