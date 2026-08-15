use std::collections::{BTreeMap, BTreeSet};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use pl_protocol::{
    ConversationRecoveryMode, InteractionKind, InteractionPayload, InteractionRequest,
    InteractionResolution, InteractionScope, InteractionStatus, ThreadNotification,
    TurnBillingRecord,
};
use pretty_assertions::assert_eq;
use tokio::sync::Notify;

use super::host::ThreadContextMutation;
use super::*;
use crate::{AgentSession, Message};

#[derive(Debug, Clone, thiserror::Error)]
#[error("{0}")]
struct TestError(String);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FactoryMode {
    Fail,
    Block,
}

#[derive(Clone)]
struct TestRepository {
    states: Arc<Mutex<BTreeMap<AgentId, ThreadActorState>>>,
    mutations: Arc<Mutex<Vec<ThreadMutation>>>,
    contexts: Arc<Mutex<Vec<Option<ThreadContextMutation>>>>,
    submissions: Arc<Mutex<BTreeMap<AgentId, Vec<AgentSubmissionRecord>>>>,
    fail_trace: Arc<Mutex<bool>>,
    fail_terminal: Arc<Mutex<bool>>,
    fail_registration: Arc<Mutex<bool>>,
    fail_lifecycle: Arc<Mutex<Option<AgentLifecycleState>>>,
    fail_fault: Arc<Mutex<bool>>,
    fail_turn_queue: Arc<Mutex<bool>>,
    fail_turn_started: Arc<Mutex<bool>>,
}

impl TestRepository {
    fn empty() -> Self {
        Self {
            states: Arc::new(Mutex::new(BTreeMap::new())),
            mutations: Arc::new(Mutex::new(Vec::new())),
            contexts: Arc::new(Mutex::new(Vec::new())),
            submissions: Arc::new(Mutex::new(BTreeMap::new())),
            fail_trace: Arc::new(Mutex::new(false)),
            fail_terminal: Arc::new(Mutex::new(false)),
            fail_registration: Arc::new(Mutex::new(false)),
            fail_lifecycle: Arc::new(Mutex::new(None)),
            fail_fault: Arc::new(Mutex::new(false)),
            fail_turn_queue: Arc::new(Mutex::new(false)),
            fail_turn_started: Arc::new(Mutex::new(false)),
        }
    }

    fn with_state(state: ThreadActorState) -> Self {
        let repository = Self::empty();
        repository
            .states
            .lock()
            .unwrap()
            .insert(state.snapshot.identity.id.clone(), state);
        repository
    }

    fn fail_terminal_commits(&self) {
        *self.fail_terminal.lock().unwrap() = true;
    }

    fn fail_next_registration(&self) {
        *self.fail_registration.lock().unwrap() = true;
    }

    fn fail_next_lifecycle_commit(&self, lifecycle: AgentLifecycleState) {
        *self.fail_lifecycle.lock().unwrap() = Some(lifecycle);
    }

    fn fail_next_turn_started_commit(&self) {
        *self.fail_turn_started.lock().unwrap() = true;
    }

    fn fail_next_fault_commit(&self) {
        *self.fail_fault.lock().unwrap() = true;
    }

    fn fail_next_turn_queue_commit(&self) {
        *self.fail_turn_queue.lock().unwrap() = true;
    }

    fn state(&self, id: &AgentId) -> ThreadActorState {
        self.states.lock().unwrap()[id].clone()
    }

    fn last_context(&self) -> Option<ThreadContextMutation> {
        self.contexts.lock().unwrap().last().cloned().flatten()
    }
}

impl ThreadRepository for TestRepository {
    type Error = TestError;

    async fn restore_runtime(&self) -> std::result::Result<Vec<RestoredAgentRuntime>, Self::Error> {
        Ok(self
            .states
            .lock()
            .unwrap()
            .values()
            .cloned()
            .map(|state| RestoredAgentRuntime {
                state,
                thread_snapshot: None,
            })
            .collect())
    }

    async fn restore_thread(
        &self,
        thread_id: &ThreadId,
    ) -> std::result::Result<Option<RestoredAgentRuntime>, Self::Error> {
        Ok(self
            .states
            .lock()
            .unwrap()
            .get(&thread_id.clone())
            .cloned()
            .map(|state| RestoredAgentRuntime {
                state,
                thread_snapshot: None,
            }))
    }

    async fn commit(
        &self,
        commit: ThreadCommit,
    ) -> std::result::Result<ThreadCommitOutcome, Self::Error> {
        if commit.expected_revision.is_none()
            && std::mem::take(&mut *self.fail_registration.lock().unwrap())
        {
            return Err(TestError("registration commit failed".to_string()));
        }
        if !commit.facts.trace_events.is_empty()
            && std::mem::take(&mut *self.fail_trace.lock().unwrap())
        {
            return Err(TestError("trace commit failed".to_string()));
        }
        let should_fail_lifecycle = self
            .fail_lifecycle
            .lock()
            .unwrap()
            .is_some_and(|lifecycle| lifecycle == commit.next_state.snapshot.lifecycle);
        if should_fail_lifecycle {
            self.fail_lifecycle.lock().unwrap().take();
            return Err(TestError("lifecycle commit failed".to_string()));
        }
        if commit
            .facts
            .runtime_events
            .iter()
            .any(|event| matches!(event.kind, AgentRuntimeEventKind::Faulted { .. }))
            && std::mem::take(&mut *self.fail_fault.lock().unwrap())
        {
            return Err(TestError("fault commit failed".to_string()));
        }
        if *self.fail_terminal.lock().unwrap()
            && commit
                .facts
                .runtime_events
                .iter()
                .any(|event| matches!(event.kind, AgentRuntimeEventKind::TurnFinished { .. }))
        {
            return Err(TestError("terminal commit failed".to_string()));
        }
        if commit
            .facts
            .runtime_events
            .iter()
            .any(|event| matches!(event.kind, AgentRuntimeEventKind::TurnQueued { .. }))
            && std::mem::take(&mut *self.fail_turn_queue.lock().unwrap())
        {
            return Err(TestError("turn queue commit failed".to_string()));
        }
        if commit
            .facts
            .runtime_events
            .iter()
            .any(|event| matches!(event.kind, AgentRuntimeEventKind::TurnStarted { .. }))
            && std::mem::take(&mut *self.fail_turn_started.lock().unwrap())
        {
            return Err(TestError("turn started commit failed".to_string()));
        }
        let mut states = self.states.lock().unwrap();
        let actual = states
            .get(&commit.agent_id)
            .map(|state| state.snapshot.revision);
        if actual != commit.expected_revision {
            return Ok(ThreadCommitOutcome::RevisionConflict {
                actual_revision: actual,
            });
        }
        self.mutations.lock().unwrap().push(commit.mutation.clone());
        self.contexts
            .lock()
            .unwrap()
            .push(commit.facts.context.clone());
        if let Some(submission) = commit.facts.submission.as_ref() {
            self.submissions
                .lock()
                .unwrap()
                .entry(commit.agent_id.clone())
                .or_default()
                .push(submission.into());
        }
        states.insert(commit.agent_id, commit.next_state);
        Ok(ThreadCommitOutcome::Applied)
    }

    async fn flush_pending(
        &self,
        _thread_id: Option<&ThreadId>,
    ) -> std::result::Result<(), Self::Error> {
        Ok(())
    }

    fn pending_commit_count(&self) -> usize {
        0
    }

    async fn list_submissions(
        &self,
        thread_id: &ThreadId,
        offset: usize,
        limit: usize,
    ) -> std::result::Result<AgentSubmissionPage, Self::Error> {
        let all = self
            .submissions
            .lock()
            .unwrap()
            .get(thread_id)
            .cloned()
            .unwrap_or_default();
        let total = all.len();
        let limit = limit.max(1);
        let items = all.into_iter().skip(offset).take(limit).collect::<Vec<_>>();
        let returned = items.len();
        Ok(AgentSubmissionPage {
            items,
            offset,
            limit,
            total,
            has_more: offset + returned < total,
        })
    }
}

#[tokio::test]
async fn registration_persists_non_empty_initial_transcript_as_baseline() {
    let repository = TestRepository::empty();
    let host = TestHost::new(repository.clone(), FactoryMode::Fail);
    let runtime = AgentRuntime::start(host, test_options()).await.unwrap();
    let handle = runtime.handle();
    let mut registration = registration("root", "root-chat");
    registration
        .session
        .session
        .push_user_prompt("seed".to_string());

    handle.register(registration).await.unwrap();

    assert!(matches!(
        repository.last_context(),
        Some(ThreadContextMutation::Replace { items }) if items.len() == 1
    ));
    runtime.shutdown().await.unwrap();
}

#[derive(Clone)]
struct TestTurnFactory {
    mode: FactoryMode,
    prepared_messages: Arc<Mutex<Vec<String>>>,
    prepared_batches: Arc<Mutex<Vec<Vec<String>>>>,
    prepared_sessions: Arc<Mutex<Vec<Vec<Message>>>>,
    blocker: Arc<Notify>,
}

impl TestTurnFactory {
    fn new(mode: FactoryMode) -> Self {
        Self {
            mode,
            prepared_messages: Arc::new(Mutex::new(Vec::new())),
            prepared_batches: Arc::new(Mutex::new(Vec::new())),
            prepared_sessions: Arc::new(Mutex::new(Vec::new())),
            blocker: Arc::new(Notify::new()),
        }
    }
}

impl AgentTurnFactory for TestTurnFactory {
    type Error = TestError;

    async fn prepare_turn(
        &self,
        context: AgentTurnPreparationContext,
    ) -> std::result::Result<PreparedAgentTurn, Self::Error> {
        self.prepared_sessions
            .lock()
            .unwrap()
            .push(context.session.messages().to_vec());
        let mut batch = context
            .leading_inputs
            .iter()
            .map(|input| input.payload.message.clone())
            .collect::<Vec<_>>();
        batch.push(context.input.payload.message.clone());
        self.prepared_batches.lock().unwrap().push(batch);
        self.prepared_messages
            .lock()
            .unwrap()
            .push(context.input.payload.message);
        match self.mode {
            FactoryMode::Fail => Err(TestError("prepared turn failed".to_string())),
            FactoryMode::Block => {
                self.blocker.notified().await;
                Err(TestError("blocker released".to_string()))
            }
        }
    }
}

#[derive(Debug, Clone, Default)]
struct TestLifecycle {
    close_order: Arc<Mutex<Vec<AgentId>>>,
    spawn_rollbacks: Arc<Mutex<Vec<AgentId>>>,
    close_rollbacks: Arc<Mutex<Vec<AgentId>>>,
    fail_prepare_spawn: Arc<Mutex<bool>>,
    fail_activate_spawn: Arc<Mutex<bool>>,
    fail_rollback_spawn: Arc<Mutex<bool>>,
    fail_prepare_close: Arc<Mutex<bool>>,
    fail_commit_close: Arc<Mutex<bool>>,
    fail_rollback_close: Arc<Mutex<bool>>,
}

impl TestLifecycle {
    fn fail_next_prepare_spawn(&self) {
        *self.fail_prepare_spawn.lock().unwrap() = true;
    }

    fn fail_next_activate_spawn(&self) {
        *self.fail_activate_spawn.lock().unwrap() = true;
    }

    fn fail_next_rollback_spawn(&self) {
        *self.fail_rollback_spawn.lock().unwrap() = true;
    }

    fn fail_next_prepare_close(&self) {
        *self.fail_prepare_close.lock().unwrap() = true;
    }

    fn fail_next_commit_close(&self) {
        *self.fail_commit_close.lock().unwrap() = true;
    }

    fn fail_next_rollback_close(&self) {
        *self.fail_rollback_close.lock().unwrap() = true;
    }
}

impl AgentLifecycleAdapter for TestLifecycle {
    type Error = TestError;
    type SpawnLease = AgentId;
    type CloseLease = AgentId;

    async fn prepare_spawn(
        &self,
        request: SpawnLifecycleRequest,
    ) -> std::result::Result<Self::SpawnLease, Self::Error> {
        if std::mem::take(&mut *self.fail_prepare_spawn.lock().unwrap()) {
            Err(TestError("prepare spawn failed".to_string()))
        } else {
            Ok(request.child.identity.id)
        }
    }

    async fn activate_spawn(
        &self,
        _lease: &Self::SpawnLease,
    ) -> std::result::Result<(), Self::Error> {
        if std::mem::take(&mut *self.fail_activate_spawn.lock().unwrap()) {
            Err(TestError("activate spawn failed".to_string()))
        } else {
            Ok(())
        }
    }

    async fn rollback_spawn(
        &self,
        lease: Self::SpawnLease,
    ) -> std::result::Result<(), Self::Error> {
        self.spawn_rollbacks.lock().unwrap().push(lease);
        if std::mem::take(&mut *self.fail_rollback_spawn.lock().unwrap()) {
            Err(TestError("rollback spawn failed".to_string()))
        } else {
            Ok(())
        }
    }

    async fn prepare_close(
        &self,
        request: CloseLifecycleRequest,
    ) -> std::result::Result<Self::CloseLease, Self::Error> {
        if std::mem::take(&mut *self.fail_prepare_close.lock().unwrap()) {
            Err(TestError("prepare close failed".to_string()))
        } else {
            Ok(request.agent.identity.id)
        }
    }

    async fn commit_close(&self, lease: &Self::CloseLease) -> std::result::Result<(), Self::Error> {
        self.close_order.lock().unwrap().push(lease.clone());
        if std::mem::take(&mut *self.fail_commit_close.lock().unwrap()) {
            Err(TestError("commit close failed".to_string()))
        } else {
            Ok(())
        }
    }

    async fn rollback_close(
        &self,
        lease: Self::CloseLease,
    ) -> std::result::Result<(), Self::Error> {
        self.close_rollbacks.lock().unwrap().push(lease);
        if std::mem::take(&mut *self.fail_rollback_close.lock().unwrap()) {
            Err(TestError("rollback close failed".to_string()))
        } else {
            Ok(())
        }
    }
}

#[derive(Clone, Default)]
struct TestEvents {
    runtime: Arc<Mutex<Vec<AgentRuntimeEvent>>>,
    traces: Arc<Mutex<Vec<pl_trace::TraceEvent>>>,
}

impl TestEvents {
    fn runtime_len(&self) -> usize {
        self.runtime.lock().unwrap().len()
    }
}

impl AgentCommitObserver for TestEvents {
    async fn publish(&self, committed: AgentCommittedEvent) {
        self.runtime
            .lock()
            .unwrap()
            .extend(committed.runtime_events);
        self.traces.lock().unwrap().extend(committed.trace_events);
    }
}

#[derive(Clone)]
struct TestHost {
    repository: TestRepository,
    turn_factory: TestTurnFactory,
    lifecycle: TestLifecycle,
    events: TestEvents,
}

impl TestHost {
    fn new(repository: TestRepository, mode: FactoryMode) -> Self {
        Self {
            repository,
            turn_factory: TestTurnFactory::new(mode),
            lifecycle: TestLifecycle::default(),
            events: TestEvents::default(),
        }
    }
}

impl AgentRuntimeHost for TestHost {
    type Error = TestError;
    type Repository = TestRepository;
    type TurnFactory = TestTurnFactory;
    type Lifecycle = TestLifecycle;
    type Observer = TestEvents;

    fn repository(&self) -> &Self::Repository {
        &self.repository
    }

    fn turn_factory(&self) -> &Self::TurnFactory {
        &self.turn_factory
    }

    fn lifecycle(&self) -> &Self::Lifecycle {
        &self.lifecycle
    }

    fn observer(&self) -> &Self::Observer {
        &self.events
    }
}

fn identity(id: &str) -> AgentIdentity {
    AgentIdentity {
        id: AgentId::new(id).unwrap(),
        parent_id: None,
        role: crate::AgentRoleId::new("executor").unwrap(),
        depth: 0,
    }
}

fn registration(id: &str, _former_session: &str) -> AgentRegistration {
    AgentRegistration::new(identity(id))
}

fn pending_user_interaction(
    interaction_id: &str,
    thread_id: &ThreadId,
    turn_id: &str,
) -> InteractionRequest {
    InteractionRequest {
        interaction_id: interaction_id.to_string(),
        kind: InteractionKind::UserInput,
        status: InteractionStatus::Pending,
        scope: InteractionScope {
            thread_id: thread_id.to_string(),
            turn_id: turn_id.to_string(),
            item_id: Some(format!("{interaction_id}:item")),
            tool_id: Some(format!("{interaction_id}:tool")),
            agent_path: None,
        },
        payload: InteractionPayload::UserInput {
            questions: Vec::new(),
        },
        created_at: 1,
        updated_at: 1,
        resolved_at: None,
        resolution: None,
    }
}

fn interaction_continuation(pending: &InteractionRequest) -> AgentInteractionContinuationRequest {
    let mut resolved = pending.clone();
    resolved.status = InteractionStatus::Resolved;
    resolved.updated_at = 2;
    resolved.resolved_at = Some(2);
    resolved.resolution = Some(InteractionResolution::UserInput {
        answers: Default::default(),
    });
    let mail_id = AgentInteractionContinuationRequest::stable_mail_id(&pending.interaction_id);
    AgentInteractionContinuationRequest::new(
        resolved,
        AgentCurrentSessionSubmitRequest::start(format!(
            "interaction resolution for {}",
            pending.interaction_id
        ))
        .with_presentation(MailboxPresentation::Hidden)
        .with_mail_id(mail_id),
    )
}

async fn record_pending_interaction(
    handle: &AgentRuntimeHandle,
    agent_id: AgentId,
    thread_id: ThreadId,
    interaction: InteractionRequest,
) {
    handle
        .record_thread_facts(
            agent_id,
            thread_id,
            vec![crate::ThreadNotificationFact::durable(
                interaction.updated_at,
                ThreadNotification::InteractionChanged {
                    interaction: Box::new(interaction),
                },
            )],
        )
        .await
        .unwrap();
}

fn test_options() -> AgentRuntimeOptions {
    AgentRuntimeOptions {
        command_capacity: 32,
        cancel_grace: Duration::from_millis(10),
        restored_inputs: RestoredInputPolicy::Start,
        thread_events: crate::ThreadEventOptions::default(),
    }
}

fn child_spawn_request(parent_id: AgentId) -> AgentSpawnRequest {
    AgentSpawnRequest {
        thread_id: ThreadId::new("child-chat").unwrap(),
        parent_id,
        role: crate::AgentRoleId::new("worker").unwrap(),
        session: ThreadContextState::empty(),
        initial_turn_id: None,
        initial_message: None,
        metadata: serde_json::Value::Null,
    }
}

async fn wait_for_prepared_messages(factory: &TestTurnFactory, expected: usize) {
    tokio::time::timeout(Duration::from_secs(1), async {
        loop {
            if factory.prepared_messages.lock().unwrap().len() >= expected {
                break;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("turn factory should receive the expected inputs");
}

async fn wait_for_idle(handle: &AgentRuntimeHandle, agent_id: AgentId) -> AgentWaitResult {
    tokio::time::timeout(Duration::from_secs(1), handle.wait_until_idle(agent_id))
        .await
        .expect("agent should become idle")
        .unwrap()
}

#[tokio::test]
async fn failed_turn_returns_agent_to_active_idle_and_commits_snapshot() {
    let repository = TestRepository::empty();
    let host = TestHost::new(repository.clone(), FactoryMode::Fail);
    let runtime = AgentRuntime::start(host.clone(), test_options())
        .await
        .unwrap();
    let handle = runtime.handle();
    let agent_id = AgentId::new("root").unwrap();

    handle.register(registration("root", "chat")).await.unwrap();
    handle
        .submit(
            agent_id.clone(),
            AgentSubmitRequest::start(ThreadId::new("root").unwrap(), "hello"),
        )
        .await
        .unwrap();
    let waited = handle.wait_until_idle(agent_id.clone()).await.unwrap();

    assert_eq!(waited.snapshot.lifecycle, AgentLifecycleState::Active);
    assert_eq!(waited.snapshot.activity, AgentActivityState::Idle);
    assert_eq!(waited.last_turn.unwrap().kind, TurnOutcomeKind::Failed);
    assert_eq!(repository.state(&agent_id).snapshot, waited.snapshot);
    assert_eq!(host.events.runtime_len(), 4);
    runtime.shutdown().await.unwrap();
}

#[tokio::test]
async fn report_progress_appends_durable_submission_with_detail() {
    let repository = TestRepository::empty();
    let host = TestHost::new(repository.clone(), FactoryMode::Fail);
    let runtime = AgentRuntime::start(host.clone(), test_options())
        .await
        .unwrap();
    let handle = runtime.handle();
    let agent_id = AgentId::new("root").unwrap();
    handle.register(registration("root", "chat")).await.unwrap();

    // 携带 detail 的提交始终追加到 durable 日志。
    handle
        .report_progress(
            agent_id.clone(),
            AgentProgressStage::Implementing,
            "wiring submissions".to_string(),
            "verify pagination".to_string(),
            Some("replaced ephemeral progress with thread_submissions append".to_string()),
        )
        .await
        .unwrap();
    // 仅短字段、与上次相同 → 去重，不追加。
    handle
        .report_progress(
            agent_id.clone(),
            AgentProgressStage::Implementing,
            "wiring submissions".to_string(),
            "verify pagination".to_string(),
            None,
        )
        .await
        .unwrap();
    // 新 detail → 再追加一条。
    handle
        .report_progress(
            agent_id.clone(),
            AgentProgressStage::Verifying,
            "running tests".to_string(),
            "ship it".to_string(),
            Some("added integration test for read_agent_submissions".to_string()),
        )
        .await
        .unwrap();

    let page = handle
        .read_submissions(agent_id.clone(), 0, 10)
        .await
        .unwrap();
    assert_eq!(page.total, 2);
    assert_eq!(page.items.len(), 2);
    assert_eq!(page.items[0].report.stage, AgentProgressStage::Implementing);
    assert_eq!(
        page.items[0].detail.as_deref(),
        Some("replaced ephemeral progress with thread_submissions append")
    );
    assert_eq!(page.items[1].report.stage, AgentProgressStage::Verifying);

    // 分页：第二页为空，has_more 反映总数。
    let next = handle
        .read_submissions(agent_id.clone(), 2, 10)
        .await
        .unwrap();
    assert!(next.items.is_empty());
    assert!(!next.has_more);

    runtime.shutdown().await.unwrap();
}

#[tokio::test]
async fn conversation_recovery_excludes_rolled_back_provider_context_and_preserves_audit_usage() {
    let repository = TestRepository::empty();
    let host = TestHost::new(repository.clone(), FactoryMode::Fail);
    let runtime = AgentRuntime::start(host.clone(), test_options())
        .await
        .unwrap();
    let handle = runtime.handle();
    let agent_id = AgentId::new("root").unwrap();
    let retained_messages = vec![
        crate::user_text_message("retained request"),
        crate::assistant_text_message("retained answer"),
    ];
    let mut registration = registration("root", "chat");
    registration.session.session = AgentSession::from_messages(vec![
        retained_messages[0].clone(),
        retained_messages[1].clone(),
        crate::user_text_message("broken tail"),
        crate::assistant_text_message("failed answer"),
    ]);
    let usage = pl_model::TokenUsage {
        prompt_tokens: 120,
        completion_tokens: 30,
        total_tokens: 150,
        cached_prompt_tokens: 20,
        cache_write_tokens: 10,
        reasoning_tokens: 5,
    };
    registration.session.usage = usage.clone();
    let billing = TurnBillingRecord::new();
    registration
        .session
        .billing_by_turn
        .insert("turn-broken".to_string(), billing.clone());
    handle.register(registration).await.unwrap();

    let preview = handle
        .preview_conversation_recovery(
            agent_id.clone(),
            ConversationRecoveryTarget {
                mode: ConversationRecoveryMode::RewindTail,
                turn_ids: vec!["turn-broken".to_string()],
                input_hashes: vec![crate::canonical_content_hash(b"broken tail")],
            },
        )
        .await
        .unwrap();
    assert_eq!(preview.retained_item_count, 2);
    let request = ConversationRecoveryRequest {
        recovery_id: "recovery-usage".to_string(),
        preview,
    };
    let recovered = handle
        .recover_conversation(agent_id.clone(), request.clone())
        .await
        .unwrap();
    assert_eq!(
        handle
            .recover_conversation(agent_id.clone(), request)
            .await
            .unwrap(),
        recovered
    );

    let durable = repository.state(&agent_id);
    assert_eq!(durable.session.session.messages(), retained_messages);
    assert_eq!(durable.session.usage, usage);
    assert_eq!(
        durable.session.billing_by_turn.get("turn-broken"),
        Some(&billing)
    );
    assert!(matches!(
        repository.last_context(),
        Some(ThreadContextMutation::Replace { items }) if items.len() == 2
    ));

    handle
        .submit(
            agent_id.clone(),
            AgentSubmitRequest::start(ThreadId::new("root").unwrap(), "continue"),
        )
        .await
        .unwrap();
    wait_for_prepared_messages(&host.turn_factory, 1).await;
    assert_eq!(
        host.turn_factory.prepared_sessions.lock().unwrap()[0],
        retained_messages
    );
    assert!(
        host.turn_factory.prepared_sessions.lock().unwrap()[0]
            .iter()
            .all(|message| crate::message_content_text(&message.content) != "broken tail")
    );
    wait_for_idle(&handle, agent_id).await;
    runtime.shutdown().await.unwrap();
}

#[tokio::test]
async fn rebuild_thread_recovers_when_compaction_has_no_rewindable_prefix() {
    let repository = TestRepository::empty();
    let host = TestHost::new(repository.clone(), FactoryMode::Fail);
    let runtime = AgentRuntime::start(host.clone(), test_options())
        .await
        .unwrap();
    let handle = runtime.handle();
    let agent_id = AgentId::new("root").unwrap();
    let mut registration = registration("root", "chat");
    registration.session.session =
        AgentSession::from_messages(vec![crate::assistant_text_message(
            "compacted summary without original mailbox inputs",
        )]);
    handle.register(registration).await.unwrap();

    let preview = handle
        .preview_conversation_recovery(
            agent_id.clone(),
            ConversationRecoveryTarget {
                mode: ConversationRecoveryMode::RebuildThread,
                turn_ids: vec!["turn-compacted".to_string()],
                input_hashes: Vec::new(),
            },
        )
        .await
        .unwrap();
    assert_eq!(preview.retained_item_count, 0);
    assert_eq!(preview.removed_item_count, 1);
    handle
        .recover_conversation(
            agent_id.clone(),
            ConversationRecoveryRequest {
                recovery_id: "recovery-rebuild".to_string(),
                preview,
            },
        )
        .await
        .unwrap();

    handle
        .submit(
            agent_id.clone(),
            AgentSubmitRequest::start(ThreadId::new("root").unwrap(), "continue from workspace"),
        )
        .await
        .unwrap();
    wait_for_prepared_messages(&host.turn_factory, 1).await;
    assert!(host.turn_factory.prepared_sessions.lock().unwrap()[0].is_empty());
    wait_for_idle(&handle, agent_id).await;
    runtime.shutdown().await.unwrap();
}

#[tokio::test]
async fn idle_agent_role_reconfiguration_is_durable_and_updates_directory() {
    let repository = TestRepository::empty();
    let host = TestHost::new(repository.clone(), FactoryMode::Fail);
    let runtime = AgentRuntime::start(host, test_options()).await.unwrap();
    let handle = runtime.handle();
    let agent_id = AgentId::new("root").unwrap();
    let planner = crate::AgentRoleId::new("planner").unwrap();

    handle.register(registration("root", "chat")).await.unwrap();
    let changed = handle
        .reconfigure_idle_role(agent_id.clone(), planner.clone())
        .await
        .unwrap();

    assert_eq!(changed.identity.role, planner);
    assert_eq!(repository.state(&agent_id).snapshot, changed);
    assert_eq!(
        handle.directory_snapshot().agents[0].identity.role,
        crate::AgentRoleId::new("planner").unwrap()
    );
    runtime.shutdown().await.unwrap();
}

#[tokio::test]
async fn running_agent_rejects_role_reconfiguration() {
    let repository = TestRepository::empty();
    let host = TestHost::new(repository.clone(), FactoryMode::Block);
    let runtime = AgentRuntime::start(host, test_options()).await.unwrap();
    let handle = runtime.handle();
    let agent_id = AgentId::new("root").unwrap();

    handle.register(registration("root", "chat")).await.unwrap();
    handle
        .submit(
            agent_id.clone(),
            AgentSubmitRequest::start(ThreadId::new("root").unwrap(), "block"),
        )
        .await
        .unwrap();
    let error = handle
        .reconfigure_idle_role(
            agent_id.clone(),
            crate::AgentRoleId::new("planner").unwrap(),
        )
        .await
        .unwrap_err();

    assert!(
        error
            .to_string()
            .contains("only change while the Thread is idle")
    );
    assert_eq!(
        repository.state(&agent_id).snapshot.identity.role,
        crate::AgentRoleId::new("executor").unwrap()
    );
    handle.close(agent_id).await.unwrap();
    runtime.shutdown().await.unwrap();
}

#[tokio::test]
async fn wait_agents_observes_turn_that_finished_before_subscription() {
    let repository = TestRepository::empty();
    let host = TestHost::new(repository, FactoryMode::Fail);
    let runtime = AgentRuntime::start(host, test_options()).await.unwrap();
    let handle = runtime.handle();
    let agent_id = AgentId::new("root").unwrap();

    handle.register(registration("root", "chat")).await.unwrap();
    handle
        .submit(
            agent_id.clone(),
            AgentSubmitRequest::start(ThreadId::new("root").unwrap(), "hello"),
        )
        .await
        .unwrap();
    handle.wait_until_idle(agent_id.clone()).await.unwrap();

    let result = tokio::time::timeout(
        Duration::from_secs(1),
        handle.wait_agents(vec![agent_id.clone()]),
    )
    .await
    .expect("settled turn must be visible without a later directory revision")
    .unwrap();

    assert_eq!(result.reason, AgentDirectoryWaitReason::Terminal);
    assert_eq!(result.messages.len(), 1);
    assert_eq!(result.messages[0].identity.id, agent_id);
    assert_eq!(result.messages[0].lifecycle, AgentLifecycleState::Active);
    assert_eq!(
        result.messages[0].turn_outcome,
        Some(TurnOutcomeKind::Failed)
    );
    runtime.shutdown().await.unwrap();
}

#[tokio::test]
async fn submit_rejects_session_not_owned_by_target_agent() {
    let repository = TestRepository::empty();
    let host = TestHost::new(repository.clone(), FactoryMode::Fail);
    let runtime = AgentRuntime::start(host, test_options()).await.unwrap();
    let handle = runtime.handle();
    let agent_id = AgentId::new("root").unwrap();
    let unknown_session = ThreadId::new("child-session").unwrap();

    handle.register(registration("root", "chat")).await.unwrap();
    let error = handle
        .submit(
            agent_id.clone(),
            AgentSubmitRequest::start(unknown_session.clone(), "must fail"),
        )
        .await
        .unwrap_err();

    assert_eq!(
        error,
        AgentRuntimeError::ThreadMismatch {
            agent_id: agent_id.clone(),
            expected: ThreadId::new("root").unwrap(),
            actual: unknown_session,
        }
    );
    assert_eq!(
        repository.state(&agent_id).snapshot.identity.id,
        ThreadId::new("root").unwrap()
    );
    runtime.shutdown().await.unwrap();
}

#[tokio::test]
async fn current_session_submit_resolves_the_single_owned_session() {
    let repository = TestRepository::empty();
    let host = TestHost::new(repository.clone(), FactoryMode::Fail);
    let runtime = AgentRuntime::start(host, test_options()).await.unwrap();
    let handle = runtime.handle();
    let agent_id = AgentId::new("root").unwrap();
    let session_id = ThreadId::new("root").unwrap();

    handle.register(registration("root", "chat")).await.unwrap();
    handle
        .submit_current_session(
            agent_id.clone(),
            AgentCurrentSessionSubmitRequest::start("resolved input"),
        )
        .await
        .unwrap();
    handle.wait_until_idle(agent_id.clone()).await.unwrap();

    assert_eq!(
        repository
            .state(&agent_id)
            .snapshot
            .last_turn
            .as_ref()
            .map(|turn| &turn.thread_id),
        Some(&session_id)
    );
    runtime.shutdown().await.unwrap();
}

#[tokio::test]
async fn current_session_submit_rejects_a_missing_parent_graph() {
    let host = TestHost::new(TestRepository::empty(), FactoryMode::Fail);
    let runtime = AgentRuntime::start(host, test_options()).await.unwrap();
    let handle = runtime.handle();
    let agent_id = AgentId::new("child").unwrap();
    let missing_parent = AgentId::new("missing-parent").unwrap();
    let mut child_identity = identity("child");
    child_identity.parent_id = Some(missing_parent.clone());
    child_identity.depth = 1;

    handle
        .register(AgentRegistration::new(child_identity))
        .await
        .unwrap();
    let error = handle
        .submit_current_session(
            agent_id,
            AgentCurrentSessionSubmitRequest::start("must reject invalid graph"),
        )
        .await
        .unwrap_err();

    assert_eq!(
        error,
        AgentRuntimeError::Lifecycle(format!(
            "agent parent {} is missing while resolving root for child",
            missing_parent.as_str()
        ))
    );
    runtime.shutdown().await.unwrap();
}

#[tokio::test]
async fn concurrent_submits_are_serialized_without_losing_inputs() {
    let host = TestHost::new(TestRepository::empty(), FactoryMode::Fail);
    let runtime = AgentRuntime::start(host.clone(), test_options())
        .await
        .unwrap();
    let handle = runtime.handle();
    let agent_id = AgentId::new("root").unwrap();
    let session_id = ThreadId::new("root").unwrap();
    handle.register(registration("root", "chat")).await.unwrap();

    let submits = (0..16).map(|index| {
        let handle = handle.clone();
        let agent_id = agent_id.clone();
        let session_id = session_id.clone();
        tokio::spawn(async move {
            handle
                .submit(
                    agent_id,
                    AgentSubmitRequest::start(session_id, format!("message-{index}")),
                )
                .await
                .unwrap()
        })
    });
    let mut turn_ids = BTreeSet::new();
    for submit in submits {
        turn_ids.insert(submit.await.unwrap().to_string());
    }
    handle.wait_until_idle(agent_id).await.unwrap();

    let prepared = host.turn_factory.prepared_messages.lock().unwrap().clone();
    assert_eq!(turn_ids.len(), 16);
    assert_eq!(prepared.len(), 16);
    assert_eq!(prepared.into_iter().collect::<BTreeSet<_>>().len(), 16);
    runtime.shutdown().await.unwrap();
}

#[tokio::test]
async fn concurrent_start_only_submits_allow_one_turn_and_steer_is_atomic() {
    let host = TestHost::new(TestRepository::empty(), FactoryMode::Block);
    let runtime = AgentRuntime::start(host.clone(), test_options())
        .await
        .unwrap();
    let handle = runtime.handle();
    let agent_id = AgentId::new("root").unwrap();
    let thread_id = ThreadId::new("root").unwrap();
    handle.register(registration("root", "chat")).await.unwrap();

    let submits = ["first", "second"].map(|message| {
        let handle = handle.clone();
        let agent_id = agent_id.clone();
        let thread_id = thread_id.clone();
        tokio::spawn(async move {
            handle
                .submit(
                    agent_id,
                    AgentSubmitRequest::start(thread_id, message)
                        .with_turn_policy(AgentTurnSubmitPolicy::StartOnly),
                )
                .await
        })
    });
    let mut started_turn = None;
    let mut rejected = 0;
    for submit in submits {
        match submit.await.unwrap() {
            Ok(turn_id) => started_turn = Some(turn_id),
            Err(AgentRuntimeError::InvalidInput(reason)) => {
                assert_eq!(reason, "startTurn requires an idle Thread");
                rejected += 1;
            }
            Err(error) => panic!("unexpected concurrent start result: {error}"),
        }
    }
    let started_turn = started_turn.expect("one concurrent start must win");
    assert_eq!(rejected, 1);
    wait_for_prepared_messages(&host.turn_factory, 1).await;

    let steered_turn = handle
        .submit(
            agent_id.clone(),
            AgentSubmitRequest::start(thread_id, "steer")
                .with_turn_policy(AgentTurnSubmitPolicy::SteerOnly),
        )
        .await
        .unwrap();
    assert_eq!(steered_turn, started_turn);

    runtime.shutdown().await.unwrap();
}

#[tokio::test]
async fn start_or_queue_never_steers_an_active_turn() {
    let repository = TestRepository::empty();
    let host = TestHost::new(repository.clone(), FactoryMode::Block);
    let runtime = AgentRuntime::start(host.clone(), test_options())
        .await
        .unwrap();
    let handle = runtime.handle();
    let agent_id = AgentId::new("root").unwrap();
    let thread_id = ThreadId::new("root").unwrap();
    handle.register(registration("root", "chat")).await.unwrap();

    let active_turn = handle
        .submit(
            agent_id.clone(),
            AgentSubmitRequest::start(thread_id.clone(), "active"),
        )
        .await
        .unwrap();
    wait_for_prepared_messages(&host.turn_factory, 1).await;

    let queued_turn = handle
        .submit(
            agent_id.clone(),
            AgentSubmitRequest::start(thread_id, "next")
                .with_mail_id("mail:next")
                .with_turn_policy(AgentTurnSubmitPolicy::StartOrQueue),
        )
        .await
        .unwrap();

    assert_ne!(queued_turn, active_turn);
    assert_eq!(
        handle
            .snapshot(agent_id.clone())
            .await
            .unwrap()
            .pending_inputs,
        1
    );
    assert_eq!(host.turn_factory.prepared_messages.lock().unwrap().len(), 1);
    let durable = repository.state(&agent_id);
    assert_eq!(durable.pending_inputs.len(), 1);
    assert!(durable.active_input.is_some());
    assert!(durable.pending_inputs.iter().any(|input| {
        input.mail_id == "mail:next"
            && matches!(input.delivery_state, MailboxDeliveryState::Pending)
    }));

    host.turn_factory.blocker.notify_one();
    wait_for_prepared_messages(&host.turn_factory, 2).await;
    runtime.shutdown().await.unwrap();
}

#[tokio::test]
async fn interaction_continuation_starts_idle_fresh_turn_and_deduplicates_stable_mail() {
    let repository = TestRepository::empty();
    let host = TestHost::new(repository.clone(), FactoryMode::Block);
    let runtime = AgentRuntime::start(host.clone(), test_options())
        .await
        .unwrap();
    let handle = runtime.handle();
    let agent_id = AgentId::new("root").unwrap();
    let thread_id = ThreadId::new("root").unwrap();
    handle.register(registration("root", "chat")).await.unwrap();
    let pending = pending_user_interaction("ask-idle", &thread_id, "turn-origin");
    record_pending_interaction(
        &handle,
        agent_id.clone(),
        thread_id.clone(),
        pending.clone(),
    )
    .await;
    let continuation = interaction_continuation(&pending);

    handle
        .submit_interaction_continuation(agent_id.clone(), continuation.clone())
        .await
        .unwrap();
    wait_for_prepared_messages(&host.turn_factory, 1).await;
    let active = repository.state(&agent_id);
    assert!(active.active_input.as_ref().is_some_and(|input| {
        input.mail_id == AgentInteractionContinuationRequest::stable_mail_id("ask-idle")
            && input.payload.presentation == MailboxPresentation::Hidden
    }));

    handle
        .submit_interaction_continuation(agent_id.clone(), continuation)
        .await
        .unwrap();
    assert_eq!(host.turn_factory.prepared_messages.lock().unwrap().len(), 1);
    assert!(repository.state(&agent_id).pending_inputs.is_empty());

    host.turn_factory.blocker.notify_one();
    handle.wait_until_idle(agent_id.clone()).await.unwrap();
    assert_eq!(host.turn_factory.prepared_messages.lock().unwrap().len(), 1);
    let consumed = repository.state(&agent_id);
    assert!(consumed.active_input.is_none());
    assert!(consumed.pending_inputs.is_empty());

    assert!(
        handle
            .thread_snapshot(&thread_id)
            .unwrap()
            .interactions
            .iter()
            .all(|interaction| interaction.interaction_id != "ask-idle")
    );
    runtime.shutdown().await.unwrap();
}

#[tokio::test]
async fn interaction_continuations_queue_without_steering_origin_or_unrelated_active_turn() {
    let repository = TestRepository::empty();
    let host = TestHost::new(repository.clone(), FactoryMode::Block);
    let runtime = AgentRuntime::start(host.clone(), test_options())
        .await
        .unwrap();
    let handle = runtime.handle();
    let agent_id = AgentId::new("root").unwrap();
    let thread_id = ThreadId::new("root").unwrap();
    handle.register(registration("root", "chat")).await.unwrap();
    let active_turn = handle
        .submit(
            agent_id.clone(),
            AgentSubmitRequest::start(thread_id.clone(), "active"),
        )
        .await
        .unwrap();
    wait_for_prepared_messages(&host.turn_factory, 1).await;

    let origin = pending_user_interaction("ask-origin", &thread_id, active_turn.as_str());
    let unrelated = pending_user_interaction("ask-unrelated", &thread_id, "turn-completed");
    for pending in [&origin, &unrelated] {
        record_pending_interaction(
            &handle,
            agent_id.clone(),
            thread_id.clone(),
            pending.clone(),
        )
        .await;
        handle
            .submit_interaction_continuation(agent_id.clone(), interaction_continuation(pending))
            .await
            .unwrap();
    }

    assert_eq!(host.turn_factory.prepared_messages.lock().unwrap().len(), 1);
    let durable = repository.state(&agent_id);
    assert_eq!(durable.snapshot.active_turn_id.as_ref(), Some(&active_turn));
    assert_eq!(durable.pending_inputs.len(), 2);
    assert!(durable.pending_inputs.iter().all(|input| {
        matches!(input.delivery_state, MailboxDeliveryState::Pending)
            && input.queue_coalescing_key.is_none()
    }));
    runtime.shutdown().await.unwrap();
}

#[tokio::test]
async fn interaction_continuation_repository_failure_rolls_back_resolution_and_input() {
    let repository = TestRepository::empty();
    let host = TestHost::new(repository.clone(), FactoryMode::Block);
    let runtime = AgentRuntime::start(host, test_options()).await.unwrap();
    let handle = runtime.handle();
    let agent_id = AgentId::new("root").unwrap();
    let thread_id = ThreadId::new("root").unwrap();
    handle.register(registration("root", "chat")).await.unwrap();
    let pending = pending_user_interaction("ask-rollback", &thread_id, "turn-origin");
    record_pending_interaction(
        &handle,
        agent_id.clone(),
        thread_id.clone(),
        pending.clone(),
    )
    .await;
    repository.fail_next_turn_queue_commit();

    let error = handle
        .submit_interaction_continuation(agent_id.clone(), interaction_continuation(&pending))
        .await
        .unwrap_err();
    assert!(error.to_string().contains("turn queue commit failed"));
    let durable = repository.state(&agent_id);
    assert!(durable.pending_inputs.is_empty());
    assert!(durable.active_input.is_none());
    let canonical = handle.thread_snapshot(&thread_id).unwrap();
    assert_eq!(
        canonical
            .interactions
            .iter()
            .find(|interaction| interaction.interaction_id == pending.interaction_id)
            .map(|interaction| interaction.status.clone()),
        Some(InteractionStatus::Pending)
    );
    runtime.shutdown().await.unwrap();
}

#[tokio::test]
async fn queued_inputs_with_the_same_key_share_the_latest_turn() {
    let repository = TestRepository::empty();
    let host = TestHost::new(repository.clone(), FactoryMode::Block);
    let runtime = AgentRuntime::start(host.clone(), test_options())
        .await
        .unwrap();
    let handle = runtime.handle();
    let agent_id = AgentId::new("root").unwrap();
    let thread_id = ThreadId::new("root").unwrap();
    handle.register(registration("root", "chat")).await.unwrap();

    handle
        .submit(
            agent_id.clone(),
            AgentSubmitRequest::start(thread_id.clone(), "active"),
        )
        .await
        .unwrap();
    wait_for_prepared_messages(&host.turn_factory, 1).await;

    let mut queued_turns = Vec::new();
    for message in ["first wake", "second wake", "latest wake"] {
        queued_turns.push(
            handle
                .submit(
                    agent_id.clone(),
                    AgentSubmitRequest::start(thread_id.clone(), message)
                        .with_mail_id(format!("mail:{message}"))
                        .with_queue_coalescing_key("task-planner-wake")
                        .with_turn_policy(AgentTurnSubmitPolicy::StartOrQueue),
                )
                .await
                .unwrap(),
        );
    }

    host.turn_factory.blocker.notify_one();
    wait_for_prepared_messages(&host.turn_factory, 2).await;

    let durable = repository.state(&agent_id);
    assert_eq!(
        durable.snapshot.active_turn_id,
        Some(queued_turns[2].clone())
    );
    assert_eq!(
        durable
            .active_input
            .as_ref()
            .map(|input| input.payload.message.as_str()),
        Some("latest wake")
    );
    assert_eq!(durable.pending_inputs.len(), 2);
    assert!(durable.pending_inputs.iter().all(|input| matches!(
        &input.delivery_state,
        MailboxDeliveryState::Claimed { turn_id, .. } if turn_id == &queued_turns[2]
    )));
    assert_eq!(
        host.turn_factory.prepared_batches.lock().unwrap()[1],
        vec![
            "first wake".to_string(),
            "second wake".to_string(),
            "latest wake".to_string(),
        ]
    );

    host.turn_factory.blocker.notify_one();
    runtime.shutdown().await.unwrap();
}

#[tokio::test]
async fn cancellation_aborts_blocked_turn_after_grace_and_records_cancelled_outcome() {
    let repository = TestRepository::empty();
    let host = TestHost::new(repository.clone(), FactoryMode::Block);
    let runtime = AgentRuntime::start(host, test_options()).await.unwrap();
    let handle = runtime.handle();
    let agent_id = AgentId::new("root").unwrap();
    let mut registration = registration("root", "chat");
    registration
        .session
        .session
        .push_user_prompt("已有上下文".to_string());
    handle.register(registration).await.unwrap();
    let turn_id = handle
        .submit(
            agent_id.clone(),
            AgentSubmitRequest::start(ThreadId::new("root").unwrap(), "block"),
        )
        .await
        .unwrap();

    handle.cancel_turn(agent_id.clone(), turn_id).await.unwrap();
    let waited = wait_for_idle(&handle, agent_id).await;

    assert_eq!(waited.last_turn.unwrap().kind, TurnOutcomeKind::Cancelled);
    assert_eq!(
        repository
            .state(&AgentId::new("root").unwrap())
            .session
            .session
            .messages()
            .len(),
        1
    );
    runtime.shutdown().await.unwrap();
}

#[tokio::test]
async fn shutdown_durably_cancels_running_turn_before_actor_exit() {
    let repository = TestRepository::empty();
    let host = TestHost::new(repository.clone(), FactoryMode::Block);
    let runtime = AgentRuntime::start(host, test_options()).await.unwrap();
    let handle = runtime.handle();
    let agent_id = AgentId::new("root").unwrap();
    handle.register(registration("root", "chat")).await.unwrap();
    handle
        .submit(
            agent_id.clone(),
            AgentSubmitRequest::start(ThreadId::new("root").unwrap(), "block"),
        )
        .await
        .unwrap();

    runtime.shutdown().await.unwrap();

    let state = repository.state(&agent_id);
    assert_eq!(state.snapshot.activity, AgentActivityState::Idle);
    assert_eq!(state.snapshot.active_turn_id, None);
    let outcome = state.snapshot.last_turn.unwrap();
    assert_eq!(outcome.kind, TurnOutcomeKind::Cancelled);
    assert_eq!(outcome.reason.as_deref(), Some("runtime_shutdown"));
}

#[tokio::test]
async fn terminal_repository_failure_faults_actor_and_rejects_new_input() {
    let repository = TestRepository::empty();
    repository.fail_terminal_commits();
    let host = TestHost::new(repository, FactoryMode::Fail);
    let runtime = AgentRuntime::start(host, test_options()).await.unwrap();
    let handle = runtime.handle();
    let agent_id = AgentId::new("root").unwrap();
    handle.register(registration("root", "chat")).await.unwrap();
    handle
        .submit(
            agent_id.clone(),
            AgentSubmitRequest::start(ThreadId::new("root").unwrap(), "fail terminal"),
        )
        .await
        .unwrap();
    let waited = wait_for_idle(&handle, agent_id.clone()).await;
    let snapshot = waited.snapshot;

    assert_eq!(snapshot.lifecycle, AgentLifecycleState::Faulted);
    assert_eq!(
        waited.last_turn.as_ref().map(|outcome| outcome.kind),
        Some(TurnOutcomeKind::Failed)
    );
    assert!(
        waited
            .last_turn
            .as_ref()
            .and_then(|outcome| outcome.reason.as_deref())
            .is_some_and(|reason| reason.contains("terminal commit failed"))
    );
    let error = handle
        .submit(
            agent_id.clone(),
            AgentSubmitRequest::start(ThreadId::new("root").unwrap(), "again"),
        )
        .await
        .unwrap_err();
    assert_eq!(
        error,
        AgentRuntimeError::NotActive(agent_id, AgentLifecycleState::Faulted)
    );
    runtime.shutdown().await.unwrap();
}

#[tokio::test]
async fn fault_commit_failure_preserves_in_memory_turn_outcome() {
    let repository = TestRepository::empty();
    repository.fail_terminal_commits();
    repository.fail_next_fault_commit();
    let host = TestHost::new(repository, FactoryMode::Fail);
    let runtime = AgentRuntime::start(host, test_options()).await.unwrap();
    let handle = runtime.handle();
    let agent_id = AgentId::new("root").unwrap();
    handle.register(registration("root", "chat")).await.unwrap();
    handle
        .submit(
            agent_id.clone(),
            AgentSubmitRequest::start(ThreadId::new("root").unwrap(), "fail terminal and fault"),
        )
        .await
        .unwrap();

    let waited = wait_for_idle(&handle, agent_id).await;

    assert_eq!(waited.snapshot.lifecycle, AgentLifecycleState::Faulted);
    assert_eq!(waited.snapshot.activity, AgentActivityState::Idle);
    let outcome = waited
        .last_turn
        .expect("in-memory fallback must retain the failed turn outcome");
    assert_eq!(outcome.kind, TurnOutcomeKind::Failed);
    assert!(
        outcome
            .reason
            .as_deref()
            .is_some_and(|reason| reason.contains("terminal commit failed"))
    );
    runtime.shutdown().await.unwrap();
}

#[tokio::test]
async fn turn_started_repository_failure_commits_faulted_state() {
    let repository = TestRepository::empty();
    repository.fail_next_turn_started_commit();
    let host = TestHost::new(repository.clone(), FactoryMode::Fail);
    let runtime = AgentRuntime::start(host, test_options()).await.unwrap();
    let handle = runtime.handle();
    let agent_id = AgentId::new("root").unwrap();
    handle.register(registration("root", "chat")).await.unwrap();
    handle
        .submit(
            agent_id.clone(),
            AgentSubmitRequest::start(ThreadId::new("root").unwrap(), "fail turn start"),
        )
        .await
        .unwrap();

    let waited = wait_for_idle(&handle, agent_id.clone()).await;
    assert_eq!(waited.snapshot.lifecycle, AgentLifecycleState::Faulted);
    assert_eq!(waited.snapshot.activity, AgentActivityState::Idle);
    assert_eq!(waited.snapshot.active_turn_id, None);
    let failed_turn = waited.last_turn.expect("failed turn should be visible");
    assert_eq!(failed_turn.kind, TurnOutcomeKind::Failed);
    assert!(
        failed_turn
            .reason
            .as_deref()
            .is_some_and(|reason| reason.contains("turn started commit failed"))
    );

    let durable = repository.state(&agent_id).snapshot;
    assert_eq!(durable.lifecycle, AgentLifecycleState::Faulted);
    assert_eq!(durable.activity, AgentActivityState::Idle);
    assert_eq!(durable.active_turn_id, None);
    assert_eq!(durable.pending_inputs, 1);
    assert_eq!(durable.last_turn, waited.snapshot.last_turn);
    runtime.shutdown().await.unwrap();

    let restarted_host = TestHost::new(repository.clone(), FactoryMode::Fail);
    let restarted = AgentRuntime::start(restarted_host.clone(), test_options())
        .await
        .unwrap();
    let recovered = wait_for_idle(&restarted.handle(), agent_id.clone()).await;
    assert_eq!(recovered.snapshot.lifecycle, AgentLifecycleState::Faulted);
    assert_eq!(recovered.snapshot.activity, AgentActivityState::Idle);
    assert_eq!(recovered.snapshot.pending_inputs, 1);
    assert_eq!(
        restarted_host
            .turn_factory
            .prepared_messages
            .lock()
            .unwrap()
            .as_slice(),
        [] as [&str; 0]
    );
    assert_eq!(repository.state(&agent_id).snapshot, recovered.snapshot);
    restarted.shutdown().await.unwrap();
}

#[tokio::test]
async fn turn_activity_change_commits_event_and_snapshot_atomically() {
    let repository = TestRepository::empty();
    let host = TestHost::new(repository.clone(), FactoryMode::Block);
    let events = host.events.clone();
    let runtime = AgentRuntime::start(host, test_options()).await.unwrap();
    let handle = runtime.handle();
    let agent_id = AgentId::new("root").unwrap();
    handle.register(registration("root", "chat")).await.unwrap();
    let turn_id = handle
        .submit(
            agent_id.clone(),
            AgentSubmitRequest::start(ThreadId::new("root").unwrap(), "wait for tool"),
        )
        .await
        .unwrap();

    handle
        .set_activity(agent_id.clone(), turn_id.clone(), ActiveKind::WaitingTool)
        .await
        .unwrap();

    let snapshot = handle.snapshot(agent_id.clone()).await.unwrap();
    assert_eq!(
        snapshot.activity,
        AgentActivityState::Active(ActiveKind::WaitingTool)
    );
    let activity_event = {
        let committed = events.runtime.lock().unwrap();
        committed
            .iter()
            .find_map(|event| match &event.kind {
                AgentRuntimeEventKind::TurnActivityChanged {
                    turn_id: event_turn_id,
                    kind,
                    snapshot,
                    ..
                } if event_turn_id == &turn_id => Some((*kind, snapshot.as_ref().clone())),
                _ => None,
            })
            .expect("activity change must publish a committed runtime event")
    };
    assert_eq!(activity_event.0, ActiveKind::WaitingTool);
    assert_eq!(activity_event.1, snapshot);
    assert_eq!(repository.state(&agent_id).snapshot, snapshot);
    handle.cancel_turn(agent_id, turn_id).await.unwrap();
    runtime.shutdown().await.unwrap();
}

#[tokio::test]
async fn restart_recovery_cancels_running_turn_before_actor_registration() {
    let mut state = registration("root", "chat").into_durable_state();
    state.snapshot.revision = 7;
    state.snapshot.event_sequence = 11;
    state.snapshot.activity = AgentActivityState::Active(ActiveKind::Running);
    state.snapshot.active_turn_id = Some(TurnId::new("old-turn").unwrap());
    let repository = TestRepository::with_state(state);
    let host = TestHost::new(repository.clone(), FactoryMode::Fail);

    let runtime = AgentRuntime::start(host, test_options()).await.unwrap();
    let snapshot = runtime
        .handle()
        .snapshot(AgentId::new("root").unwrap())
        .await
        .unwrap();

    assert_eq!(snapshot.activity, AgentActivityState::Idle);
    assert_eq!(snapshot.revision, 8);
    assert_eq!(
        snapshot.last_turn.unwrap().reason,
        Some("runtime_restarted".to_string())
    );
    assert_eq!(
        repository
            .state(&AgentId::new("root").unwrap())
            .snapshot
            .revision,
        8
    );
    runtime.shutdown().await.unwrap();
}

#[tokio::test]
async fn restart_recovery_replays_pending_inputs_in_fifo_order() {
    let agent_id = AgentId::new("root").unwrap();
    let session_id = ThreadId::new("root").unwrap();
    let mut state = registration("root", "chat").into_durable_state();
    for (index, message) in ["first", "second"].into_iter().enumerate() {
        state.pending_inputs.push_back(DurableMailboxEnvelope {
            mail_id: format!("mail:turn-{index}"),
            turn_id: TurnId::new(format!("turn-{index}")).unwrap(),
            thread_id: session_id.clone(),
            payload: MailboxInputPayload::user(message),
            queue_coalescing_key: None,
            delivery_state: Default::default(),
            queued_at: index as i64,
        });
    }
    state.snapshot.activity = AgentActivityState::Queued;
    state.snapshot.pending_inputs = state.pending_inputs.len();
    let repository = TestRepository::with_state(state);
    let host = TestHost::new(repository.clone(), FactoryMode::Fail);

    let runtime = AgentRuntime::start(host.clone(), test_options())
        .await
        .unwrap();
    runtime
        .handle()
        .wait_until_idle(agent_id.clone())
        .await
        .unwrap();

    assert_eq!(
        host.turn_factory.prepared_messages.lock().unwrap().clone(),
        vec!["first".to_string(), "second".to_string()]
    );
    assert_eq!(repository.state(&agent_id).snapshot.pending_inputs, 0);
    runtime.shutdown().await.unwrap();
}

#[tokio::test]
async fn restored_inputs_wait_for_host_resource_activation() {
    let agent_id = AgentId::new("root").unwrap();
    let session_id = ThreadId::new("root").unwrap();
    let mut state = registration("root", "chat").into_durable_state();
    state.pending_inputs.push_back(DurableMailboxEnvelope {
        mail_id: "mail:restored-turn".to_string(),
        turn_id: TurnId::new("restored-turn").unwrap(),
        thread_id: session_id,
        payload: MailboxInputPayload::user("after-resources-ready"),
        queue_coalescing_key: None,
        delivery_state: Default::default(),
        queued_at: 1,
    });
    state.snapshot.activity = AgentActivityState::Queued;
    state.snapshot.pending_inputs = 1;
    let host = TestHost::new(TestRepository::with_state(state), FactoryMode::Fail);
    let mut options = test_options();
    options.restored_inputs = RestoredInputPolicy::Hold;

    let runtime = AgentRuntime::start(host.clone(), options).await.unwrap();
    tokio::time::sleep(Duration::from_millis(20)).await;
    assert!(
        host.turn_factory
            .prepared_messages
            .lock()
            .unwrap()
            .is_empty()
    );

    runtime.handle().start_restored_inputs().await.unwrap();
    runtime.handle().wait_until_idle(agent_id).await.unwrap();
    assert_eq!(
        host.turn_factory
            .prepared_messages
            .lock()
            .unwrap()
            .as_slice(),
        &["after-resources-ready".to_string()]
    );
    runtime.shutdown().await.unwrap();
}

#[tokio::test]
async fn close_closes_descendants_from_deepest_to_root() {
    let host = TestHost::new(TestRepository::empty(), FactoryMode::Fail);
    let runtime = AgentRuntime::start(host.clone(), test_options())
        .await
        .unwrap();
    let handle = runtime.handle();
    let root = AgentId::new("root").unwrap();
    handle
        .register(registration("root", "root-chat"))
        .await
        .unwrap();
    let child = handle
        .spawn(AgentSpawnRequest {
            thread_id: ThreadId::new("child-chat").unwrap(),
            parent_id: root.clone(),
            role: crate::AgentRoleId::new("worker").unwrap(),
            session: ThreadContextState::empty(),
            initial_turn_id: None,
            initial_message: None,
            metadata: serde_json::Value::Null,
        })
        .await
        .unwrap()
        .snapshot
        .identity
        .id;
    let grandchild = handle
        .spawn(AgentSpawnRequest {
            thread_id: ThreadId::new("grandchild-chat").unwrap(),
            parent_id: child.clone(),
            role: crate::AgentRoleId::new("worker").unwrap(),
            session: ThreadContextState::empty(),
            initial_turn_id: None,
            initial_message: None,
            metadata: serde_json::Value::Null,
        })
        .await
        .unwrap()
        .snapshot
        .identity
        .id;

    let snapshot = handle.close(root.clone()).await.unwrap();

    assert_eq!(snapshot.lifecycle, AgentLifecycleState::Closed);
    assert_eq!(
        host.lifecycle.close_order.lock().unwrap().clone(),
        vec![grandchild.clone(), child.clone(), root]
    );
    assert_eq!(
        handle.snapshot(child).await.unwrap().lifecycle,
        AgentLifecycleState::Closed
    );
    assert_eq!(
        handle.snapshot(grandchild).await.unwrap().lifecycle,
        AgentLifecycleState::Closed
    );
    runtime.shutdown().await.unwrap();
}

#[tokio::test]
async fn spawn_prepare_failure_has_no_framework_side_effects() {
    let repository = TestRepository::empty();
    let host = TestHost::new(repository.clone(), FactoryMode::Fail);
    let runtime = AgentRuntime::start(host.clone(), test_options())
        .await
        .unwrap();
    let handle = runtime.handle();
    let root = AgentId::new("root").unwrap();
    handle
        .register(registration("root", "root-chat"))
        .await
        .unwrap();
    host.lifecycle.fail_next_prepare_spawn();

    let error = handle.spawn(child_spawn_request(root)).await.unwrap_err();

    assert!(error.to_string().contains("prepare spawn failed"));
    assert_eq!(repository.states.lock().unwrap().len(), 1);
    assert_eq!(host.lifecycle.spawn_rollbacks.lock().unwrap().len(), 0);
    runtime.shutdown().await.unwrap();
}

#[tokio::test]
async fn spawn_registration_failure_rolls_back_prepared_resources() {
    let repository = TestRepository::empty();
    let host = TestHost::new(repository.clone(), FactoryMode::Fail);
    let runtime = AgentRuntime::start(host.clone(), test_options())
        .await
        .unwrap();
    let handle = runtime.handle();
    let root = AgentId::new("root").unwrap();
    handle
        .register(registration("root", "root-chat"))
        .await
        .unwrap();
    repository.fail_next_registration();

    let error = handle.spawn(child_spawn_request(root)).await.unwrap_err();

    assert!(error.to_string().contains("registration commit failed"));
    assert_eq!(repository.states.lock().unwrap().len(), 1);
    assert_eq!(host.lifecycle.spawn_rollbacks.lock().unwrap().len(), 1);
    runtime.shutdown().await.unwrap();
}

#[tokio::test]
async fn spawn_activation_failure_is_durably_closed_after_successful_rollback() {
    let repository = TestRepository::empty();
    let host = TestHost::new(repository.clone(), FactoryMode::Fail);
    let runtime = AgentRuntime::start(host.clone(), test_options())
        .await
        .unwrap();
    let handle = runtime.handle();
    let root = AgentId::new("root").unwrap();
    handle
        .register(registration("root", "root-chat"))
        .await
        .unwrap();
    host.lifecycle.fail_next_activate_spawn();

    let error = handle
        .spawn(child_spawn_request(root.clone()))
        .await
        .unwrap_err();
    let children = handle
        .list()
        .await
        .unwrap()
        .into_iter()
        .filter(|snapshot| snapshot.identity.id != root)
        .collect::<Vec<_>>();

    assert!(error.to_string().contains("activate spawn failed"));
    assert_eq!(children.len(), 1);
    assert_eq!(children[0].lifecycle, AgentLifecycleState::Closed);
    assert_eq!(children[0].pending_inputs, 0);
    assert_eq!(
        repository.state(&children[0].identity.id).snapshot,
        children[0]
    );
    assert_eq!(host.lifecycle.spawn_rollbacks.lock().unwrap().len(), 1);
    runtime.shutdown().await.unwrap();
}

#[tokio::test]
async fn spawn_rollback_failure_is_retained_in_fault_diagnostic() {
    let repository = TestRepository::empty();
    let host = TestHost::new(repository, FactoryMode::Fail);
    let runtime = AgentRuntime::start(host.clone(), test_options())
        .await
        .unwrap();
    let handle = runtime.handle();
    let root = AgentId::new("root").unwrap();
    handle
        .register(registration("root", "root-chat"))
        .await
        .unwrap();
    host.lifecycle.fail_next_activate_spawn();
    host.lifecycle.fail_next_rollback_spawn();

    let error = handle.spawn(child_spawn_request(root)).await.unwrap_err();

    assert!(error.to_string().contains("activate spawn failed"));
    assert!(error.to_string().contains("rollback spawn failed"));
    let child = handle
        .list()
        .await
        .unwrap()
        .into_iter()
        .find(|snapshot| snapshot.identity.parent_id.is_some())
        .expect("faulted child");
    assert_eq!(child.lifecycle, AgentLifecycleState::Faulted);
    runtime.shutdown().await.unwrap();
}

#[tokio::test]
async fn close_prepare_failure_keeps_agent_active() {
    let repository = TestRepository::empty();
    let host = TestHost::new(repository.clone(), FactoryMode::Fail);
    let runtime = AgentRuntime::start(host.clone(), test_options())
        .await
        .unwrap();
    let handle = runtime.handle();
    let root = AgentId::new("root").unwrap();
    handle.register(registration("root", "chat")).await.unwrap();
    host.lifecycle.fail_next_prepare_close();

    let error = handle.close(root.clone()).await.unwrap_err();

    assert!(error.to_string().contains("prepare close failed"));
    assert_eq!(
        handle.snapshot(root.clone()).await.unwrap().lifecycle,
        AgentLifecycleState::Active
    );
    assert_eq!(
        repository.state(&root).snapshot.lifecycle,
        AgentLifecycleState::Active
    );
    runtime.shutdown().await.unwrap();
}

#[tokio::test]
async fn closing_commit_failure_rolls_back_prepared_resources() {
    let repository = TestRepository::empty();
    let host = TestHost::new(repository.clone(), FactoryMode::Fail);
    let runtime = AgentRuntime::start(host.clone(), test_options())
        .await
        .unwrap();
    let handle = runtime.handle();
    let root = AgentId::new("root").unwrap();
    handle.register(registration("root", "chat")).await.unwrap();
    repository.fail_next_lifecycle_commit(AgentLifecycleState::Closing);

    let error = handle.close(root.clone()).await.unwrap_err();

    assert!(error.to_string().contains("lifecycle commit failed"));
    assert_eq!(
        repository.state(&root).snapshot.lifecycle,
        AgentLifecycleState::Active
    );
    assert_eq!(host.lifecycle.close_order.lock().unwrap().len(), 0);
    assert_eq!(
        host.lifecycle.close_rollbacks.lock().unwrap().as_slice(),
        &[root]
    );
    runtime.shutdown().await.unwrap();
}

#[tokio::test]
async fn external_close_failure_rolls_back_and_restores_active_state() {
    let repository = TestRepository::empty();
    let host = TestHost::new(repository.clone(), FactoryMode::Fail);
    let runtime = AgentRuntime::start(host.clone(), test_options())
        .await
        .unwrap();
    let handle = runtime.handle();
    let root = AgentId::new("root").unwrap();
    handle.register(registration("root", "chat")).await.unwrap();
    host.lifecycle.fail_next_commit_close();

    let error = handle.close(root.clone()).await.unwrap_err();

    assert!(error.to_string().contains("commit close failed"));
    assert_eq!(
        handle.snapshot(root.clone()).await.unwrap().lifecycle,
        AgentLifecycleState::Active
    );
    assert_eq!(
        repository.state(&root).snapshot.lifecycle,
        AgentLifecycleState::Active
    );
    assert_eq!(
        host.lifecycle.close_rollbacks.lock().unwrap().as_slice(),
        &[root]
    );
    runtime.shutdown().await.unwrap();
}

#[tokio::test]
async fn failed_close_compensation_faults_agent_durably() {
    let repository = TestRepository::empty();
    let host = TestHost::new(repository.clone(), FactoryMode::Fail);
    let runtime = AgentRuntime::start(host.clone(), test_options())
        .await
        .unwrap();
    let handle = runtime.handle();
    let root = AgentId::new("root").unwrap();
    handle.register(registration("root", "chat")).await.unwrap();
    host.lifecycle.fail_next_commit_close();
    host.lifecycle.fail_next_rollback_close();

    let error = handle.close(root.clone()).await.unwrap_err();

    assert!(error.to_string().contains("close rollback failed"));
    assert_eq!(
        handle.snapshot(root.clone()).await.unwrap().lifecycle,
        AgentLifecycleState::Faulted
    );
    assert_eq!(
        repository.state(&root).snapshot.lifecycle,
        AgentLifecycleState::Faulted
    );
    assert_eq!(
        handle
            .wait_until_idle(root)
            .await
            .unwrap()
            .snapshot
            .lifecycle,
        AgentLifecycleState::Faulted
    );
    runtime.shutdown().await.unwrap();
}

#[tokio::test]
async fn closed_state_commit_failure_rolls_back_external_close() {
    let repository = TestRepository::empty();
    let host = TestHost::new(repository.clone(), FactoryMode::Fail);
    let runtime = AgentRuntime::start(host.clone(), test_options())
        .await
        .unwrap();
    let handle = runtime.handle();
    let root = AgentId::new("root").unwrap();
    handle.register(registration("root", "chat")).await.unwrap();
    repository.fail_next_lifecycle_commit(AgentLifecycleState::Closed);

    let error = handle.close(root.clone()).await.unwrap_err();

    assert!(error.to_string().contains("lifecycle commit failed"));
    assert_eq!(
        repository.state(&root).snapshot.lifecycle,
        AgentLifecycleState::Active
    );
    assert_eq!(
        host.lifecycle.close_order.lock().unwrap().as_slice(),
        std::slice::from_ref(&root)
    );
    assert_eq!(
        host.lifecycle.close_rollbacks.lock().unwrap().as_slice(),
        &[root]
    );
    runtime.shutdown().await.unwrap();
}

#[tokio::test]
async fn closing_running_agent_durably_records_cancelled_outcome() {
    let repository = TestRepository::empty();
    let host = TestHost::new(repository.clone(), FactoryMode::Block);
    let runtime = AgentRuntime::start(host, test_options()).await.unwrap();
    let handle = runtime.handle();
    let root = AgentId::new("root").unwrap();
    handle.register(registration("root", "chat")).await.unwrap();
    handle
        .submit(
            root.clone(),
            AgentSubmitRequest::start(ThreadId::new("root").unwrap(), "block"),
        )
        .await
        .unwrap();

    let snapshot = handle.close(root.clone()).await.unwrap();

    assert_eq!(snapshot.lifecycle, AgentLifecycleState::Closed);
    assert_eq!(
        snapshot.last_turn.as_ref().unwrap().kind,
        TurnOutcomeKind::Cancelled
    );
    assert_eq!(
        snapshot.last_turn.as_ref().unwrap().reason.as_deref(),
        Some("agent_close_requested")
    );
    assert_eq!(repository.state(&root).snapshot, snapshot);
    runtime.shutdown().await.unwrap();
}
