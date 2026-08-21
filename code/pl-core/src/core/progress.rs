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
        let item = TracePart::runtime_commentary(
            self.turn_id.clone(),
            item_id,
            self.next_ordinal,
            text.into(),
            now,
        );
        recorder.complete_item(item);
    }
}
