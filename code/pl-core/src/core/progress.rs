use pl_trace::TracePart;

use crate::TraceRecorder;
use crate::time::unix_seconds;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ProgressVerbosity {
    Quiet,
    Normal,
    Verbose,
    Debug,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ProgressLevel {
    Milestone,
    ToolDetail,
    Heartbeat,
    Debug,
}

impl ProgressVerbosity {
    pub(crate) fn from_env() -> Self {
        match std::env::var("PURE_PROGRESS_VERBOSITY") {
            Ok(value) if value.eq_ignore_ascii_case("quiet") => Self::Quiet,
            Ok(value) if value.eq_ignore_ascii_case("verbose") => Self::Verbose,
            Ok(value) if value.eq_ignore_ascii_case("debug") => Self::Debug,
            Ok(_) | Err(_) => Self::Normal,
        }
    }

    pub(crate) fn allows(self, level: ProgressLevel) -> bool {
        match (self, level) {
            (Self::Quiet, _) => false,
            (Self::Normal, ProgressLevel::Milestone) => true,
            (
                Self::Normal,
                ProgressLevel::ToolDetail | ProgressLevel::Heartbeat | ProgressLevel::Debug,
            ) => false,
            (Self::Verbose, ProgressLevel::Debug) => false,
            (Self::Verbose, _) => true,
            (Self::Debug, _) => true,
        }
    }
}

pub(crate) struct ProgressEmitter {
    turn_id: String,
    item_prefix: String,
    next_ordinal: u64,
    verbosity: ProgressVerbosity,
}

impl ProgressEmitter {
    pub(crate) fn new(turn_id: impl Into<String>, verbosity: ProgressVerbosity) -> Self {
        let turn_id = turn_id.into();
        let item_prefix = format!("{turn_id}:progress");
        Self::new_scoped(turn_id, item_prefix, verbosity)
    }

    pub(crate) fn new_scoped(
        turn_id: impl Into<String>,
        item_prefix: impl Into<String>,
        verbosity: ProgressVerbosity,
    ) -> Self {
        Self {
            turn_id: turn_id.into(),
            item_prefix: item_prefix.into(),
            next_ordinal: 0,
            verbosity,
        }
    }

    pub(crate) fn milestone(&mut self, recorder: &mut TraceRecorder, text: impl Into<String>) {
        self.emit(recorder, ProgressLevel::Milestone, text);
    }

    pub(crate) fn tool_detail(&mut self, recorder: &mut TraceRecorder, text: impl Into<String>) {
        self.emit(recorder, ProgressLevel::ToolDetail, text);
    }

    pub(crate) fn heartbeat(&mut self, recorder: &mut TraceRecorder, text: impl Into<String>) {
        self.emit(recorder, ProgressLevel::Heartbeat, text);
    }

    pub(crate) fn debug(&mut self, recorder: &mut TraceRecorder, text: impl Into<String>) {
        self.emit(recorder, ProgressLevel::Debug, text);
    }

    fn emit(
        &mut self,
        recorder: &mut TraceRecorder,
        level: ProgressLevel,
        text: impl Into<String>,
    ) {
        if !self.verbosity.allows(level) {
            return;
        }

        self.next_ordinal += 1;
        let prefix = &self.item_prefix;
        let ordinal = self.next_ordinal;
        let item_id = format!("{prefix}:{ordinal}");
        let now = unix_seconds();
        let content = text.into();
        let mut item = TracePart::runtime_commentary(
            self.turn_id.clone(),
            item_id,
            recorder.current_sequence(),
            content.clone(),
            now,
        );
        recorder.start_item(item.clone());
        if let Err(error) = item.apply(item.command(
            now,
            pl_trace::TracePartAction::Complete(pl_trace::TracePartCompletion::Text {
                authoritative_content: Some(content),
            }),
        )) {
            tracing::error!(%error, "failed to complete runtime progress item");
            return;
        }
        recorder.complete_item(item);
    }
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;

    use super::*;
    use crate::trace::TraceRecorder;
    use pl_trace::{AgentEvent, TraceEventKind, TracePartSource, TraceTextChannel};

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
        let AgentEvent::TracePartCompleted { item: completed } = receiver.try_recv().unwrap()
        else {
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
        let mut progress = ProgressEmitter::new("turn-1", ProgressVerbosity::Normal);

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
        let mut progress = ProgressEmitter::new("turn-1", ProgressVerbosity::Normal);

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
        let mut normal = ProgressEmitter::new("turn-1", ProgressVerbosity::Normal);
        normal.tool_detail(&mut normal_recorder, "工具 `exec` 已完成。");
        normal.tool_detail(
            &mut normal_recorder,
            "工具结果已写入上下文，准备继续调用模型。",
        );
        normal.tool_detail(&mut normal_recorder, "模型请求调用 2 个工具。");
        assert!(normal_rx.try_recv().is_err());

        let (verbose_tx, mut verbose_rx) = tokio::sync::broadcast::channel(8);
        let mut verbose_recorder = TraceRecorder::disabled(verbose_tx);
        let mut verbose = ProgressEmitter::new("turn-1", ProgressVerbosity::Verbose);
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
        let mut root_progress = ProgressEmitter::new("turn-1", ProgressVerbosity::Normal);
        let mut tool_progress = ProgressEmitter::new_scoped(
            "turn-1",
            "turn-1:tool-progress",
            ProgressVerbosity::Normal,
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
}
