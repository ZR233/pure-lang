//! Simple（简洁）模式 live 验收：真实模型、真实 prompt、临时项目。
//!
//! 覆盖：内存先行的目录创建、完整 Turn 执行与工具写盘、关机排空落库、
//! 重启后冷分页目录与冷历史查询。

#[path = "task_fixture/git.rs"]
mod git;
#[path = "task_fixture/live.rs"]
mod live_fixture;

use std::time::Duration;

use anyhow::{Context, Result, bail};
use live_fixture::LiveTaskFixture;
use pl_studio_runtime::{
    StudioHostKind, StudioMode, StudioRuntime, StudioRuntimeOptions, StudioSubmitPromptOptions,
    StudioSubmitPromptRequest,
};

const LIVE_TIMEOUT: Duration = Duration::from_secs(10 * 60);
const EXPECTED_NOTES: &str = "PURE_SIMPLE_LIVE_OK";

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "uses the installed Studio model configuration and incurs real model usage"]
async fn installed_config_simple_mode_completes_a_full_turn_and_survives_restart() -> Result<()> {
    let fixture = LiveTaskFixture::new_with_mode(
        StudioMode::Simple,
        "Live simple mode turn",
        /* require_node */ false,
    )
    .await?;
    let result = tokio::time::timeout(LIVE_TIMEOUT, run_live_simple_flow(&fixture))
        .await
        .context("live Simple integration test exceeded the 10 minute timeout")
        .and_then(|result| result);
    if let Err(error) = &result {
        eprintln!(
            "live Simple integration failed: {error:#}\n{}",
            fixture.diagnostics().await
        );
    }
    let shutdown = tokio::time::timeout(Duration::from_secs(30), fixture.shutdown())
        .await
        .context("Studio runtime shutdown timed out")
        .and_then(|result| result);
    let config_unchanged = fixture.assert_config_unchanged();

    result?;
    shutdown?;
    config_unchanged
}

async fn run_live_simple_flow(fixture: &LiveTaskFixture) -> Result<()> {
    fixture
        .runtime
        .submit_prompt(StudioSubmitPromptRequest {
            thread_id: fixture.thread_id.clone(),
            input: pl_protocol::studio::StudioPromptInput {
                text: format!(
                    "Create a file named notes.txt in the repository root with exactly this single \
                     line of content and no extra whitespace: {EXPECTED_NOTES}. Use your file tools, \
                     then finish with a one-line summary."
                ),
                attachment_draft_ids: Vec::new(),
            },
            options: StudioSubmitPromptOptions::default(),
        })
        .await?;

    let deadline = tokio::time::Instant::now() + LIVE_TIMEOUT;
    let mut notes_seen = false;
    while tokio::time::Instant::now() < deadline {
        if tokio::fs::try_exists(fixture.workspace.join("notes.txt")).await? {
            notes_seen = true;
            break;
        }
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
    anyhow::ensure!(notes_seen, "model never created notes.txt");
    fixture.wait_for_no_active_turns().await?;

    let content = tokio::fs::read_to_string(fixture.workspace.join("notes.txt"))
        .await
        .context("notes.txt disappeared")?;
    if !content.contains(EXPECTED_NOTES) {
        bail!("notes.txt content did not contain {EXPECTED_NOTES}: {content:?}");
    }

    // 关机排空后重启：目录必须能从冷数据分页恢复可见，历史从 SQLite 冷读。
    fixture
        .runtime
        .shutdown_runtime()
        .await
        .context("first shutdown failed")?;
    let reopened = StudioRuntime::with_options(StudioRuntimeOptions {
        studio_home: Some(fixture.studio_home.clone()),
        host: StudioHostKind::Test,
    })
    .await
    .map_err(anyhow::Error::new)?;
    reopened
        .start_runtime()
        .await
        .context("reopened runtime failed to start")?;

    let page = reopened
        .list_threads_page(None, 10)
        .await
        .context("cold directory page failed")?;
    let page_data = page
        .state
        .value()
        .context("cold directory page was not ready")?;
    anyhow::ensure!(
        page_data
            .threads
            .iter()
            .any(|thread| thread.id == fixture.thread_id),
        "thread was not visible in the cold directory page after restart"
    );

    let history = reopened
        .list_thread_turns(&fixture.thread_id, None, 10)
        .await
        .context("cold history query failed")?;
    anyhow::ensure!(
        !history.turns.is_empty(),
        "no durable turns were readable after restart"
    );

    tokio::time::timeout(Duration::from_secs(30), reopened.shutdown_runtime())
        .await
        .context("reopened runtime shutdown timed out")
        .and_then(|result| result)?;
    Ok(())
}
