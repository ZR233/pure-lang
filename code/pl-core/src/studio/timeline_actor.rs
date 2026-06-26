use std::collections::HashMap;

use pl_protocol::{StudioPart, StudioPartStatus};
use pl_trace::TracePartDeltaEvent;

use super::records::StudioPartRecord;

#[derive(Default)]
pub(super) struct TurnTimelineActor {
    part_revisions: HashMap<String, u64>,
    part_orders: HashMap<String, u64>,
    next_orders_by_message: HashMap<String, u64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum TimelineDeltaDecision {
    Accept,
    Stale,
}

impl TurnTimelineActor {
    pub(super) fn prepare_snapshot_order(
        &mut self,
        part: &mut StudioPart,
        existing: Option<&StudioPartRecord>,
        durable_next_order: u64,
    ) {
        if let Some(existing) = existing {
            let order = existing.part.order;
            part.order = order;
            self.part_orders.insert(part.part_id.clone(), order);
            self.seed_next_order(&part.message_id, durable_next_order.max(order + 1));
            return;
        }

        if let Some(order) = self.part_orders.get(&part.part_id).copied() {
            part.order = order;
            self.seed_next_order(&part.message_id, durable_next_order);
            return;
        }

        let next_order = self
            .next_orders_by_message
            .entry(part.message_id.clone())
            .and_modify(|order| *order = (*order).max(durable_next_order))
            .or_insert(durable_next_order);
        part.order = *next_order;
        *next_order += 1;
        self.part_orders.insert(part.part_id.clone(), part.order);
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

    fn seed_next_order(&mut self, message_id: &str, minimum_next_order: u64) {
        self.next_orders_by_message
            .entry(message_id.to_string())
            .and_modify(|order| *order = (*order).max(minimum_next_order))
            .or_insert(minimum_next_order);
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
        let mut actor = TurnTimelineActor::default();
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
    fn snapshot_order_allocates_unique_orders_inside_message_scope() {
        let mut actor = TurnTimelineActor::default();
        let mut first = part("part-1", StudioPartStatus::Started, 0);
        let mut second = part("part-2", StudioPartStatus::Started, 0);
        let mut third = part("part-3", StudioPartStatus::Started, 0);

        actor.prepare_snapshot_order(&mut first, None, 5);
        actor.prepare_snapshot_order(&mut second, None, 5);
        actor.prepare_snapshot_order(&mut third, None, 10);

        assert_eq!(first.order, 5);
        assert_eq!(second.order, 6);
        assert_eq!(third.order, 10);
    }

    #[test]
    fn snapshot_order_reuses_live_allocation_for_repeated_part() {
        let mut actor = TurnTimelineActor::default();
        let mut first = part("part-1", StudioPartStatus::Started, 0);
        actor.prepare_snapshot_order(&mut first, None, 7);

        let mut repeat = part("part-1", StudioPartStatus::Streaming, 1);
        actor.prepare_snapshot_order(&mut repeat, None, 7);

        assert_eq!(repeat.order, 7);
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
