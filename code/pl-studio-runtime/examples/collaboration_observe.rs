//! Live collection only. A human/agent reads the evidence; no marker or automated acceptance verdict.
use anyhow::{Context, Result};
use pl_studio_runtime::{
    ConfigStore, StudioHostKind, StudioRuntime, StudioRuntimeOptions, ThreadModeId,
};
use std::{
    collections::BTreeSet,
    io::Write,
    path::{Path, PathBuf},
    time::Duration,
};

#[tokio::main]
async fn main() -> Result<()> {
    let artifacts = match std::env::var_os("ANYWORK_WORKFLOW_ARTIFACT_DIR") {
        Some(path) => PathBuf::from(path),
        None => tempfile::Builder::new()
            .prefix("anywork-collaboration-")
            .tempdir()?
            .keep(),
    };
    std::fs::create_dir_all(&artifacts)?;
    let home = artifacts.join("studio-home");
    let workspace = artifacts.join("workspace");
    std::fs::create_dir_all(&home)?;
    std::fs::create_dir_all(&workspace)?;
    let installed = ConfigStore::default_app()?;
    // Copy bytes, never save a hydrated config: the credential store belongs to the user.
    std::fs::copy(installed.paths().config_file(), home.join("config.toml"))?;
    let prompt = match std::env::var_os("ANYWORK_OBSERVATION_PROMPT") {
        Some(path) => std::fs::read_to_string(path)?,
        None => "请派两个 fresh-context explorer 分别解释文件所有权和 Turn 生命周期。详细、自包含地派发。一个自然 final，另一个 finish_turn。等待完整报告，阅读后总结；不修改文件，不提交计划，不询问用户。".into(),
    };
    let seconds = std::env::var("ANYWORK_OBSERVATION_SECONDS")
        .ok()
        .map(|v| v.parse::<u64>())
        .transpose()?
        .unwrap_or(180);
    let runtime = StudioRuntime::with_options(StudioRuntimeOptions {
        studio_home: Some(home),
        host: StudioHostKind::Test,
    })
    .await?;
    runtime.start_runtime().await?;
    let result = async {
        let project = runtime.open_project(&workspace).await?;
        let thread = runtime.create_thread(&project.id, "真实协作过程观察").await?;
        runtime.set_thread_mode(&thread.id, ThreadModeId::new("mode.simple")?).await?;
        let mut observers = Vec::new();
        let mut observed = BTreeSet::new();
        observers.push(observe(&runtime, &thread.id, &artifacts).await?);
        observed.insert(thread.id.clone());
        runtime.submit_prompt_command(thread.id.clone(), pl_protocol::studio::SubmitPromptRequest {
            input: pl_protocol::studio::StudioPromptInput { input_id: format!("observation-{}",std::process::id()), text: prompt, attachment_draft_ids: vec![] },
        }).await?;
        println!("Observing real API execution for {seconds}s. Artifacts: {}. No automatic pass/fail verdict.",artifacts.display());
        let deadline = tokio::time::Instant::now() + Duration::from_secs(seconds);
        while tokio::time::Instant::now() < deadline {
            let snapshot = runtime.thread_snapshot(&thread.id).await?;
            for tool in snapshot.items.iter().filter_map(pl_protocol::ThreadItem::tool) {
                if tool.invocation().name() != "spawn_agent" { continue; }
                if let pl_protocol::ThreadToolState::Succeeded(done) = tool.state() {
                    let receipt: serde_json::Value = serde_json::from_str(done.output().result())?;
                    let id = receipt.get("agentId").and_then(serde_json::Value::as_str).context("spawn receipt lacks agentId")?;
                    if observed.insert(id.to_owned()) { observers.push(observe(&runtime,id,&artifacts).await?); }
                }
            }
            tokio::time::sleep(Duration::from_millis(500)).await;
        }
        for id in &observed {
            let mut cursor = None;
            let mut turns = Vec::new();
            loop {
                let page = runtime.list_thread_turns(id, cursor.as_deref(), 200).await?;
                turns.extend(page.turns);
                cursor = page.next_cursor;
                if cursor.is_none() { break; }
            }
            std::fs::write(artifacts.join(format!("{id}.turns.json")), serde_json::to_vec_pretty(&turns)?)?;
            let snapshot = runtime.thread_snapshot(&id).await?;
            std::fs::write(artifacts.join(format!("{id}.snapshot.json")),serde_json::to_vec_pretty(&snapshot)?)?;
        }
        runtime.shutdown_runtime().await?;
        for observer in observers { observer.await??; }
        std::fs::write(artifacts.join("observation.txt"),"Collection ended. Read the complete event streams, snapshots and wire evidence; no automated acceptance verdict was computed.\n")?;
        Ok(())
    }.await;
    // Preserve errors while still reclaiming processes and flushing journal facts.
    let shutdown = runtime.shutdown_runtime().await;
    match (result, shutdown) {
        (Ok(()), Ok(_)) => Ok(()),
        (Err(error), _) => Err(error),
        (Ok(()), Err(error)) => Err(error.into()),
    }
}

async fn observe(
    runtime: &StudioRuntime,
    id: &str,
    artifacts: &Path,
) -> Result<tokio::task::JoinHandle<Result<()>>> {
    let mut stream = runtime
        .subscribe_thread(pl_protocol::ThreadSubscriptionRequest {
            thread_id: id.into(),
        })
        .await?;
    let mut file = std::fs::File::create(artifacts.join(format!("{id}.events.jsonl")))?;
    Ok(tokio::spawn(async move {
        while let Some(event) = stream.recv().await? {
            serde_json::to_writer(&mut file, &event)?;
            file.write_all(b"\n")?;
            file.flush()?;
        }
        Ok(())
    }))
}
