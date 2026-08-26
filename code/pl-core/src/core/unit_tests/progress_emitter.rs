use super::*;
use pretty_assertions::assert_eq;

fn commentary(item: &pl_trace::TracePart) -> &str {
    let text = item.text().expect("runtime commentary part");
    assert_eq!(text.channel(), TraceTextChannel::Commentary);
    text.content()
}

fn receive_completed_progress(
    receiver: &mut tokio::sync::broadcast::Receiver<AgentEvent>,
) -> pl_trace::TracePart {
    let AgentEvent::TracePartStarted { item: started } = receiver.try_recv().unwrap() else {
        panic!("expected started progress part");
    };
    let AgentEvent::TracePartCompleted { item: completed } = receiver.try_recv().unwrap() else {
        panic!("expected completed progress part");
    };
    assert_eq!(started.item_id(), completed.item_id());
    assert!(!started.is_terminal());
    assert!(completed.is_terminal());
    completed
}

#[test]
fn progress_emitter_sends_runtime_commentary_by_verbosity() {
    let (event_tx, mut event_rx) = tokio::sync::broadcast::channel(8);
    let mut recorder = TraceRecorder::disabled(event_tx);
    let mut progress =
        progress::ProgressEmitter::new("turn-1", progress::ProgressVerbosity::Normal);

    progress.milestone(&mut recorder, "正在准备上下文。");
    progress.heartbeat(&mut recorder, "等待模型响应。");
    progress.tool_detail(&mut recorder, "工具 `exec` 已完成。");

    let first = receive_completed_progress(&mut event_rx);
    assert!(event_rx.try_recv().is_err());
    assert_eq!(first.turn_id(), "turn-1");
    assert_eq!(first.item_id(), "turn-1:progress:1");
    assert_eq!(first.started_sequence(), 0);
    assert_eq!(first.source(), TracePartSource::Runtime);
    assert_eq!(commentary(&first), "正在准备上下文。");
}

#[test]
fn progress_milestone_enters_the_durable_trace_channel() {
    let (event_tx, _) = tokio::sync::broadcast::channel(8);
    let (durable_tx, mut durable_rx) = tokio::sync::mpsc::unbounded_channel();
    let mut recorder =
        TraceRecorder::streaming("thread-1".to_string(), event_tx.clone(), 7, durable_tx);
    let mut progress =
        progress::ProgressEmitter::new("turn-1", progress::ProgressVerbosity::Normal);

    progress.milestone(&mut recorder, "正在准备上下文。");

    let started = durable_rx
        .try_recv()
        .expect("milestone must enter the durable trace channel");
    assert_eq!(started.session_id, "thread-1");
    assert_eq!(started.sequence, 7);
    assert!(matches!(
        started.kind,
        TraceEventKind::TracePartStarted { .. }
    ));
    let completed = durable_rx
        .try_recv()
        .expect("milestone terminal must follow its start");
    assert_eq!(completed.sequence, 8);
    let TraceEventKind::TracePartCompleted { item } = completed.kind else {
        panic!("expected completed progress part");
    };
    assert_eq!(item.turn_id(), "turn-1");
    assert_eq!(item.source(), TracePartSource::Runtime);
    assert_eq!(commentary(&item), "正在准备上下文。");
}

#[test]
fn progress_emitter_sends_tool_detail_only_when_verbose() {
    let (normal_tx, mut normal_rx) = tokio::sync::broadcast::channel(8);
    let mut normal_recorder = TraceRecorder::disabled(normal_tx);
    let mut normal = progress::ProgressEmitter::new("turn-1", progress::ProgressVerbosity::Normal);
    normal.tool_detail(&mut normal_recorder, "工具 `exec` 已完成。");
    normal.tool_detail(
        &mut normal_recorder,
        "工具结果已写入上下文，准备继续调用模型。",
    );
    normal.tool_detail(&mut normal_recorder, "模型请求调用 2 个工具。");
    assert!(normal_rx.try_recv().is_err());

    let (verbose_tx, mut verbose_rx) = tokio::sync::broadcast::channel(8);
    let mut verbose_recorder = TraceRecorder::disabled(verbose_tx);
    let mut verbose =
        progress::ProgressEmitter::new("turn-1", progress::ProgressVerbosity::Verbose);
    verbose.tool_detail(&mut verbose_recorder, "工具 `exec` 已完成。");
    verbose.tool_detail(
        &mut verbose_recorder,
        "工具结果已写入上下文，准备继续调用模型。",
    );
    verbose.tool_detail(&mut verbose_recorder, "模型请求调用 2 个工具。");

    let first = receive_completed_progress(&mut verbose_rx);
    assert_eq!(first.source(), TracePartSource::Runtime);
    assert_eq!(commentary(&first), "工具 `exec` 已完成。");

    let second = receive_completed_progress(&mut verbose_rx);
    assert_eq!(second.source(), TracePartSource::Runtime);
    assert_eq!(
        commentary(&second),
        "工具结果已写入上下文，准备继续调用模型。"
    );

    let third = receive_completed_progress(&mut verbose_rx);
    assert_eq!(third.source(), TracePartSource::Runtime);
    assert_eq!(commentary(&third), "模型请求调用 2 个工具。");
}

#[test]
fn progress_emitter_scopes_item_ids_without_changing_turn_id() {
    let (event_tx, mut event_rx) = tokio::sync::broadcast::channel(8);
    let mut recorder = TraceRecorder::disabled(event_tx);
    let mut root_progress =
        progress::ProgressEmitter::new("turn-1", progress::ProgressVerbosity::Normal);
    let mut tool_progress = progress::ProgressEmitter::new_scoped(
        "turn-1",
        "turn-1:tool-progress",
        progress::ProgressVerbosity::Normal,
    );

    root_progress.milestone(&mut recorder, "准备上下文");
    tool_progress.milestone(&mut recorder, "执行工具");

    let first = receive_completed_progress(&mut event_rx);
    let second = receive_completed_progress(&mut event_rx);
    assert_eq!(first.turn_id(), "turn-1");
    assert_eq!(first.item_id(), "turn-1:progress:1");
    assert_eq!(second.turn_id(), "turn-1");
    assert_eq!(second.item_id(), "turn-1:tool-progress:1");
}
