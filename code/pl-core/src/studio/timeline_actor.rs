use std::collections::HashMap;

use pl_protocol::{StudioPart, StudioPartStatus};

use super::records::StudioPartRecord;

#[derive(Default)]
pub(super) struct TurnTimelineActor {
    part_revisions: HashMap<String, u64>,
    part_orders: HashMap<String, u64>,
    trace_part_ids: HashMap<TracePartScope, String>,
    next_orders_by_message: HashMap<MessageScope, u64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(super) struct TracePartScope {
    session_id: String,
    turn_id: String,
    trace_part_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct MessageScope {
    session_id: String,
    message_id: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum TimelineDeltaDecision {
    Accept,
    Stale,
}

impl TurnTimelineActor {
    pub(super) fn resolve_trace_part_id(
        &self,
        scope: &TracePartScope,
        existing: Option<&StudioPartRecord>,
    ) -> Option<String> {
        existing
            .map(|record| record.part.part_id.clone())
            .or_else(|| self.trace_part_ids.get(scope).cloned())
    }

    pub(super) fn prepare_snapshot_order(
        &mut self,
        part: &mut StudioPart,
        trace_scope: Option<&TracePartScope>,
        existing: Option<&StudioPartRecord>,
        durable_next_order: u64,
    ) {
        if let Some(existing) = existing {
            part.part_id = existing.part.part_id.clone();
            let order = existing.part.order;
            part.order = order;
            self.part_orders.insert(part.part_id.clone(), order);
            self.remember_trace_part_id(trace_scope, &part.part_id);
            self.seed_next_order(part, durable_next_order.max(order + 1));
            return;
        }

        if let Some(studio_part_id) =
            trace_scope.and_then(|trace_scope| self.trace_part_ids.get(trace_scope))
        {
            part.part_id = studio_part_id.clone();
        }

        if let Some(order) = self.part_orders.get(&part.part_id).copied() {
            part.order = order;
            self.remember_trace_part_id(trace_scope, &part.part_id);
            self.seed_next_order(part, durable_next_order);
            return;
        }

        let next_order = self
            .next_orders_by_message
            .entry(MessageScope::for_part(part))
            .and_modify(|order| *order = (*order).max(durable_next_order))
            .or_insert(durable_next_order);
        part.order = *next_order;
        *next_order += 1;
        if should_allocate_part_id(trace_scope, &part.part_id) {
            part.part_id = format!("{}:part-{}", part.turn_id, part.order);
        }
        self.part_orders.insert(part.part_id.clone(), part.order);
        self.remember_trace_part_id(trace_scope, &part.part_id);
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
        part_id: &str,
        revision: u64,
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
            .get(part_id)
            .copied()
            .unwrap_or(existing.part.revision);
        if revision == current_revision + 1 {
            self.part_revisions.insert(part_id.to_string(), revision);
            TimelineDeltaDecision::Accept
        } else {
            TimelineDeltaDecision::Stale
        }
    }

    fn seed_next_order(&mut self, part: &StudioPart, minimum_next_order: u64) {
        self.next_orders_by_message
            .entry(MessageScope::for_part(part))
            .and_modify(|order| *order = (*order).max(minimum_next_order))
            .or_insert(minimum_next_order);
    }

    fn remember_trace_part_id(&mut self, trace_scope: Option<&TracePartScope>, part_id: &str) {
        if let Some(trace_scope) = trace_scope {
            self.trace_part_ids
                .insert(trace_scope.clone(), part_id.to_string());
        }
    }
}

impl TracePartScope {
    pub(super) fn new(session_id: &str, turn_id: &str, trace_part_id: &str) -> Self {
        Self {
            session_id: session_id.to_string(),
            turn_id: turn_id.to_string(),
            trace_part_id: trace_part_id.to_string(),
        }
    }
}

impl MessageScope {
    fn for_part(part: &StudioPart) -> Self {
        Self {
            session_id: part.session_id.clone(),
            message_id: part.message_id.clone(),
        }
    }
}

fn should_allocate_part_id(trace_scope: Option<&TracePartScope>, part_id: &str) -> bool {
    trace_scope.is_some_and(|scope| scope.trace_part_id == part_id)
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

    use super::*;

    #[test]
    fn accepts_only_contiguous_live_delta_revisions() {
        let mut actor = TurnTimelineActor::default();
        let existing = StudioPartRecord {
            part: part("part-1", StudioPartStatus::Streaming, 0),
            sequence: 0,
        };

        assert_eq!(
            actor.prepare_delta("part-1", 1, Some(&existing)),
            TimelineDeltaDecision::Accept
        );
        assert_eq!(
            actor.prepare_delta("part-1", 1, Some(&existing)),
            TimelineDeltaDecision::Stale
        );
        assert_eq!(
            actor.prepare_delta("part-1", 3, Some(&existing)),
            TimelineDeltaDecision::Stale
        );
        assert_eq!(
            actor.prepare_delta("part-1", 2, Some(&existing)),
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
        let _ = actor.prepare_delta("part-1", 1, Some(&existing));
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
        actor.prepare_snapshot_order(&mut repeat, None, Some(&existing), 42);
        assert_eq!(repeat.order, 7);

        let mut new_part = part("part-2", StudioPartStatus::Started, 0);
        actor.prepare_snapshot_order(&mut new_part, None, None, 42);
        assert_eq!(new_part.order, 42);
    }

    #[test]
    fn snapshot_order_allocates_unique_orders_inside_message_scope() {
        let mut actor = TurnTimelineActor::default();
        let mut first = part("part-1", StudioPartStatus::Started, 0);
        let mut second = part("part-2", StudioPartStatus::Started, 0);
        let mut third = part("part-3", StudioPartStatus::Started, 0);

        actor.prepare_snapshot_order(&mut first, None, None, 5);
        actor.prepare_snapshot_order(&mut second, None, None, 5);
        actor.prepare_snapshot_order(&mut third, None, None, 10);

        assert_eq!(first.order, 5);
        assert_eq!(second.order, 6);
        assert_eq!(third.order, 10);
    }

    #[test]
    fn snapshot_order_reuses_live_allocation_for_repeated_part() {
        let mut actor = TurnTimelineActor::default();
        let mut first = part("part-1", StudioPartStatus::Started, 0);
        actor.prepare_snapshot_order(&mut first, None, None, 7);

        let mut repeat = part("part-1", StudioPartStatus::Streaming, 1);
        actor.prepare_snapshot_order(&mut repeat, None, None, 7);

        assert_eq!(repeat.order, 7);
    }

    #[test]
    fn snapshot_order_allocates_actor_part_id_for_trace_item() {
        let mut actor = TurnTimelineActor::default();
        let trace_scope = scope("session-1", "turn-1", "provider-text-1");
        let mut first = part("provider-text-1", StudioPartStatus::Started, 0);
        actor.prepare_snapshot_order(&mut first, Some(&trace_scope), None, 3);

        assert_eq!(first.part_id, "turn-1:part-3");
        assert_eq!(first.order, 3);
        assert_eq!(
            actor.resolve_trace_part_id(&trace_scope, None).as_deref(),
            Some("turn-1:part-3")
        );

        let mut repeat = part("provider-text-1", StudioPartStatus::Streaming, 1);
        actor.prepare_snapshot_order(&mut repeat, Some(&trace_scope), None, 3);

        assert_eq!(repeat.part_id, "turn-1:part-3");
        assert_eq!(repeat.order, 3);
    }

    #[test]
    fn trace_part_identity_is_scoped_by_session_and_turn() {
        let mut actor = TurnTimelineActor::default();
        let first_scope = scope("session-1", "turn-1", "provider-text-1");
        let second_scope = scope("session-1", "turn-2", "provider-text-1");
        let third_scope = scope("session-2", "turn-1", "provider-text-1");
        let mut first = part("provider-text-1", StudioPartStatus::Started, 0);
        actor.prepare_snapshot_order(&mut first, Some(&first_scope), None, 3);

        let mut second = part("provider-text-1", StudioPartStatus::Started, 0);
        second.turn_id = "turn-2".to_string();
        second.message_id = "turn-2:assistant".to_string();
        actor.prepare_snapshot_order(&mut second, Some(&second_scope), None, 0);

        let mut third = part("provider-text-1", StudioPartStatus::Started, 0);
        third.session_id = "session-2".to_string();
        actor.prepare_snapshot_order(&mut third, Some(&third_scope), None, 0);

        assert_eq!(
            actor.resolve_trace_part_id(&first_scope, None).as_deref(),
            Some("turn-1:part-3")
        );
        assert_eq!(
            actor.resolve_trace_part_id(&second_scope, None).as_deref(),
            Some("turn-2:part-0")
        );
        assert_eq!(
            actor.resolve_trace_part_id(&third_scope, None).as_deref(),
            Some("turn-1:part-0")
        );
    }

    #[test]
    fn terminal_parts_reject_late_deltas() {
        let mut actor = TurnTimelineActor::default();
        let existing = StudioPartRecord {
            part: part("part-1", StudioPartStatus::Completed, 1),
            sequence: 0,
        };

        assert_eq!(
            actor.prepare_delta("part-1", 2, Some(&existing)),
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

    fn scope(session_id: &str, turn_id: &str, trace_part_id: &str) -> TracePartScope {
        TracePartScope::new(session_id, turn_id, trace_part_id)
    }
}
