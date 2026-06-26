use std::collections::HashMap;

use pl_protocol::{StudioPart, StudioPartStatus};
use pl_trace::TracePartDeltaEvent;

use super::records::StudioPartRecord;

#[derive(Default)]
pub(super) struct TurnTimelineActor {
    part_revisions: HashMap<String, u64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum TimelineDeltaDecision {
    Accept,
    Stale,
}

impl TurnTimelineActor {
    pub(super) fn prepare_snapshot_order(
        &self,
        part: &mut StudioPart,
        existing: Option<&StudioPartRecord>,
        next_order: u64,
    ) {
        part.order = existing
            .map(|record| record.part.order)
            .unwrap_or(next_order);
    }

    pub(super) fn prepare_snapshot(&mut self, part: &mut StudioPart) {
        if is_terminal_studio_part_status(part.status)
            && let Some(live_revision) = self.part_revisions.get(&part.part_id).copied()
            && part.revision < live_revision
        {
            part.revision = live_revision;
        }
    }

    pub(super) fn record_snapshot(&mut self, part: &StudioPart) {
        if is_terminal_studio_part_status(part.status) {
            self.part_revisions.remove(&part.part_id);
        } else {
            let revision = self
                .part_revisions
                .get(&part.part_id)
                .copied()
                .unwrap_or(0)
                .max(part.revision);
            self.part_revisions.insert(part.part_id.clone(), revision);
        }
    }

    pub(super) fn prepare_delta(
        &mut self,
        event: &TracePartDeltaEvent,
        existing: Option<&StudioPartRecord>,
    ) -> TimelineDeltaDecision {
        let Some(existing) = existing else {
            return TimelineDeltaDecision::Stale;
        };
        if is_terminal_studio_part_status(existing.part.status) {
            return TimelineDeltaDecision::Stale;
        }
        let current_revision = self
            .part_revisions
            .get(&event.item_id)
            .copied()
            .unwrap_or(existing.part.revision);
        if event.revision == current_revision + 1 {
            self.part_revisions
                .insert(event.item_id.clone(), event.revision);
            TimelineDeltaDecision::Accept
        } else {
            TimelineDeltaDecision::Stale
        }
    }
}

pub(super) fn is_terminal_studio_part_status(status: StudioPartStatus) -> bool {
    matches!(
        status,
        StudioPartStatus::Completed
            | StudioPartStatus::Failed
            | StudioPartStatus::Interrupted
            | StudioPartStatus::Denied
            | StudioPartStatus::BudgetLimited
    )
}

#[cfg(test)]
mod tests {
    use pl_protocol::{StudioPartType, StudioTextChannel};
    use pl_trace::{TraceDelta, TracePartDeltaEvent, TracePartKind, TracePartStatus};

    use super::*;

    #[test]
    fn accepts_only_contiguous_live_delta_revisions() {
        let mut actor = TurnTimelineActor::default();
        let existing = StudioPartRecord {
            part: part("part-1", StudioPartStatus::Streaming, 0),
            sequence: 0,
        };

        assert_eq!(
            actor.prepare_delta(&delta("part-1", 1), Some(&existing)),
            TimelineDeltaDecision::Accept
        );
        assert_eq!(
            actor.prepare_delta(&delta("part-1", 1), Some(&existing)),
            TimelineDeltaDecision::Stale
        );
        assert_eq!(
            actor.prepare_delta(&delta("part-1", 3), Some(&existing)),
            TimelineDeltaDecision::Stale
        );
        assert_eq!(
            actor.prepare_delta(&delta("part-1", 2), Some(&existing)),
            TimelineDeltaDecision::Accept
        );
    }

    #[test]
    fn terminal_snapshot_catches_up_to_live_revision() {
        let mut actor = TurnTimelineActor::default();
        let existing = StudioPartRecord {
            part: part("part-1", StudioPartStatus::Streaming, 0),
            sequence: 0,
        };
        let _ = actor.prepare_delta(&delta("part-1", 1), Some(&existing));
        let mut terminal = part("part-1", StudioPartStatus::Completed, 0);

        actor.prepare_snapshot(&mut terminal);

        assert_eq!(terminal.revision, 1);
    }

    #[test]
    fn snapshot_order_uses_existing_part_or_next_message_order() {
        let actor = TurnTimelineActor::default();
        let existing = StudioPartRecord {
            part: {
                let mut part = part("part-1", StudioPartStatus::Streaming, 0);
                part.order = 7;
                part
            },
            sequence: 0,
        };
        let mut repeat = part("part-1", StudioPartStatus::Completed, 1);
        actor.prepare_snapshot_order(&mut repeat, Some(&existing), 42);
        assert_eq!(repeat.order, 7);

        let mut new_part = part("part-2", StudioPartStatus::Started, 0);
        actor.prepare_snapshot_order(&mut new_part, None, 42);
        assert_eq!(new_part.order, 42);
    }

    #[test]
    fn terminal_parts_reject_late_deltas() {
        let mut actor = TurnTimelineActor::default();
        let existing = StudioPartRecord {
            part: part("part-1", StudioPartStatus::Completed, 1),
            sequence: 0,
        };

        assert_eq!(
            actor.prepare_delta(&delta("part-1", 2), Some(&existing)),
            TimelineDeltaDecision::Stale
        );
    }

    fn part(part_id: &str, status: StudioPartStatus, revision: u64) -> StudioPart {
        StudioPart {
            part_id: part_id.to_string(),
            message_id: "turn-1:assistant".to_string(),
            session_id: "session-1".to_string(),
            turn_id: "turn-1".to_string(),
            part_type: StudioPartType::Text,
            order: 0,
            revision,
            status,
            created_at: 100,
            updated_at: 100,
            completed_at: None,
            error: None,
            text_channel: Some(StudioTextChannel::Final),
            text: String::new(),
            attachments: Vec::new(),
            tool: None,
            agent: None,
            inference: None,
            plan: None,
            file: None,
            usage: None,
            synthetic: false,
            ignored: false,
        }
    }

    fn delta(part_id: &str, revision: u64) -> TracePartDeltaEvent {
        TracePartDeltaEvent {
            turn_id: "turn-1".to_string(),
            item_id: part_id.to_string(),
            started_sequence: 0,
            revision,
            kind: TracePartKind::Text,
            status: TracePartStatus::Streaming,
            created_at: 100,
            updated_at: 100,
            delta: TraceDelta::Text {
                text_channel: pl_trace::TraceTextChannel::Final,
                delta: "x".to_string(),
            },
        }
    }
}
