use super::*;
use pretty_assertions::assert_eq;

#[test]
fn progress_emitter_sends_runtime_commentary_by_verbosity() {
    let (event_tx, mut event_rx) = tokio::sync::broadcast::channel(8);
    let mut progress =
        progress::ProgressEmitter::new(event_tx, "turn-1", progress::ProgressVerbosity::Normal);

    progress.milestone("正在准备上下文。");
    progress.heartbeat("等待模型响应。");
    progress.tool_detail("工具 `exec` 已完成。");

    let first = event_rx.try_recv().unwrap();
    assert!(event_rx.try_recv().is_err());

    let AgentEvent::TracePartCompleted { item: first } = first else {
        panic!("expected completed progress part");
    };
    assert_eq!(first.turn_id, "turn-1");
    assert_eq!(first.item_id, "turn-1:progress:1");
    assert_eq!(first.started_sequence, 1);
    assert_eq!(first.source, TracePartSource::Runtime);
    assert_eq!(first.text_channel, Some(TraceTextChannel::Commentary));
    assert_eq!(first.content, "正在准备上下文。");
}

#[test]
fn progress_emitter_sends_tool_detail_only_when_verbose() {
    let (normal_tx, mut normal_rx) = tokio::sync::broadcast::channel(8);
    let mut normal =
        progress::ProgressEmitter::new(normal_tx, "turn-1", progress::ProgressVerbosity::Normal);
    normal.tool_detail("工具 `exec` 已完成。");
    normal.tool_detail("工具结果已写入上下文，准备继续调用模型。");
    normal.tool_detail("模型请求调用 2 个工具。");
    assert!(normal_rx.try_recv().is_err());

    let (verbose_tx, mut verbose_rx) = tokio::sync::broadcast::channel(8);
    let mut verbose =
        progress::ProgressEmitter::new(verbose_tx, "turn-1", progress::ProgressVerbosity::Verbose);
    verbose.tool_detail("工具 `exec` 已完成。");
    verbose.tool_detail("工具结果已写入上下文，准备继续调用模型。");
    verbose.tool_detail("模型请求调用 2 个工具。");

    let AgentEvent::TracePartCompleted { item: first } = verbose_rx.try_recv().unwrap() else {
        panic!("expected completed progress part");
    };
    assert_eq!(first.source, TracePartSource::Runtime);
    assert_eq!(first.text_channel, Some(TraceTextChannel::Commentary));
    assert_eq!(first.content, "工具 `exec` 已完成。");

    let AgentEvent::TracePartCompleted { item: second } = verbose_rx.try_recv().unwrap() else {
        panic!("expected completed progress part");
    };
    assert_eq!(second.source, TracePartSource::Runtime);
    assert_eq!(second.text_channel, Some(TraceTextChannel::Commentary));
    assert_eq!(second.content, "工具结果已写入上下文，准备继续调用模型。");

    let AgentEvent::TracePartCompleted { item: third } = verbose_rx.try_recv().unwrap() else {
        panic!("expected completed progress part");
    };
    assert_eq!(third.source, TracePartSource::Runtime);
    assert_eq!(third.text_channel, Some(TraceTextChannel::Commentary));
    assert_eq!(third.content, "模型请求调用 2 个工具。");
}

#[test]
fn progress_emitter_scopes_item_ids_without_changing_turn_id() {
    let (event_tx, mut event_rx) = tokio::sync::broadcast::channel(8);
    let mut root_progress = progress::ProgressEmitter::new(
        event_tx.clone(),
        "turn-1",
        progress::ProgressVerbosity::Normal,
    );
    let mut tool_progress = progress::ProgressEmitter::new_scoped(
        event_tx,
        "turn-1",
        "turn-1:tool-progress",
        progress::ProgressVerbosity::Normal,
    );

    root_progress.milestone("准备上下文");
    tool_progress.milestone("执行工具");

    let first = event_rx.try_recv().unwrap();
    let second = event_rx.try_recv().unwrap();

    let AgentEvent::TracePartCompleted { item: first } = first else {
        panic!("expected completed progress part");
    };
    let AgentEvent::TracePartCompleted { item: second } = second else {
        panic!("expected completed progress part");
    };
    assert_eq!(first.turn_id, "turn-1");
    assert_eq!(first.item_id, "turn-1:progress:1");
    assert_eq!(second.turn_id, "turn-1");
    assert_eq!(second.item_id, "turn-1:tool-progress:1");
}
