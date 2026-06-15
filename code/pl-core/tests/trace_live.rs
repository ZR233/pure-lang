use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use pl_core::{
    CompileMode, CoreSession, ModelRole, PureConfig, PureCore, TraceEvent, TraceEventKind,
    TurnBudget, TurnRequest, TurnResultStatus,
};
use pretty_assertions::{assert_eq, assert_ne};

const DEEPSEEK_LIVE_ENV_KEY: &str = "API_KEY_DEEPSEEK";

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
            eprintln!("{DEEPSEEK_LIVE_ENV_KEY} is not set; skipping live trace test");
            None
        }
    }
}

async fn configured_core(api_key: String, workspace: &Path) -> PureCore {
    let mut config = PureConfig::default_config();
    config.skills.enabled = false;
    config.skills.auto_learn = false;
    config
        .providers
        .get_mut("deepseek")
        .expect("default config should include deepseek provider")
        .bearer_token = Some(api_key);

    let mut core = PureCore::from_config(&config, ModelRole::Planner).unwrap();
    core.register_default_tools(workspace, None).await;
    core
}

/// 提取 trace events 涉及的所有 turn_id（去重）。
fn turn_ids(events: &[TraceEvent]) -> HashSet<String> {
    events
        .iter()
        .filter_map(|event| match &event.kind {
            TraceEventKind::TracePartStarted { item }
            | TraceEventKind::TracePartCompleted { item }
            | TraceEventKind::TracePartFailed { item, .. } => Some(item.turn_id.clone()),
            TraceEventKind::TracePartDelta { event } => Some(event.turn_id.clone()),
            _ => None,
        })
        .collect()
}

/// 提取 trace events 涉及的所有 item_id（去重）。
fn item_ids(events: &[TraceEvent]) -> HashSet<String> {
    events
        .iter()
        .filter_map(|event| match &event.kind {
            TraceEventKind::TracePartStarted { item }
            | TraceEventKind::TracePartCompleted { item }
            | TraceEventKind::TracePartFailed { item, .. } => Some(item.item_id.clone()),
            TraceEventKind::TracePartDelta { event } => Some(event.item_id.clone()),
            _ => None,
        })
        .collect()
}

fn final_text_items(events: &[TraceEvent]) -> Vec<String> {
    let mut ids: Vec<String> = events
        .iter()
        .filter_map(|event| match &event.kind {
            TraceEventKind::TracePartStarted { item }
                if item.kind == pl_protocol::TracePartKind::Text =>
            {
                Some(item.item_id.clone())
            }
            _ => None,
        })
        .collect();
    ids.sort();
    ids.dedup();
    ids
}

#[tokio::test]
async fn cross_turn_trace_isolation_live() {
    let Some(api_key) = live_api_key() else {
        return;
    };

    let workspace = TempWorkspace::new("trace-live");
    tokio::fs::create_dir_all(workspace.path()).await.unwrap();
    let core = configured_core(api_key, workspace.path()).await;
    let mut session = CoreSession::new();
    let (event_tx, _event_rx) = tokio::sync::broadcast::channel(256);
    let mut recorder = pl_core::TraceRecorder::new("trace-live".to_string(), event_tx, 0);

    // turn 1：要求模型输出简短中文 final 文本
    let request1 = TurnRequest::new(
        "请只输出 <final>你好，第一轮。</final>，不要输出其它内容。".to_string(),
        CompileMode::Auto,
    )
    .with_budget(TurnBudget::new(90_000));
    let result1 = core
        .run_turn_with_trace(&mut session, request1, &mut recorder, Default::default())
        .await
        .unwrap();
    assert_eq!(
        result1.status,
        TurnResultStatus::Completed,
        "turn 1 failed: {:?}",
        result1.error
    );
    let turn1_turn_ids = turn_ids(&result1.trace_events);
    let turn1_item_ids = item_ids(&result1.trace_events);
    let turn1_final_items = final_text_items(&result1.trace_events);
    assert_eq!(
        turn1_turn_ids.len(),
        1,
        "turn 1 events should share a single turn_id: {turn1_turn_ids:?}"
    );
    let turn1_id = turn1_turn_ids.iter().next().unwrap().clone();
    assert!(
        !turn1_final_items.is_empty(),
        "turn 1 should produce at least one final text item"
    );

    // turn 2：第二轮，不同 turn_id（recorder/generate_session_id per-turn 唯一）
    let request2 = TurnRequest::new(
        "请只输出 <final>你好，第二轮。</final>，不要输出其它内容。".to_string(),
        CompileMode::Auto,
    )
    .with_budget(TurnBudget::new(90_000));
    let result2 = core
        .run_turn_with_trace(&mut session, request2, &mut recorder, Default::default())
        .await
        .unwrap();
    assert_eq!(
        result2.status,
        TurnResultStatus::Completed,
        "turn 2 failed: {:?}",
        result2.error
    );
    let turn2_turn_ids = turn_ids(&result2.trace_events);
    let turn2_item_ids = item_ids(&result2.trace_events);
    let turn2_final_items = final_text_items(&result2.trace_events);

    // 跨 turn turn_id 必须不同（防串台的根本隔离）
    let turn2_id = turn2_turn_ids.iter().next().expect("turn 2 has turn_id");
    assert_ne!(
        turn1_id, *turn2_id,
        "cross-turn turn_id must differ (otherwise trace parts collide)"
    );
    // 跨 turn item_id 绝不重叠
    let overlap: Vec<_> = turn1_item_ids.intersection(&turn2_item_ids).collect();
    assert!(
        overlap.is_empty(),
        "cross-turn item ids must not overlap: {overlap:?}"
    );
    assert!(
        !turn2_final_items.is_empty(),
        "turn 2 should produce at least one final text item"
    );
}
