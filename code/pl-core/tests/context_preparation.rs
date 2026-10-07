mod support;

use pl_core::{
    context::{ContextContent, ContextSource},
    model::DynModelSession,
    thread::{
        RuntimeFact, ThreadHandle,
        context_preparation::{
            ContextPreparation, ContextPreparationHook, ContextPreparationRequest, ContextPreparer,
        },
    },
};
use std::sync::{Arc, Mutex};
use support::{ScriptedModel, text, turn};

struct CapacityModel {
    observed: Arc<Mutex<Vec<pl_core::context::ContextSnapshot>>>,
    dispatched: Arc<std::sync::atomic::AtomicUsize>,
    always_exceeds: bool,
}
impl pl_core::model::ModelSession for CapacityModel {
    async fn prepare(
        &mut self,
        request: pl_core::model::ModelRequest,
    ) -> Result<pl_core::model::PreparedModelCall, pl_core::model::ModelError> {
        self.observed.lock().unwrap().push(request.context.clone());
        if self.always_exceeds
            || request
                .context
                .records
                .iter()
                .any(|record| record.id == "large-history")
        {
            return Err(pl_core::model::ModelError {
                kind: pl_core::model::ModelFailureKind::ContextLimit,
                details: None,
                usage: Default::default(),
                source: None,
            });
        }
        let dispatched = self.dispatched.clone();
        Ok(pl_core::model::PreparedModelCall::new(async move {
            dispatched.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            Ok(support::response(&request, vec![]))
        }))
    }
    async fn close(&mut self) -> Result<(), pl_core::model::ModelError> {
        Ok(())
    }
}
#[derive(Debug)]
struct CapacityRecovery(Arc<Mutex<Vec<bool>>>);
impl ContextPreparationHook for CapacityRecovery {
    async fn before_step(&self, request: ContextPreparationRequest) -> ContextPreparation {
        self.0.lock().unwrap().push(request.force_compaction);
        if !request.force_compaction {
            return ContextPreparation::Unchanged;
        }
        assert!(
            request
                .committed_context
                .records
                .iter()
                .all(|record| record.source != ContextSource::User)
        );
        ContextPreparation::Prepared {
            expected_extension_sequence: request.extension_sequence,
            rejected_mutations: vec![],
            replacement: Some(pl_core::thread::ReplaceContext {
                expected_revision: request.committed_context.revision,
                reason: pl_core::thread::ContextReplacementReason::Compaction,
                records: vec![pl_core::context::ContextRecord {
                    id: "reduced".into(),
                    turn_id: None,
                    source: ContextSource::Runtime {
                        source_id: "summary".into(),
                    },
                    content: vec![text("completed history")],
                    tool_calls: vec![],
                }],
            }),
            facts: request.current_facts.to_vec(),
            mutations: vec![],
        }
    }
}

#[tokio::test]
async fn preparation_overflow_recovers_once_before_dispatch_and_consumes_input_once() {
    for always_exceeds in [false, true] {
        let observed = Arc::new(Mutex::new(Vec::new()));
        let dispatched = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let preparations = Arc::new(Mutex::new(Vec::new()));
        let thread = ThreadHandle::start(
            format!("capacity-{always_exceeds}"),
            DynModelSession::new(CapacityModel {
                observed: observed.clone(),
                dispatched: dispatched.clone(),
                always_exceeds,
            }),
        )
        .unwrap();
        thread
            .replace_context(pl_core::thread::ReplaceContext {
                expected_revision: 0,
                reason: pl_core::thread::ContextReplacementReason::Rebuild,
                records: vec![pl_core::context::ContextRecord {
                    id: "large-history".into(),
                    turn_id: None,
                    source: ContextSource::Runtime {
                        source_id: "history".into(),
                    },
                    content: vec![text("oversized committed history")],
                    tool_calls: vec![],
                }],
            })
            .await
            .unwrap();
        thread
            .set_context_preparation(Some(ContextPreparer::new(CapacityRecovery(
                preparations.clone(),
            ))))
            .await
            .unwrap();
        let result = thread.run_turn(turn("single-input")).await;
        assert_eq!(result.is_err(), always_exceeds);
        assert_eq!(*preparations.lock().unwrap(), [false, true]);
        {
            let requests = observed.lock().unwrap();
            assert_eq!(requests.len(), 2);
            for request in requests.iter() {
                assert_eq!(
                    request
                        .records
                        .iter()
                        .filter(|record| record.source == ContextSource::User)
                        .count(),
                    1
                );
            }
        }
        assert_eq!(
            dispatched.load(std::sync::atomic::Ordering::Relaxed),
            usize::from(!always_exceeds)
        );
        thread.close().await.unwrap();
    }
}

#[tokio::test]
async fn independent_sessions_keep_facts_inputs_extensions_and_checkpoints_private() {
    use pl_core::{context::OpaquePayload, thread::extensions::ExtensionMutation};
    let mut sessions = Vec::new();
    for marker in ["root-a", "root-b", "parent", "child-a", "child-b"] {
        let (model, requests) = ScriptedModel::new(&[]);
        let thread = ThreadHandle::start(marker.into(), DynModelSession::new(model)).unwrap();
        thread
            .update_facts(vec![RuntimeFact {
                source_id: "workspace".into(),
                content: vec![text(marker)],
            }])
            .await
            .unwrap();
        thread
            .mutate_extensions(vec![ExtensionMutation::Put {
                id: "state".into(),
                expected_revision: None,
                payload: OpaquePayload::text(marker),
            }])
            .await
            .unwrap();
        let mut input = turn(marker);
        input.content = vec![text(marker)];
        thread.run_turn(input).await.unwrap();
        let checkpoint = thread
            .checkpoint(thread.snapshot().commit_sequence)
            .unwrap();
        thread.close().await.unwrap();
        let restored = ThreadHandle::resume(
            marker.into(),
            DynModelSession::new(ScriptedModel::new(&[]).0),
            Some(checkpoint),
        )
        .unwrap();
        assert_eq!(
            restored.snapshot().extensions["state"].payload.content(),
            marker
        );
        let encoded = serde_json::to_string(&restored.snapshot().context).unwrap();
        for other in ["root-a", "root-b", "parent", "child-a", "child-b"] {
            assert_eq!(encoded.contains(other), other == marker);
        }
        assert!(
            serde_json::to_string(&requests.lock().unwrap()[0])
                .unwrap()
                .contains(marker)
        );
        sessions.push(restored);
    }
    for session in sessions {
        session.close().await.unwrap();
    }
}

#[tokio::test]
async fn changed_facts_append_without_mutating_observed_history_and_clear_explicitly() {
    let (model, observed) = ScriptedModel::new(&[]);
    let thread = ThreadHandle::start("facts".into(), DynModelSession::new(model)).unwrap();
    let fact = |value: &str| RuntimeFact {
        source_id: "host.state".into(),
        content: vec![text(value)],
    };
    thread
        .update_facts(vec![fact("approved version one")])
        .await
        .unwrap();
    thread.run_turn(turn("first")).await.unwrap();
    let first = observed.lock().unwrap()[0].records[0].clone();
    thread
        .update_facts(vec![fact("executing version two")])
        .await
        .unwrap();
    let second = thread.snapshot().context;
    assert_eq!(
        second.records[0], first,
        "an already observed prefix must remain unchanged"
    );
    let before = second.clone();
    thread
        .update_facts(vec![fact("executing version two")])
        .await
        .unwrap();
    assert_eq!(
        thread.snapshot().context,
        before,
        "identical facts must not append"
    );
    thread.update_facts(vec![]).await.unwrap();
    let cleared = thread.snapshot().context;
    assert_eq!(cleared.records[0], first);
    assert!(
        matches!(&cleared.records.last().unwrap().content[0], ContextContent::Text { text } if text.contains("no longer current"))
    );
    thread.close().await.unwrap();
}

#[derive(Debug)]
struct ObservePreview(Arc<Mutex<Vec<pl_core::context::ContextSnapshot>>>);

#[derive(Debug)]
struct RestoreFactAuthority;
impl ContextPreparationHook for RestoreFactAuthority {
    async fn before_step(&self, request: ContextPreparationRequest) -> ContextPreparation {
        ContextPreparation::Prepared {
            expected_extension_sequence: request.extension_sequence,
            rejected_mutations: vec![],
            replacement: None,
            facts: vec![RuntimeFact {
                source_id: "approved-state".into(),
                content: vec![text("approved")],
            }],
            mutations: vec![],
        }
    }
}

#[tokio::test]
async fn preparation_restores_fact_authority_even_when_the_snapshot_already_exists() {
    let (model, _) = ScriptedModel::new(&[]);
    let thread =
        ThreadHandle::start("restore-fact-authority".into(), DynModelSession::new(model)).unwrap();
    thread
        .replace_context(pl_core::thread::ReplaceContext {
            expected_revision: 0,
            reason: pl_core::thread::ContextReplacementReason::Rebuild,
            records: vec![pl_core::context::ContextRecord {
                id: "restored-state".into(),
                turn_id: None,
                source: ContextSource::RuntimeFact {
                    source_id: "approved-state".into(),
                },
                content: vec![text("approved")],
                tool_calls: vec![],
            }],
        })
        .await
        .unwrap();
    thread
        .set_context_preparation(Some(ContextPreparer::new(RestoreFactAuthority)))
        .await
        .unwrap();
    thread.run_turn(turn("resume-approved-task")).await.unwrap();
    assert_eq!(thread.snapshot().runtime_facts.len(), 1);
    let state = thread.snapshot();
    thread
        .replace_context(pl_core::thread::ReplaceContext {
            expected_revision: state.context.revision,
            reason: pl_core::thread::ContextReplacementReason::Compaction,
            records: vec![],
        })
        .await
        .unwrap();
    assert_eq!(thread.snapshot().context.records.len(), 1);
    assert_eq!(
        thread.snapshot().context.records[0].content,
        vec![text("approved")]
    );
    thread.close().await.unwrap();
}

impl ContextPreparationHook for ObservePreview {
    async fn before_step(&self, request: ContextPreparationRequest) -> ContextPreparation {
        self.0.lock().unwrap().push(request.model.context);
        ContextPreparation::Unchanged
    }
}

#[tokio::test]
async fn preparation_capacity_preview_includes_the_unconsumed_user_input() {
    let (model, observed) = ScriptedModel::new(&[]);
    let thread = ThreadHandle::start("preview".into(), DynModelSession::new(model)).unwrap();
    let previews = Arc::new(Mutex::new(Vec::new()));
    thread
        .set_context_preparation(Some(ContextPreparer::new(ObservePreview(previews.clone()))))
        .await
        .unwrap();
    thread.run_turn(turn("new-task")).await.unwrap();
    let preview = previews.lock().unwrap()[0].clone();
    assert!(
        preview
            .records
            .iter()
            .any(|r| r.source == ContextSource::User)
    );
    assert_eq!(
        preview,
        observed.lock().unwrap()[0],
        "the capacity preview must match the admitted request"
    );
    thread.close().await.unwrap();
}

#[derive(Debug)]
struct CancelCandidate;
impl ContextPreparationHook for CancelCandidate {
    async fn before_step(&self, request: ContextPreparationRequest) -> ContextPreparation {
        use pl_core::{context::OpaquePayload, thread::extensions::ExtensionMutation};
        let put = |id: &str| ExtensionMutation::Put {
            id: id.into(),
            expected_revision: None,
            payload: OpaquePayload::text(id),
        };
        request.model.cancellation.cancel();
        let mut cost = put("observed.cost");
        if let ExtensionMutation::Put {
            expected_revision, ..
        } = &mut cost
        {
            *expected_revision = Some(99);
        }
        ContextPreparation::Prepared {
            expected_extension_sequence: request.extension_sequence,
            rejected_mutations: vec![cost],
            replacement: None,
            facts: vec![RuntimeFact {
                source_id: "state".into(),
                content: vec![text("wrong advance")],
            }],
            mutations: vec![put("success.receipt")],
        }
    }
}

#[tokio::test]
async fn cancelled_candidate_keeps_accounting_without_committing_state_or_success() {
    let (model, observed) = ScriptedModel::new(&[]);
    let thread =
        ThreadHandle::start("cancel-candidate".into(), DynModelSession::new(model)).unwrap();
    thread
        .update_facts(vec![RuntimeFact {
            source_id: "state".into(),
            content: vec![text("approved")],
        }])
        .await
        .unwrap();
    let before = thread.snapshot().context;
    thread
        .set_context_preparation(Some(ContextPreparer::new(CancelCandidate)))
        .await
        .unwrap();
    assert!(thread.run_turn(turn("cancelled")).await.is_err());
    let snapshot = thread.snapshot();
    assert_eq!(snapshot.context, before);
    assert!(snapshot.extensions.contains_key("observed.cost"));
    assert!(!snapshot.extensions.contains_key("success.receipt"));
    assert!(observed.lock().unwrap().is_empty());
    thread.close().await.unwrap();
}

#[derive(Debug)]
struct ConflictOnce(std::sync::atomic::AtomicUsize);
impl ContextPreparationHook for ConflictOnce {
    async fn before_step(&self, request: ContextPreparationRequest) -> ContextPreparation {
        let first = self.0.fetch_add(1, std::sync::atomic::Ordering::Relaxed) == 0;
        ContextPreparation::Prepared {
            expected_extension_sequence: request.extension_sequence + u64::from(first),
            rejected_mutations: vec![],
            replacement: None,
            facts: vec![RuntimeFact {
                source_id: "state".into(),
                content: vec![text(if first { "stale" } else { "current approved" })],
            }],
            mutations: vec![],
        }
    }
}

#[tokio::test]
async fn a_conflict_refreezes_once_without_admitting_stale_state_or_duplicate_input() {
    let (model, observed) = ScriptedModel::new(&[]);
    let thread = ThreadHandle::start("conflict".into(), DynModelSession::new(model)).unwrap();
    thread
        .set_context_preparation(Some(ContextPreparer::new(ConflictOnce(Default::default()))))
        .await
        .unwrap();
    thread.run_turn(turn("retry-preparation")).await.unwrap();
    {
        let requests = observed.lock().unwrap();
        assert_eq!(requests.len(), 1);
        assert_eq!(
            requests[0]
                .records
                .iter()
                .filter(|record| record.source == ContextSource::User)
                .count(),
            1
        );
        let encoded = serde_json::to_string(&requests[0]).unwrap();
        assert!(encoded.contains("current approved"));
        assert!(!encoded.contains("stale"));
    }
    thread.close().await.unwrap();
}

#[derive(Debug)]
struct FrozenMailbox {
    first: std::sync::atomic::AtomicBool,
    started: Arc<tokio::sync::Notify>,
    release: Arc<tokio::sync::Notify>,
}
impl ContextPreparationHook for FrozenMailbox {
    async fn before_step(&self, _: ContextPreparationRequest) -> ContextPreparation {
        if self.first.swap(false, std::sync::atomic::Ordering::Relaxed) {
            self.started.notify_one();
            self.release.notified().await;
        }
        ContextPreparation::Unchanged
    }
}

#[tokio::test]
async fn messages_arriving_during_preparation_remain_for_the_next_request() {
    let (model, observed) = ScriptedModel::new(&[]);
    let thread = ThreadHandle::start("watermark".into(), DynModelSession::new(model)).unwrap();
    let started = Arc::new(tokio::sync::Notify::new());
    let release = Arc::new(tokio::sync::Notify::new());
    thread
        .set_context_preparation(Some(ContextPreparer::new(FrozenMailbox {
            first: std::sync::atomic::AtomicBool::new(true),
            started: started.clone(),
            release: release.clone(),
        })))
        .await
        .unwrap();
    let running = thread.clone();
    let task = tokio::spawn(async move { running.run_turn(turn("first")).await });
    started.notified().await;
    let sequence = thread
        .send_message(pl_core::thread::inbox::ThreadMessage {
            kind: pl_core::context::AgentMessageKind::Report,
            id: "late-report".into(),
            source_id: "agent:child".into(),
            payload: pl_core::context::OpaquePayload::text("explicit report"),
            context: vec![text("LATE_CHILD_REPORT")],
        })
        .await
        .unwrap();
    release.notify_one();
    task.await.unwrap().unwrap();
    assert_eq!(thread.snapshot().consumed_messages, 0);
    assert!(
        !serde_json::to_string(&observed.lock().unwrap()[0])
            .unwrap()
            .contains("LATE_CHILD_REPORT")
    );
    thread.run_turn(turn("second")).await.unwrap();
    assert_eq!(thread.snapshot().consumed_messages, sequence);
    {
        let requests = observed.lock().unwrap();
        assert_eq!(
            serde_json::to_string(&requests[1])
                .unwrap()
                .matches("LATE_CHILD_REPORT")
                .count(),
            1
        );
    }
    thread.close().await.unwrap();
}
