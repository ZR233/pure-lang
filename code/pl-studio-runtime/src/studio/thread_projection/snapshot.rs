//! Authoritative product snapshots derived from one immutable core watermark.
use super::{ProjectionError, project_items, project_turns};
use pl_core::thread::{
    ThreadLifecycle, ThreadSnapshot, TurnState, input::InputState, journal::ThreadCommit,
};
use pl_protocol::{Thread, ThreadStatus};
use std::sync::Arc;

pub(in crate::studio) fn project_snapshot(
    mut thread: Thread,
    state: &ThreadSnapshot,
    journal: &[Arc<ThreadCommit>],
) -> Result<pl_protocol::ThreadSnapshot, ProjectionError> {
    let mut items = project_items(&thread.id, state, journal)?;
    let turns = project_turns(&thread.id, state, journal)?;
    let active_turn = turns.into_iter().rev().find(|turn| {
        state
            .turns
            .iter()
            .any(|record| record.turn_id == turn.id && record.state == TurnState::Running)
    });
    let mut interactions = Vec::new();
    for id in state.permissions.keys().chain(state.interactions.keys()) {
        match crate::thread_assembler::project_thread_interaction(&thread.id, id, state) {
            Ok(Some(interaction)) => interactions.push(interaction),
            Ok(None) => {}
            Err(error) => {
                let payloads = state
                    .permissions
                    .get(id)
                    .map(|record| &record.payload)
                    .or_else(|| {
                        state
                            .interactions
                            .get(id)
                            .map(|record| &record.request.payload)
                    })
                    .into_iter()
                    .map(super::raw_payload)
                    .collect();
                append_raw_notice(
                    &mut items,
                    &thread,
                    RawNotice {
                        id: format!("raw-interaction:{id}"),
                        payloads,
                        notice: error.to_string(),
                        revision: state.commit_sequence,
                        recorded_at: journal.last().map_or(0, |commit| commit.committed_at),
                    },
                );
            }
        }
    }
    thread.status = status(state);
    match saved_mode(state) {
        Ok(Some(mode)) => thread.mode = mode,
        Ok(None) => {}
        Err(error) => append_raw_notice(
            &mut items,
            &thread,
            RawNotice {
                id: "raw-mode".into(),
                payloads: state
                    .extensions
                    .get("studio.mode")
                    .map(|record| super::raw_payload(&record.payload))
                    .into_iter()
                    .collect(),
                notice: error.to_string(),
                revision: state.commit_sequence,
                recorded_at: journal.last().map_or(0, |commit| commit.committed_at),
            },
        ),
    }
    let runtime = match super::runtime::project_runtime(&thread.id, state, journal) {
        Ok(runtime) => Some(runtime),
        Err(error) => {
            let recorded_at = journal
                .last()
                .map_or(thread.updated_at, |commit| commit.committed_at);
            let mut payloads = state
                .extensions
                .values()
                .map(|record| super::raw_payload(&record.payload))
                .collect::<Vec<_>>();
            for attempt in state.attempts.iter() {
                if let Some(payload) = &attempt.request_metadata {
                    payloads.push(super::raw_payload(payload));
                }
            }
            items.push(pl_protocol::ThreadItem::new(
                format!("raw-runtime:{}", state.commit_sequence),
                thread.id.clone(),
                String::new(),
                items
                    .last()
                    .map_or(1, |item| item.ordinal.saturating_add(1)),
                state.commit_sequence,
                recorded_at,
                recorded_at,
                pl_protocol::ThreadItemState::Raw(pl_protocol::ThreadRawItem {
                    payloads,
                    notice: error.to_string(),
                    recorded_at,
                }),
            ));
            None
        }
    };
    Ok(pl_protocol::ThreadSnapshot {
        schema_version: pl_protocol::THREAD_SCHEMA_VERSION,
        revision: state.commit_sequence,
        runtime,
        thread,
        active_turn,
        items,
        interactions,
    })
}

struct RawNotice {
    id: String,
    payloads: Vec<pl_protocol::ThreadRawPayload>,
    notice: String,
    revision: u64,
    recorded_at: i64,
}
fn append_raw_notice(items: &mut Vec<pl_protocol::ThreadItem>, thread: &Thread, record: RawNotice) {
    items.push(pl_protocol::ThreadItem::new(
        record.id,
        thread.id.clone(),
        String::new(),
        items
            .last()
            .map_or(1, |item| item.ordinal.saturating_add(1)),
        record.revision,
        record.recorded_at,
        record.recorded_at,
        pl_protocol::ThreadItemState::Raw(pl_protocol::ThreadRawItem {
            payloads: record.payloads,
            notice: record.notice,
            recorded_at: record.recorded_at,
        }),
    ));
}

pub(in crate::studio) fn status(state: &ThreadSnapshot) -> ThreadStatus {
    match state.lifecycle {
        ThreadLifecycle::Closing => ThreadStatus::Closing,
        ThreadLifecycle::Closed => ThreadStatus::Closed,
        ThreadLifecycle::Open => {
            if state.interactions.values().any(|record| {
                record.state == pl_core::thread::interactions::InteractionState::Pending
            }) || state.permissions.values().any(|record| {
                record.state == pl_core::thread::permissions::PermissionState::Pending
            }) {
                ThreadStatus::WaitingInteraction
            } else if state
                .turns
                .iter()
                .any(|turn| turn.state == TurnState::Running)
            {
                ThreadStatus::Running
            } else if state
                .inputs
                .iter()
                .any(|input| input.state == InputState::Pending)
            {
                ThreadStatus::Queued
            } else {
                ThreadStatus::Idle
            }
        }
    }
}

pub(in crate::studio) fn saved_mode(
    state: &ThreadSnapshot,
) -> Result<Option<pl_protocol::ThreadModeId>, ProjectionError> {
    state
        .extensions
        .get("studio.mode")
        .map(|record| {
            if record.payload.format() != "pl.studio.mode" || record.payload.version() != 1 {
                return Err(ProjectionError::UnsupportedOutput(
                    "unsupported saved Mode".into(),
                ));
            }
            Ok(serde_json::from_str(record.payload.content())?)
        })
        .transpose()
}

#[cfg(test)]
mod tests {
    use super::*;
    use pl_core::{
        context::{ContextContent, OpaquePayload},
        model::{
            DynModelSession, ModelError, ModelRequest, ModelSession, ModelStepOutput,
            PreparedModelCall,
        },
        thread::{
            ThreadHandle,
            input::{QueuedTurn, ThreadInput},
        },
    };
    use pretty_assertions::assert_eq;

    struct Reply;
    impl ModelSession for Reply {
        async fn prepare(
            &mut self,
            request: ModelRequest,
        ) -> Result<PreparedModelCall, ModelError> {
            Ok(PreparedModelCall::new(async move {
                Ok(ModelStepOutput {
                    attempt_id: request.attempt_id,
                    base_context_revision: request.context.revision,
                    content: vec![ContextContent::Text {
                        text: Arc::from("  answer\r\n"),
                    }],
                    tool_calls: Vec::new(),
                    private_context: None,
                    usage: pl_core::model::ModelUsage {
                        input_tokens: Some(10),
                        output_tokens: Some(4),
                        cache_read_tokens: Some(8),
                        ..Default::default()
                    },
                })
            }))
        }
        async fn close(&mut self) -> Result<(), ModelError> {
            Ok(())
        }
    }

    #[tokio::test]
    async fn input_turn_and_response_projection_matches_cold_replay_without_calling_current_services()
     {
        let handle = ThreadHandle::start("thread".into(), DynModelSession::new(Reply)).unwrap();
        handle.submit_input(ThreadInput {
            id: "input".into(),
            payload: OpaquePayload::new("pl.studio.prompt", 1, serde_json::json!({"text":"  question\r\n", "presentation":"visible", "attachments":[]}).to_string()).unwrap(),
            context: vec![ContextContent::Text { text: Arc::from("  question\r\n") }],
        }).await.unwrap();
        handle
            .run_next_input(QueuedTurn {
                turn_id: "turn".into(),
                attempt_prefix: "request".into(),
                max_model_steps: std::num::NonZeroU32::new(2).unwrap(),
                cancellation: Default::default(),
            })
            .await
            .unwrap();
        let state = handle.snapshot();
        let journal = handle.journal().await.unwrap();
        let product = project_snapshot(Thread::placeholder("thread"), &state, &journal).unwrap();
        let restored = pl_core::thread::journal::replay(&journal).unwrap();
        assert_eq!(
            project_snapshot(Thread::placeholder("thread"), &restored, &journal).unwrap(),
            product
        );
        assert_eq!(product.thread.status, ThreadStatus::Idle);
        assert!(product.active_turn.is_none());
        assert_eq!(
            product.items.len(),
            4,
            "input, Turn, inference and response all remain visible"
        );
        assert_eq!(product.items[0].id, "input");
        assert_eq!(product.items[0].turn_id, "turn");
        let pl_protocol::ThreadItemState::Turn(turn) = product.items[1].state() else {
            panic!("Turn item");
        };
        assert_eq!(turn.input_id(), Some("input"));
        assert!(turn.state().is_terminal());
        let pl_protocol::ThreadItemState::Text(text) = product.items[3].state() else {
            panic!("answer item");
        };
        assert_eq!(text.text(), "  answer\r\n");
        let usage = &product.runtime.as_ref().unwrap().usage;
        assert_eq!(usage.prompt_tokens, 10);
        assert_eq!(usage.total_tokens, 14);
        assert_eq!(usage.cache_hit_rate, Some(0.8));
        assert!(usage.has_unpriced_usage);
        handle.close().await.unwrap();
        let closed = project_snapshot(
            Thread::placeholder("thread"),
            &handle.snapshot(),
            &handle.journal().await.unwrap(),
        )
        .unwrap();
        assert_eq!(closed.thread.status, ThreadStatus::Closed);
        assert_eq!(closed.items, product.items);
    }
    #[tokio::test]
    async fn unknown_input_codec_remains_exactly_readable_after_cold_replay() {
        let handle =
            ThreadHandle::start("raw-history".into(), DynModelSession::new(Reply)).unwrap();
        let original = "  {\"n\":9007199254740993123456789}\r\n原文\0<script>literal</script>  ";
        handle
            .submit_input(ThreadInput {
                id: "unknown-input".into(),
                payload: OpaquePayload::new("future.studio.prompt", 99, original).unwrap(),
                context: Vec::new(),
            })
            .await
            .unwrap();
        handle.close().await.unwrap();
        let journal = handle.journal().await.unwrap();
        let replayed = pl_core::thread::journal::replay(&journal).unwrap();
        let product =
            project_snapshot(Thread::placeholder("raw-history"), &replayed, &journal).unwrap();
        assert_eq!(product.items.len(), 1);
        let pl_protocol::ThreadItemState::Raw(raw) = product.items[0].state() else {
            panic!("unknown codecs must remain readable as raw data");
        };
        assert_eq!(
            raw.payloads,
            vec![pl_protocol::ThreadRawPayload {
                format: "future.studio.prompt".into(),
                version: 99,
                content: original.into(),
            }]
        );
        assert!(
            replayed.context.records.is_empty(),
            "opaque input must not become model context"
        );
    }
}
