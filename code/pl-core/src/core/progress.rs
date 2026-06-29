use pl_trace::{AgentEvent, AgentEventSender, TracePart};

use super::turn_result::unix_seconds;

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
    Tool,
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
            (Self::Normal, ProgressLevel::Milestone | ProgressLevel::Tool) => true,
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
    event_tx: AgentEventSender,
    turn_id: String,
    item_prefix: String,
    next_ordinal: u64,
    verbosity: ProgressVerbosity,
}

impl ProgressEmitter {
    pub(crate) fn new(
        event_tx: AgentEventSender,
        turn_id: impl Into<String>,
        verbosity: ProgressVerbosity,
    ) -> Self {
        let turn_id = turn_id.into();
        let item_prefix = format!("{turn_id}:progress");
        Self::new_scoped(event_tx, turn_id, item_prefix, verbosity)
    }

    pub(crate) fn new_scoped(
        event_tx: AgentEventSender,
        turn_id: impl Into<String>,
        item_prefix: impl Into<String>,
        verbosity: ProgressVerbosity,
    ) -> Self {
        Self {
            event_tx,
            turn_id: turn_id.into(),
            item_prefix: item_prefix.into(),
            next_ordinal: 0,
            verbosity,
        }
    }

    pub(crate) fn milestone(&mut self, text: impl Into<String>) {
        self.emit(ProgressLevel::Milestone, text);
    }

    pub(crate) fn tool(&mut self, text: impl Into<String>) {
        self.emit(ProgressLevel::Tool, text);
    }

    pub(crate) fn tool_detail(&mut self, text: impl Into<String>) {
        self.emit(ProgressLevel::ToolDetail, text);
    }

    pub(crate) fn heartbeat(&mut self, text: impl Into<String>) {
        self.emit(ProgressLevel::Heartbeat, text);
    }

    pub(crate) fn debug(&mut self, text: impl Into<String>) {
        self.emit(ProgressLevel::Debug, text);
    }

    fn emit(&mut self, level: ProgressLevel, text: impl Into<String>) {
        if !self.verbosity.allows(level) {
            return;
        }

        self.next_ordinal += 1;
        let item_id = format!("{}:{}", self.item_prefix, self.next_ordinal);
        let now = unix_seconds();
        let item = TracePart::runtime_commentary(
            self.turn_id.clone(),
            item_id,
            self.next_ordinal,
            text.into(),
            now,
        );
        let _ = self.event_tx.send(AgentEvent::TracePartCompleted { item });
    }
}
