use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use pl_core::{
    CompileMode, CoreSession, ModelRole, PureConfig, PureCore, ToolApprovalDecision,
    ToolApprovalRequest, TraceEvent, TurnBudget, TurnOptions, TurnRequest, TurnResultStatus,
};
use pl_protocol::{TimelineItemStatus, TraceEventKind};
use pretty_assertions::assert_eq;

const DEEPSEEK_LIVE_ENV_KEY: &str = "API_KEY_DEEPSEEK";
const ORIGINAL_NOTES: &str = "title: apply patch live\nstatus: pending\nkeep: unchanged\n";
const EXPECTED_NOTES: &str =
    "title: apply patch live\nstatus: verified-by-live-apply-patch-test\nkeep: unchanged\n";

struct TempWorkspace {
    path: PathBuf,
}

impl TempWorkspace {
    fn new(name: &str) -> Self {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let path =
            std::env::temp_dir().join(format!("pure-lang-{name}-{}-{stamp}", std::process::id()));
        Self { path }
    }

    fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for TempWorkspace {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.path);
    }
}

fn live_api_key() -> Option<String> {
    match std::env::var(DEEPSEEK_LIVE_ENV_KEY) {
        Ok(value) if !value.trim().is_empty() => Some(value),
        _ => {
            eprintln!("{DEEPSEEK_LIVE_ENV_KEY} is not set; skipping live apply_patch test");
            None
        }
    }
}

fn allowed_tool(name: &str) -> bool {
    matches!(
        name,
        "read_file" | "list_files" | "search_files" | "stat_path" | "apply_patch"
    )
}

fn approval_options(requested_tools: Arc<Mutex<Vec<String>>>) -> TurnOptions {
    TurnOptions::manual(Arc::new(move |request: ToolApprovalRequest| {
        let requested_tools = requested_tools.clone();
        Box::pin(async move {
            {
                requested_tools
                    .lock()
                    .expect("requested tool log poisoned")
                    .push(request.name.clone());
            }
            if allowed_tool(&request.name) {
                ToolApprovalDecision::Approved
            } else {
                ToolApprovalDecision::Denied {
                    reason: format!("forbidden tool in live apply_patch test: {}", request.name),
                }
            }
        })
    }))
}

fn configured_core(api_key: String, workspace: &Path) -> PureCore {
    let mut config = PureConfig::default_config();
    config.skills.enabled = false;
    config.skills.auto_learn = false;
    config
        .providers
        .get_mut("deepseek")
        .expect("default config should include deepseek provider")
        .bearer_token = Some(api_key);

    let mut core = PureCore::from_config(&config, ModelRole::Planner).unwrap();
    core.register_default_tools(workspace, None);
    core
}

fn live_prompt() -> String {
    r#"你正在一个临时 workspace 中执行真实集成测试。

必须实际调用 `apply_patch` 工具修改 `notes.txt`，不要调用 `bash`，不要调用 `write_file`，不要把 patch 当作正文输出。

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

工具成功后，用一句中文简短说明已完成。"#
        .to_string()
}

fn tool_diagnostics(events: &[TraceEvent]) -> String {
    let mut output = String::new();
    for event in events {
        match &event.kind {
            TraceEventKind::TimelineItemStarted { item }
            | TraceEventKind::TimelineItemCompleted { item } => {
                if let Some(tool) = &item.tool {
                    let _ = writeln!(
                        output,
                        "#{} {:?} {} args={} result={}",
                        event.sequence,
                        item.status,
                        tool.name,
                        tool.arguments,
                        tool.result.as_deref().unwrap_or("")
                    );
                }
            }
            TraceEventKind::TimelineItemFailed { item, error } => {
                if let Some(tool) = &item.tool {
                    let _ = writeln!(
                        output,
                        "#{} {:?} {} args={} result={} error={}",
                        event.sequence,
                        item.status,
                        tool.name,
                        tool.arguments,
                        tool.result.as_deref().unwrap_or(""),
                        error
                    );
                }
            }
            TraceEventKind::TimelineItemDelta { .. }
            | TraceEventKind::PlanLifecycleChanged { .. } => {}
        }
    }
    if output.is_empty() {
        "no tool timeline events recorded".to_string()
    } else {
        output
    }
}

fn saw_apply_patch(events: &[TraceEvent]) -> bool {
    events.iter().any(|event| match &event.kind {
        TraceEventKind::TimelineItemStarted { item }
        | TraceEventKind::TimelineItemCompleted { item }
        | TraceEventKind::TimelineItemFailed { item, .. } => item
            .tool
            .as_ref()
            .is_some_and(|tool| tool.name == "apply_patch"),
        TraceEventKind::TimelineItemDelta { .. } | TraceEventKind::PlanLifecycleChanged { .. } => {
            false
        }
    })
}

fn failed_apply_patch(events: &[TraceEvent]) -> bool {
    events.iter().any(|event| {
        matches!(
            &event.kind,
            TraceEventKind::TimelineItemFailed { item, .. }
                if item.tool.as_ref().is_some_and(|tool| tool.name == "apply_patch")
        )
    })
}

#[tokio::test]
async fn live_deepseek_applies_patch_with_prompt() {
    let Some(api_key) = live_api_key() else {
        return;
    };

    let workspace = TempWorkspace::new("deepseek-apply-patch-live");
    tokio::fs::create_dir_all(workspace.path()).await.unwrap();
    tokio::fs::write(workspace.path().join("notes.txt"), ORIGINAL_NOTES)
        .await
        .unwrap();

    let core = configured_core(api_key, workspace.path());
    let requested_tools = Arc::new(Mutex::new(Vec::new()));
    let mut session = CoreSession::new();
    let (event_tx, _event_rx) = tokio::sync::broadcast::channel(256);
    let mut recorder = pl_core::TraceRecorder::new("live-apply-patch".to_string(), event_tx, 0);
    let request =
        TurnRequest::new(live_prompt(), CompileMode::Auto).with_budget(TurnBudget::new(180_000));
    let result = core
        .run_turn_with_trace(
            &mut session,
            request,
            &mut recorder,
            approval_options(requested_tools.clone()),
        )
        .await
        .unwrap();

    let diagnostics = tool_diagnostics(&result.timeline_events);
    assert_eq!(
        result.status,
        TurnResultStatus::Completed,
        "live turn failed: {:?}\n{}",
        result.error,
        diagnostics
    );

    let requested_tools = requested_tools
        .lock()
        .expect("requested tool log poisoned")
        .clone();
    let forbidden_tools = requested_tools
        .iter()
        .filter(|tool| !allowed_tool(tool))
        .cloned()
        .collect::<Vec<_>>();
    assert!(
        forbidden_tools.is_empty(),
        "forbidden tools were requested: {:?}\n{}",
        forbidden_tools,
        diagnostics
    );
    assert!(
        saw_apply_patch(&result.timeline_events),
        "model did not call apply_patch\n{}",
        diagnostics
    );
    assert!(
        !failed_apply_patch(&result.timeline_events),
        "apply_patch failed\n{}",
        diagnostics
    );
    assert!(
        result.timeline_events.iter().any(|event| matches!(
            &event.kind,
            TraceEventKind::TimelineItemCompleted { item }
                if item.status == TimelineItemStatus::Completed
                    && item.tool.as_ref().is_some_and(|tool| tool.name == "apply_patch")
        )),
        "apply_patch did not complete successfully\n{}",
        diagnostics
    );

    let actual = tokio::fs::read_to_string(workspace.path().join("notes.txt"))
        .await
        .unwrap();
    assert_eq!(actual, EXPECTED_NOTES, "{diagnostics}");
}
