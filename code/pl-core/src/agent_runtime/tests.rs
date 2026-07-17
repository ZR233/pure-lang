use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use pretty_assertions::assert_eq;
use tokio::sync::Notify;

use super::*;

#[derive(Debug, Clone)]
struct TestError(String);

impl fmt::Display for TestError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl std::error::Error for TestError {}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FactoryMode {
    Fail,
    Block,
}

#[derive(Clone)]
struct TestRepository {
    states: Arc<Mutex<BTreeMap<AgentId, AgentDurableState>>>,
    fail_trace: Arc<Mutex<bool>>,
    fail_terminal: Arc<Mutex<bool>>,
    fail_registration: Arc<Mutex<bool>>,
    fail_lifecycle: Arc<Mutex<Option<AgentLifecycleState>>>,
}

impl TestRepository {
    fn empty() -> Self {
        Self {
            states: Arc::new(Mutex::new(BTreeMap::new())),
            fail_trace: Arc::new(Mutex::new(false)),
            fail_terminal: Arc::new(Mutex::new(false)),
            fail_registration: Arc::new(Mutex::new(false)),
            fail_lifecycle: Arc::new(Mutex::new(None)),
        }
    }

    fn with_state(state: AgentDurableState) -> Self {
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

    fn fail_next_trace_commit(&self) {
        *self.fail_trace.lock().unwrap() = true;
    }

    fn fail_next_registration(&self) {
        *self.fail_registration.lock().unwrap() = true;
    }

    fn fail_next_lifecycle_commit(&self, lifecycle: AgentLifecycleState) {
        *self.fail_lifecycle.lock().unwrap() = Some(lifecycle);
    }

    fn state(&self, id: &AgentId) -> AgentDurableState {
        self.states.lock().unwrap()[id].clone()
    }
}

impl AgentStateRepository for TestRepository {
    type Error = TestError;

    async fn restore_runtime(&self) -> std::result::Result<Vec<RestoredAgentRuntime>, Self::Error> {
        Ok(self
            .states
            .lock()
            .unwrap()
            .values()
            .cloned()
            .map(|state| RestoredAgentRuntime { state })
            .collect())
    }

    async fn commit(
        &self,
        commit: AgentCommit,
    ) -> std::result::Result<AgentCommitOutcome, Self::Error> {
        if commit.expected_revision.is_none()
            && std::mem::take(&mut *self.fail_registration.lock().unwrap())
        {
            return Err(TestError("registration commit failed".to_string()));
        }
        if !commit.trace_events.is_empty() && std::mem::take(&mut *self.fail_trace.lock().unwrap())
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
        if *self.fail_terminal.lock().unwrap()
            && commit
                .events
                .iter()
                .any(|event| matches!(event.kind, AgentRuntimeEventKind::TurnFinished { .. }))
        {
            return Err(TestError("terminal commit failed".to_string()));
        }
        let mut states = self.states.lock().unwrap();
        let actual = states
            .get(&commit.agent_id)
            .map(|state| state.snapshot.revision);
        if actual != commit.expected_revision {
            return Ok(AgentCommitOutcome::RevisionConflict {
                actual_revision: actual,
            });
        }
        states.insert(commit.agent_id, commit.next_state);
        Ok(AgentCommitOutcome::Applied)
    }
}

#[derive(Clone)]
struct TestTurnFactory {
    mode: FactoryMode,
    prepared_messages: Arc<Mutex<Vec<String>>>,
    blocker: Arc<Notify>,
}

impl TestTurnFactory {
    fn new(mode: FactoryMode) -> Self {
        Self {
            mode,
            prepared_messages: Arc::new(Mutex::new(Vec::new())),
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
        self.prepared_messages
            .lock()
            .unwrap()
            .push(context.input.message);
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

    fn trace_len(&self) -> usize {
        self.traces.lock().unwrap().len()
    }
}

impl AgentEventSink for TestEvents {
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
    type Events = TestEvents;

    fn repository(&self) -> &Self::Repository {
        &self.repository
    }

    fn turn_factory(&self) -> &Self::TurnFactory {
        &self.turn_factory
    }

    fn lifecycle(&self) -> &Self::Lifecycle {
        &self.lifecycle
    }

    fn events(&self) -> &Self::Events {
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

fn registration(id: &str, session: &str) -> AgentRegistration {
    AgentRegistration::with_session(identity(id), SessionId::new(session).unwrap())
}

fn test_options() -> AgentRuntimeOptions {
    AgentRuntimeOptions {
        command_capacity: 32,
        cancel_grace: Duration::from_millis(10),
        restored_inputs: RestoredInputPolicy::Start,
    }
}

fn child_spawn_request(parent_id: AgentId) -> AgentSpawnRequest {
    AgentSpawnRequest {
        parent_id,
        role: crate::AgentRoleId::new("worker").unwrap(),
        session: AgentSessionState::empty(SessionId::new("child-chat").unwrap()),
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
            AgentSubmitRequest::start(SessionId::new("chat").unwrap(), "hello"),
        )
        .await
        .unwrap();
    let waited = handle.wait(agent_id.clone()).await.unwrap();

    assert_eq!(waited.snapshot.lifecycle, AgentLifecycleState::Active);
    assert_eq!(waited.snapshot.activity, AgentActivityState::Idle);
    assert_eq!(waited.last_turn.unwrap().kind, TurnOutcomeKind::Failed);
    assert_eq!(repository.state(&agent_id).snapshot, waited.snapshot);
    assert_eq!(host.events.runtime_len(), 4);
    runtime.shutdown().await.unwrap();
}

#[tokio::test]
async fn queue_only_inputs_preserve_fifo_when_a_later_input_starts_queue() {
    let host = TestHost::new(TestRepository::empty(), FactoryMode::Fail);
    let runtime = AgentRuntime::start(host.clone(), test_options())
        .await
        .unwrap();
    let handle = runtime.handle();
    let agent_id = AgentId::new("root").unwrap();
    let session_id = SessionId::new("chat").unwrap();
    handle.register(registration("root", "chat")).await.unwrap();

    for message in ["first", "second"] {
        handle
            .submit(
                agent_id.clone(),
                AgentSubmitRequest::start(session_id.clone(), message)
                    .with_delivery(InputDelivery::QueueOnly),
            )
            .await
            .unwrap();
    }
    handle
        .submit(
            agent_id.clone(),
            AgentSubmitRequest::start(session_id, "third"),
        )
        .await
        .unwrap();
    handle.wait(agent_id).await.unwrap();

    assert_eq!(
        host.turn_factory.prepared_messages.lock().unwrap().clone(),
        vec![
            "first".to_string(),
            "second".to_string(),
            "third".to_string()
        ]
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
    let session_id = SessionId::new("chat").unwrap();
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
    handle.wait(agent_id).await.unwrap();

    let prepared = host.turn_factory.prepared_messages.lock().unwrap().clone();
    assert_eq!(turn_ids.len(), 16);
    assert_eq!(prepared.len(), 16);
    assert_eq!(prepared.into_iter().collect::<BTreeSet<_>>().len(), 16);
    runtime.shutdown().await.unwrap();
}

#[tokio::test]
async fn interrupt_then_start_preempts_the_existing_fifo_queue() {
    let host = TestHost::new(TestRepository::empty(), FactoryMode::Block);
    let runtime = AgentRuntime::start(host.clone(), test_options())
        .await
        .unwrap();
    let handle = runtime.handle();
    let agent_id = AgentId::new("root").unwrap();
    let session_id = SessionId::new("chat").unwrap();
    handle.register(registration("root", "chat")).await.unwrap();

    handle
        .submit(
            agent_id.clone(),
            AgentSubmitRequest::start(session_id.clone(), "first"),
        )
        .await
        .unwrap();
    wait_for_prepared_messages(&host.turn_factory, 1).await;
    handle
        .submit(
            agent_id.clone(),
            AgentSubmitRequest::start(session_id.clone(), "later"),
        )
        .await
        .unwrap();
    handle
        .submit(
            agent_id.clone(),
            AgentSubmitRequest::start(session_id, "urgent")
                .with_delivery(InputDelivery::InterruptThenStart),
        )
        .await
        .unwrap();

    wait_for_prepared_messages(&host.turn_factory, 2).await;
    host.turn_factory.blocker.notify_one();
    wait_for_prepared_messages(&host.turn_factory, 3).await;
    host.turn_factory.blocker.notify_one();
    handle
        .wait_timeout(agent_id, Duration::from_secs(1))
        .await
        .unwrap();

    assert_eq!(
        host.turn_factory.prepared_messages.lock().unwrap().clone(),
        vec![
            "first".to_string(),
            "urgent".to_string(),
            "later".to_string()
        ]
    );
    runtime.shutdown().await.unwrap();
}

#[tokio::test]
async fn activity_updates_are_durable_and_stale_turn_updates_are_ignored() {
    let repository = TestRepository::empty();
    let host = TestHost::new(repository.clone(), FactoryMode::Block);
    let runtime = AgentRuntime::start(host, test_options()).await.unwrap();
    let handle = runtime.handle();
    let agent_id = AgentId::new("root").unwrap();
    handle.register(registration("root", "chat")).await.unwrap();
    let turn_id = handle
        .submit(
            agent_id.clone(),
            AgentSubmitRequest::start(SessionId::new("chat").unwrap(), "block"),
        )
        .await
        .unwrap();

    handle
        .set_activity(
            agent_id.clone(),
            turn_id.clone(),
            AgentActivityState::WaitingTool,
        )
        .await
        .unwrap();
    assert_eq!(
        repository.state(&agent_id).snapshot.activity,
        AgentActivityState::WaitingTool
    );
    handle
        .set_activity(
            agent_id.clone(),
            turn_id.clone(),
            AgentActivityState::WaitingInteraction,
        )
        .await
        .unwrap();
    assert_eq!(
        repository.state(&agent_id).snapshot.activity,
        AgentActivityState::WaitingInteraction
    );

    handle
        .cancel_turn(agent_id.clone(), turn_id.clone())
        .await
        .unwrap();
    handle
        .wait_timeout(agent_id.clone(), Duration::from_secs(1))
        .await
        .unwrap();
    let terminal_revision = repository.state(&agent_id).snapshot.revision;
    handle
        .set_activity(agent_id.clone(), turn_id, AgentActivityState::Running)
        .await
        .unwrap();

    assert_eq!(
        repository.state(&agent_id).snapshot.revision,
        terminal_revision
    );
    runtime.shutdown().await.unwrap();
}

#[tokio::test]
async fn trace_is_durable_before_broadcast_and_commit_failure_faults_actor() {
    use super::actor::{ActorCommand, spawn_agent_actor};

    let session_id = SessionId::new("chat").unwrap();
    let state = registration("root", session_id.as_str()).into_durable_state();
    let repository = TestRepository::with_state(state.clone());
    let host = TestHost::new(repository.clone(), FactoryMode::Block);
    let (runtime_sender, _runtime_receiver) = tokio::sync::mpsc::channel(1);
    let actor = spawn_agent_actor(
        host.clone(),
        state,
        AgentRuntimeHandle::new(runtime_sender),
        Duration::from_millis(10),
        false,
        32,
    );
    let (submit_reply, submit_receiver) = tokio::sync::oneshot::channel();
    actor
        .send(ActorCommand::Submit {
            request: AgentSubmitRequest::start(session_id.clone(), "block"),
            reply: submit_reply,
        })
        .await
        .unwrap();
    let turn_id = submit_receiver.await.unwrap().unwrap();
    wait_for_prepared_messages(&host.turn_factory, 1).await;

    let trace = |sequence: u64, suffix: &str| pl_trace::TraceEvent {
        session_id: session_id.to_string(),
        sequence,
        timestamp: 1,
        kind: pl_trace::TraceEventKind::TracePartStarted {
            item: pl_trace::TracePart::text(
                turn_id.as_str(),
                format!("trace-{suffix}"),
                sequence,
                pl_trace::TraceTextChannel::Commentary,
                suffix.to_string(),
                pl_trace::TracePartStatus::Started,
                1,
            ),
        },
    };
    actor.record_trace(trace(0, "first")).unwrap();
    tokio::time::timeout(Duration::from_secs(1), async {
        while host.events.trace_len() != 1 {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    assert_eq!(
        repository.state(&AgentId::new("root").unwrap()).sessions[&session_id].trace_sequence,
        1
    );
    assert_eq!(
        repository
            .state(&AgentId::new("root").unwrap())
            .snapshot
            .last_turn,
        None
    );

    repository.fail_next_trace_commit();
    actor.record_trace(trace(1, "second")).unwrap();
    let snapshot = tokio::time::timeout(Duration::from_secs(1), async {
        loop {
            let (reply, receiver) = tokio::sync::oneshot::channel();
            actor.send(ActorCommand::Snapshot { reply }).await.unwrap();
            let snapshot = receiver.await.unwrap().unwrap();
            if snapshot.lifecycle == AgentLifecycleState::Faulted {
                break snapshot;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    assert_eq!(snapshot.activity, AgentActivityState::Idle);

    let (submit_reply, submit_receiver) = tokio::sync::oneshot::channel();
    actor
        .send(ActorCommand::Submit {
            request: AgentSubmitRequest::start(session_id.clone(), "rejected"),
            reply: submit_reply,
        })
        .await
        .unwrap();
    assert!(matches!(
        submit_receiver.await.unwrap(),
        Err(AgentRuntimeError::NotActive(
            _,
            AgentLifecycleState::Faulted
        ))
    ));

    let (shutdown_reply, shutdown_receiver) = tokio::sync::oneshot::channel();
    actor
        .send(ActorCommand::Shutdown {
            reply: shutdown_reply,
        })
        .await
        .unwrap();
    shutdown_receiver.await.unwrap().unwrap();
}

#[tokio::test]
async fn cancellation_aborts_blocked_turn_after_grace_and_records_cancelled_outcome() {
    let repository = TestRepository::empty();
    let host = TestHost::new(repository.clone(), FactoryMode::Block);
    let runtime = AgentRuntime::start(host, test_options()).await.unwrap();
    let handle = runtime.handle();
    let agent_id = AgentId::new("root").unwrap();
    let mut registration = registration("root", "chat");
    registration.sessions[0]
        .session
        .push_user_prompt("已有上下文".to_string());
    handle.register(registration).await.unwrap();
    let turn_id = handle
        .submit(
            agent_id.clone(),
            AgentSubmitRequest::start(SessionId::new("chat").unwrap(), "block"),
        )
        .await
        .unwrap();

    handle.cancel_turn(agent_id.clone(), turn_id).await.unwrap();
    let waited = handle
        .wait_timeout(agent_id, Duration::from_secs(1))
        .await
        .unwrap();

    assert_eq!(waited.last_turn.unwrap().kind, TurnOutcomeKind::Cancelled);
    assert_eq!(
        repository.state(&AgentId::new("root").unwrap()).sessions[&SessionId::new("chat").unwrap()]
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
            AgentSubmitRequest::start(SessionId::new("chat").unwrap(), "block"),
        )
        .await
        .unwrap();

    runtime.shutdown().await.unwrap();

    let state = repository.state(&agent_id);
    assert_eq!(state.snapshot.activity, AgentActivityState::Idle);
    assert_eq!(state.snapshot.active_turn_id, None);
    assert_eq!(state.snapshot.active_session_id, None);
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
            AgentSubmitRequest::start(SessionId::new("chat").unwrap(), "fail terminal"),
        )
        .await
        .unwrap();
    let snapshot = handle
        .wait_timeout(agent_id.clone(), Duration::from_secs(1))
        .await
        .unwrap()
        .snapshot;

    assert_eq!(snapshot.lifecycle, AgentLifecycleState::Faulted);
    let error = handle
        .submit(
            agent_id.clone(),
            AgentSubmitRequest::start(SessionId::new("chat").unwrap(), "again"),
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
async fn restart_recovery_cancels_running_turn_before_actor_registration() {
    let mut state = registration("root", "chat").into_durable_state();
    state.snapshot.revision = 7;
    state.snapshot.event_sequence = 11;
    state.snapshot.activity = AgentActivityState::Running;
    state.snapshot.active_turn_id = Some(TurnId::new("old-turn").unwrap());
    state.snapshot.active_session_id = Some(SessionId::new("chat").unwrap());
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
    let session_id = SessionId::new("chat").unwrap();
    let mut state = registration("root", "chat").into_durable_state();
    for (index, message) in ["first", "second"].into_iter().enumerate() {
        state.pending_inputs.push_back(PendingAgentInput {
            turn_id: TurnId::new(format!("turn-{index}")).unwrap(),
            session_id: session_id.clone(),
            message: message.to_string(),
            metadata: serde_json::Value::Null,
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
    runtime.handle().wait(agent_id.clone()).await.unwrap();

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
    let session_id = SessionId::new("chat").unwrap();
    let mut state = registration("root", "chat").into_durable_state();
    state.pending_inputs.push_back(PendingAgentInput {
        turn_id: TurnId::new("restored-turn").unwrap(),
        session_id,
        message: "after-resources-ready".to_string(),
        metadata: serde_json::Value::Null,
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
    runtime.handle().wait(agent_id).await.unwrap();
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
            parent_id: root.clone(),
            role: crate::AgentRoleId::new("worker").unwrap(),
            session: AgentSessionState::empty(SessionId::new("child-chat").unwrap()),
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
            parent_id: child.clone(),
            role: crate::AgentRoleId::new("worker").unwrap(),
            session: AgentSessionState::empty(SessionId::new("grandchild-chat").unwrap()),
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
        handle.wait(root).await.unwrap().snapshot.lifecycle,
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
            AgentSubmitRequest::start(SessionId::new("chat").unwrap(), "block"),
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

#[tokio::test]
async fn open_session_is_durable_and_idempotent() {
    let repository = TestRepository::empty();
    let host = TestHost::new(repository.clone(), FactoryMode::Fail);
    let runtime = AgentRuntime::start(host, test_options()).await.unwrap();
    let handle = runtime.handle();
    let agent_id = AgentId::new("root").unwrap();
    handle.register(registration("root", "chat")).await.unwrap();
    let mut session = AgentSessionState::empty(SessionId::new("second").unwrap());
    session.metadata = serde_json::json!({ "title": "第二段会话" });

    handle
        .open_session(agent_id.clone(), session.clone())
        .await
        .unwrap();
    handle
        .open_session(agent_id.clone(), session)
        .await
        .unwrap();

    let state = repository.state(&agent_id);
    assert_eq!(state.sessions.len(), 2);
    assert_eq!(state.snapshot.revision, 2);
    assert_eq!(
        state.sessions[&SessionId::new("second").unwrap()].metadata,
        serde_json::json!({ "title": "第二段会话" })
    );
    runtime.shutdown().await.unwrap();
}
