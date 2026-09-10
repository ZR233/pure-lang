//! Opt-in provider/file-system acceptance through the canonical Thread/tool boundary.
use pl_core::{
    context::ContextContent,
    model::Model,
    thread::{ThreadHandle, TurnInput, TurnOutcome, task::TaskStatus},
};
use pl_model::runtime::{ModelRuntime, ThreadModel, thread_tool_declaration};
use pl_tool::{
    workspace::{AgentWorkspace, ToolWorkspace},
    workspace_file::{LocalWorkspaceFileBackend, ThreadWorkspaceFileTool, WorkspaceFileToolKind},
};
use pretty_assertions::assert_eq;
use std::sync::Arc;
use std::time::Duration;
mod support;
const ORIGINAL_NOTES: &str = "title: apply patch live\nstatus: pending\nkeep: unchanged\n";
const EXPECTED_NOTES: &str =
    "title: apply patch live\nstatus: verified-by-live-apply-patch-test\nkeep: unchanged\n";

fn live_prompt() -> String {
    r#"你正在一个临时 workspace 中执行真实集成测试。

必须实际调用 `apply_patch` 工具修改 `notes.txt`，不要调用 `exec`，不要调用 `write_file`，不要把 patch 当作正文输出。

任务：把 `notes.txt` 中这一行：
status: pending

替换为：
status: verified-by-live-apply-patch-test

保留其它行不变。可以先用 `read_file` 查看文件。`apply_patch` 必须使用 Codex 风格格式，例如：

*** Begin Patch
*** Update File: notes.txt
@@
-status: pending
+status: verified-by-live-apply-patch-test
*** End Patch

工具成功后，只输出 `<final>已完成修改。</final>`，不要输出标签之外的普通正文。"#
        .to_string()
}

#[tokio::test]
async fn live_deepseek_applies_patch_with_prompt() {
    let api_key = std::env::var("DEEPSEEK_API_KEY")
        .ok()
        .filter(|value| !value.trim().is_empty())
        .expect("explicit live acceptance requires configured provider credentials");
    let workspace = tempfile::tempdir().unwrap();
    tokio::fs::write(workspace.path().join("notes.txt"), ORIGINAL_NOTES)
        .await
        .unwrap();
    let route = support::deepseek_route(api_key);
    let model = ThreadModel::new(
        ModelRuntime::from_route(&route).unwrap(),
        route.reasoning_config(),
    );
    let thread = ThreadHandle::start(
        "live-apply-patch".into(),
        model.open_session().await.unwrap(),
    )
    .unwrap();
    let workspace_policy = ToolWorkspace::new(AgentWorkspace::local(workspace.path()));
    let backend = Arc::new(
        LocalWorkspaceFileBackend::confined(workspace_policy.clone())
            .await
            .unwrap(),
    );
    let tools = WorkspaceFileToolKind::all()
        .iter()
        .map(|kind| {
            let declaration = pl_tool::thread_catalog::ThreadBuiltin::File(*kind).declaration();
            ThreadWorkspaceFileTool::new(*kind, backend.clone(), workspace_policy.clone())
                .registration(thread_tool_declaration(&declaration).unwrap())
                .unwrap()
        })
        .collect();
    thread.register_tools(tools).await.unwrap();
    let cancellation = tokio_util::sync::CancellationToken::new();
    let execution = thread.run_turn(TurnInput {
        turn_id: "turn".into(),
        attempt_prefix: "live".into(),
        content: vec![ContextContent::Text {
            text: live_prompt().into(),
        }],
        max_model_steps: 20.try_into().unwrap(),
        cancellation: cancellation.clone(),
    });
    let result = match tokio::time::timeout(Duration::from_secs(180), execution).await {
        Ok(result) => result,
        Err(error) => {
            cancellation.cancel();
            thread.close().await.unwrap();
            panic!("live apply_patch timed out: {error}");
        }
    };
    let snapshot = thread.snapshot();
    thread.close().await.unwrap();
    let result = result
        .unwrap_or_else(|error| panic!("live Turn failed: {error}; tasks={:?}", snapshot.tasks));
    assert!(
        matches!(
            result.outcome,
            TurnOutcome::Completed | TurnOutcome::ToolCompleted
        ),
        "{result:?}"
    );
    let patch_tasks = snapshot
        .tasks
        .values()
        .filter(|task| task.tool_id == "apply_patch")
        .collect::<Vec<_>>();
    assert!(
        !patch_tasks.is_empty(),
        "model did not call apply_patch: {:?}",
        snapshot.tasks
    );
    assert!(
        patch_tasks
            .iter()
            .all(|task| task.status == TaskStatus::Succeeded),
        "patch failures: {patch_tasks:?}"
    );
    assert!(
        snapshot.tasks.values().all(|task| matches!(
            task.tool_id.as_str(),
            "read_file" | "list_files" | "apply_patch"
        )),
        "unexpected tool: {:?}",
        snapshot.tasks
    );
    let actual = tokio::fs::read_to_string(workspace.path().join("notes.txt"))
        .await
        .unwrap();
    assert_eq!(actual, EXPECTED_NOTES);
}
