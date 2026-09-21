//! Owned, retryable product projections of the durable Thread effect window.
//!
//! Observation consumes live `effect_page` batches behind a `Thread::flush` durability barrier and
//! reads history/report/directory facts only from durable storage.
mod billing;
mod directory;
mod reports;

use super::{ModelPerformanceOwner, StudioRuntime};
use crate::studio::{ProductEventBus, StudioStore};
use anyhow::{Context, Result, bail};
use pl_core::thread::{
    ThreadEffectBatch, ThreadError, ThreadHandle, ThreadLifecycle, ThreadSnapshot,
};
use std::{
    collections::BTreeMap,
    num::NonZeroUsize,
    sync::{Arc, Mutex},
};
use tokio::{
    sync::{Notify, watch},
    task::JoinHandle,
};

#[derive(Clone, Default)]
struct Progress {
    initialized: bool,
    applied: u64,
    error: Option<Arc<anyhow::Error>>,
    finished: bool,
}
struct WorkerState {
    progress: watch::Sender<Progress>,
    retry: Notify,
}
struct Observation {
    thread: ThreadHandle,
    state: Arc<WorkerState>,
    task: Mutex<Option<JoinHandle<()>>>,
}
impl Drop for Observation {
    fn drop(&mut self) {
        if let Some(task) = self
            .task
            .get_mut()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .take()
        {
            task.abort();
        }
    }
}
#[derive(Clone)]
pub(super) struct ObservationServices {
    pub(super) store: StudioStore,
    pub(super) events: ProductEventBus,
    pub(super) performance: ModelPerformanceOwner,
    pub(super) threads: crate::thread_assembler::StudioThreadAssembler,
    pub(super) writer: crate::studio::agent_host::ThreadWriteBehindWriter,
}
struct Shared {
    projector: ObservationServices,
    observations: Mutex<BTreeMap<String, Vec<Arc<Observation>>>>,
}
#[derive(Clone)]
pub(super) struct ThreadObservations(Arc<Shared>);

impl ThreadObservations {
    pub(super) fn new(projector: ObservationServices) -> Self {
        Self(Arc::new(Shared {
            projector,
            observations: Mutex::new(BTreeMap::new()),
        }))
    }

    pub(super) fn install(
        &self,
        thread_factory: crate::studio::thread_factory::StudioThreadFactory,
    ) -> Result<()> {
        let owner = Arc::downgrade(&self.0);
        let draining = owner.clone();
        let message_store = self.0.projector.store.clone();
        self.0.projector.threads.observe_assembly(
            move |id, thread| {
                if let Some(owner) = owner.upgrade() {
                    ThreadObservations(owner).observe(id, thread);
                }
            },
            move |id| {
                let owner = draining.clone();
                let thread_factory = thread_factory.clone();
                Box::pin(async move {
                    let owner = owner
                        .upgrade()
                        .ok_or(crate::thread_assembler::ThreadAssemblyError::Closed)?;
                    let observations = ThreadObservations(owner);
                    observations
                        .synchronize(Some(&id))
                        .await
                        .map_err(|source| {
                            crate::thread_assembler::ThreadAssemblyError::Resource {
                                operation: "drain Thread product observation",
                                source: source.into_boxed_dyn_error(),
                            }
                        })?;
                    thread_factory.forget_tool_binding(&id);
                    observations.drain(&id);
                    Ok(())
                })
            },
            // 一条已受理消息的持久身份：per-Thread 身份索引给出正文摘要与原始受理序号，因此离开
            // 常驻窗口的重复投递会被裁决而不是再送一次。索引行缺少摘要（迁移回填）或时间线已提交
            // 但索引无法证明正文的旧消息都按“不可验证”上报，让上层 fail-closed；查询失败同样上报，
            // 绝不在无法证明身份时把投递当成新消息受理。
            Some(Arc::new(move |thread_id: String, message_id: String| {
                let store = message_store.clone();
                Box::pin(async move {
                    let identity_error = |source: anyhow::Error| {
                        crate::thread_assembler::ThreadAssemblyError::Resource {
                            operation: "read durable message identity",
                            source: source.into_boxed_dyn_error(),
                        }
                    };
                    let history = store.history(&thread_id).await.map_err(identity_error)?;
                    if let Some(identity) =
                        history.message_identity(&message_id).await.map_err(identity_error)?
                    {
                        return Ok(Some(match identity.digest {
                            Some(digest) => {
                                crate::thread_assembler::observation::DurableMessageIdentity::Proven {
                                    sequence: identity.sequence,
                                    digest,
                                }
                            }
                            None => crate::thread_assembler::observation::DurableMessageIdentity::Unverifiable,
                        }));
                    }
                    // 时间线已经提交这条消息、但索引没有它的记录：升级前的历史无法证明正文，
                    // 必须 fail-closed，而不是重新投递成新消息。
                    let item = crate::studio::thread_projection::order::message_id(&message_id);
                    let committed = history
                        .existing_items([item])
                        .await
                        .map_err(identity_error)?;
                    Ok((!committed.is_empty()).then_some(
                        crate::thread_assembler::observation::DurableMessageIdentity::Unverifiable,
                    ))
                })
            })),
        )?;
        Ok(())
    }

    /// Releases the retained observation for a drained or evicted Thread.
    ///
    /// The observation worker owns a strong `ThreadHandle`; keeping its entry after the assembly
    /// registry released the owner would keep every session that was ever activated resident. The
    /// entry is removed once its final projection is durable, so a later re-activation installs a
    /// fresh observation instead of reusing the released handle.
    fn drain(&self, id: &str) {
        let mut all = self
            .0
            .observations
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        all.remove(id);
    }

    fn observe(&self, id: String, thread: ThreadHandle) {
        let recovered_through = thread.snapshot().commit_sequence;
        let (progress, _) = watch::channel(Progress {
            applied: recovered_through,
            ..Progress::default()
        });
        let state = Arc::new(WorkerState {
            progress,
            retry: Notify::new(),
        });
        let worker_state = state.clone();
        let projector = self.0.projector.clone();
        let worker_thread = thread.clone();
        let worker_id = id.clone();
        let task = tokio::spawn(async move {
            run(
                projector,
                worker_id,
                worker_thread,
                &worker_state,
                recovered_through,
            )
            .await;
        });
        let observation = Arc::new(Observation {
            thread,
            state,
            task: Mutex::new(Some(task)),
        });
        let mut all = self
            .0
            .observations
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let previous = all.entry(id).or_default();
        previous.retain(|entry| {
            let progress = entry.state.progress.borrow();
            !progress.finished || progress.error.is_some()
        });
        previous.push(observation);
    }

    async fn synchronize(&self, id: Option<&str>) -> Result<()> {
        self.0.projector.writer.retry_now();
        let observations: Vec<_> = self
            .0
            .observations
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .iter()
            .filter(|(key, _)| id.is_none_or(|id| id == key.as_str()))
            .flat_map(|(id, entries)| entries.iter().map(|entry| (id.clone(), entry.clone())))
            .collect();
        for (id, observation) in observations {
            let target_snapshot = observation.thread.snapshot();
            let target = target_snapshot.commit_sequence;
            let mut progress = observation.state.progress.subscribe();
            observation.state.progress.send_modify(|progress| {
                if !progress.finished {
                    progress.error = None;
                }
            });
            observation.state.retry.notify_one();
            loop {
                let state = progress.borrow().clone();
                if let Some(error) = state.error {
                    bail!("Thread {id} product observation failed: {error:#}");
                }
                if state.initialized
                    && state.applied >= target
                    && (target_snapshot.lifecycle != ThreadLifecycle::Closed || state.finished)
                {
                    break;
                }
                if state.finished {
                    bail!("Thread {id} product observer ended before commit {target}");
                }
                progress
                    .changed()
                    .await
                    .context("Thread product observer progress closed")?;
            }
        }
        Ok(())
    }

    /// Explicit parent continuation repairs terminal notifications even for unloaded children.
    /// This reads a checkpoint plus durable history only; no child model or workspace is opened.
    pub(super) async fn reconcile_children(&self, parent: &str) -> Result<()> {
        let services = &self.0.projector;
        for child in services.store.list_threads_for_root(parent).await? {
            if child.parent_thread_id.as_deref() != Some(parent) {
                continue;
            }
            if services.threads.thread(&child.id).is_some() {
                self.synchronize(Some(&child.id)).await?;
                continue;
            }
            let child = pl_protocol::Thread::from(child);
            let Some(checkpoint) =
                crate::studio::thread_factory::recovery::load_checkpoint(&services.store, &child)
                    .await?
            else {
                continue;
            };
            if let Some(turn) = checkpoint
                .state
                .turns
                .iter()
                .rev()
                .find(|turn| turn.state != pl_core::thread::TurnState::Running)
            {
                reports::publish_terminal(
                    services,
                    &child,
                    &checkpoint.state,
                    turn,
                    checkpoint.state_revision,
                    false,
                )
                .await?;
            }
        }
        Ok(())
    }

    pub(super) async fn finish(&self) -> Result<()> {
        self.synchronize(None).await?;
        let observations: Vec<_> = self
            .0
            .observations
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .values()
            .flatten()
            .cloned()
            .collect();
        for observation in observations {
            let mut progress = observation.state.progress.subscribe();
            loop {
                let state = progress.borrow().clone();
                if let Some(error) = state.error {
                    bail!("Thread product observer failed: {error:#}");
                }
                if state.finished {
                    break;
                }
                progress
                    .changed()
                    .await
                    .context("Thread product observer ended without final state")?;
            }
            let task = observation
                .task
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .take();
            if let Some(task) = task {
                task.await.context("Thread product observer join failed")?;
            }
        }
        Ok(())
    }
}

impl StudioRuntime {
    /// Waits for product directories and billing to reach the current committed Thread watermark.
    /// Reattempts a previously failed projection without invoking models or tools.
    pub async fn synchronize_thread_observation(&self, thread_id: &str) -> Result<()> {
        self.thread_observations.synchronize(Some(thread_id)).await
    }
}

/// Rebuilds the terminal and billing facts the dropped effects carried, from durable history/calls.
///
/// The bounded effect window is an observation channel, not a durable log. When it rotates past a
/// lagging observer the effects are gone, but their facts remain durable: terminal Turns in
/// `history.sqlite` and model-call facts in `calls.sqlite`. This republishes only what the effects
/// still folded into this batch do not already cover. The parent's wake dedup makes a repeat of an
/// already-delivered Turn a no-op, and the call index is idempotent by call identity, so this runs
/// only on the loss path and never becomes a second fact source. Root and child Threads take the
/// same path: a root has no parent to wake, but its billing facts are recovered the same way.
async fn rebuild_skipped_facts(
    projector: &ObservationServices,
    id: &str,
    snapshot: &ThreadSnapshot,
    folded: &[Arc<ThreadEffectBatch>],
    skipped: std::ops::RangeInclusive<u64>,
) -> Result<()> {
    let mut known = folded
        .iter()
        .filter_map(|effect| effect.turn.as_ref().map(|turn| turn.turn_id.clone()))
        .collect::<std::collections::BTreeSet<_>>();
    let product = projector
        .store
        .read_thread(id)
        .await?
        .map(pl_protocol::Thread::from)
        .context("observed Thread has no product association")?;
    // 计费/调用事实对每个 Thread 都从 durable calls 有界恢复，不只处理子 Thread。
    recover_skipped_billing(projector, id, &product, skipped).await?;
    // 有界恢复：按 ordinal 游标逐页读取 durable 的终态 Turn，既不全表扫描，也不因为丢失 effect
    // 而静默跳过 continuation。历史只保存协议形态，这里映射回 runtime 的 core 形态再发出报告。
    let history = projector.store.history(id).await?;
    // 根 Thread 没有父可唤醒，但产品目录与终态派生同样必须来自 durable terminal 事实，而不是
    // 可能已裁剪或仍显示 Running 的 snapshot。这里只读最新一条终态 Turn（ordinal 主键逆序
    // LIMIT 1，有界），缺口里更老的终态属于历史，不需要重放。
    if product.parent_thread_id.is_none() {
        if let Some(item) = history.latest_terminal_turn().await? {
            let pl_protocol::ThreadItemState::Turn(turn) = item.state() else {
                return Ok(());
            };
            let Some(state) = terminal_turn_state(turn.state()) else {
                return Ok(());
            };
            let turn_id = turn_item_id(&item.id);
            if !known.contains(&turn_id) {
                let record = pl_core::thread::TurnRecord {
                    elapsed_ms: None,
                    input_id: turn.input_id().map(str::to_owned),
                    turn_id,
                    state,
                    model_steps: 0,
                };
                directory::publish_recovered_terminal(
                    projector,
                    product,
                    snapshot,
                    &record,
                    item.updated_at,
                )
                .await?;
            }
        }
        return Ok(());
    }
    const TERMINAL_TURN_PAGE: usize = 256;
    let mut after_ordinal = 0_u64;
    loop {
        let page = history
            .terminal_turns_after(after_ordinal, TERMINAL_TURN_PAGE)
            .await?;
        if page.is_empty() {
            break;
        }
        let read = page.len();
        for item in page {
            after_ordinal = after_ordinal.max(item.ordinal);
            let pl_protocol::ThreadItemState::Turn(turn) = item.state() else {
                continue;
            };
            let Some(state) = terminal_turn_state(turn.state()) else {
                continue;
            };
            let turn_id = turn_item_id(&item.id);
            if !known.insert(turn_id.clone()) {
                continue;
            }
            let record = pl_core::thread::TurnRecord {
                elapsed_ms: None,
                input_id: turn.input_id().map(str::to_owned),
                turn_id,
                state,
                model_steps: 0,
            };
            reports::publish_terminal(projector, &product, snapshot, &record, item.ordinal, false)
                .await?;
        }
        if read < TERMINAL_TURN_PAGE {
            break;
        }
    }
    Ok(())
}

/// Canonical Turn identity carried by a `history.sqlite` Turn item id.
fn turn_item_id(item_id: &str) -> String {
    item_id
        .split_once(':')
        .and_then(|(_, rest)| rest.split_once(':'))
        .map_or(item_id, |(_, turn_id)| turn_id)
        .to_owned()
}

/// Re-records the durable billing facts of the effects a window gap dropped.
///
/// The call index is the durable source and `(thread_id, call_id)` its identity, so re-admitting a
/// fact that already exists is idempotent. Only a bounded revision window is read per page, and the
/// revision cursor advances with the facts themselves, so a lagging observer never has to scan the
/// whole call history to come back in sync.
async fn recover_skipped_billing(
    projector: &ObservationServices,
    id: &str,
    product: &pl_protocol::Thread,
    skipped: std::ops::RangeInclusive<u64>,
) -> Result<()> {
    const BILLING_PAGE: usize = 256;
    let root = product.root_thread_id.clone();
    let mut after = skipped.start().saturating_sub(1);
    loop {
        let facts = projector
            .store
            .calls()
            .model_call_facts_between(id, after, *skipped.end(), BILLING_PAGE)
            .await?;
        if facts.is_empty() {
            break;
        }
        let read = facts.len();
        for fact in facts {
            after = after.max(fact.revision);
            let Some(billing) = durable_billing(&fact) else {
                continue;
            };
            if fact.retention.as_deref() == Some("internal") {
                projector
                    .performance
                    .record_auxiliary_inference(&root, id, &billing)?;
            } else {
                projector
                    .performance
                    .record_inference(&root, id, &billing)?;
            }
        }
        if read < BILLING_PAGE {
            break;
        }
    }
    Ok(())
}

/// Rebuilds one billing record from its durable call fact, or `None` when the fact has no identity.
fn durable_billing(
    fact: &crate::studio::storage::calls::ModelCallFact,
) -> Option<pl_protocol::InferenceBillingRecord> {
    if fact.call_id.is_empty() {
        return None;
    }
    let estimated = match (fact.cost_currency.clone(), fact.cost_amount) {
        (Some(currency), Some(amount)) => Some(pl_protocol::RuntimeCostAmount { currency, amount }),
        _ => None,
    };
    let pricing = match estimated {
        Some(cost) => pl_protocol::PricingOutcome::Estimated {
            cost,
            cache_savings: None,
        },
        // 无价格但已被标记为未知价格时保留"不可定价"；其余情况是当时的定价策略是关闭的。
        None if fact.has_unpriced_usage => pl_protocol::PricingOutcome::Unpriced {
            reason: pl_protocol::UnpricedReason::MissingUsage,
        },
        None => pl_protocol::PricingOutcome::Disabled,
    };
    let timing = match (fact.ttft_millis, fact.decode_millis, fact.response_millis) {
        (Some(ttft_millis), Some(decode_millis), Some(total_millis)) => {
            Some(pl_protocol::InferenceTiming {
                ttft_millis,
                decode_millis,
                total_millis,
            })
        }
        _ => None,
    };
    let provider = fact.provider_instance_id.clone().unwrap_or_default();
    Some(pl_protocol::InferenceBillingRecord {
        inference_id: fact.call_id.clone(),
        purpose: fact.purpose.clone(),
        provider_instance_id: provider.clone(),
        provider,
        model: fact.sent_model.clone().unwrap_or_default(),
        model_observation: fact.sent_model.as_ref().map(|sent_model| {
            pl_protocol::InferenceModelObservation {
                configured_model: fact.configured_model.clone().unwrap_or_default(),
                sent_model: sent_model.clone(),
                reported_model: fact.reported_model.clone(),
            }
        }),
        reasoning_effort: fact.reasoning_effort.clone(),
        context_window: None,
        accounting: pl_protocol::InferenceAccounting {
            usage: pl_protocol::UsageReport {
                input_tokens: fact.input_tokens,
                output_tokens: fact.output_tokens,
                cache_read_tokens: fact.cache_read_tokens,
                cache_write_tokens: fact.cache_write_tokens,
                reasoning_tokens: fact.reasoning_tokens,
                total_tokens: fact.total_tokens,
            },
            pricing,
            price_snapshot: None,
            request_started_at: None,
        },
        prompt_generation: None,
        prompt_cache_policy: None,
        prefix_changed_reason: None,
        orchestration: Default::default(),
        timing,
        recorded_at: fact.started_at,
    })
}

/// Reconstructs the runtime core terminal Turn state from the durable protocol Turn state.
///
/// History and the wire carry [`pl_protocol::TurnState`]; the parent wake report carries the core
/// `TurnRecord` the runtime emits for live effects. Mapping back here keeps recovery on one
/// terminal vocabulary instead of inventing a second one, and returns `None` for an open Turn.
fn terminal_turn_state(state: &pl_protocol::TurnState) -> Option<pl_core::thread::TurnState> {
    use pl_core::thread::{TurnOutcome, TurnState as CoreTurnState};
    use pl_protocol::TurnState as WireTurnState;
    match state {
        WireTurnState::Completed(_) => Some(CoreTurnState::Finished(TurnOutcome::Completed)),
        WireTurnState::BudgetLimited(_) => Some(CoreTurnState::Finished(TurnOutcome::StepLimit)),
        WireTurnState::Cancelled(cancelled) => Some(match cancelled.cause() {
            pl_protocol::TurnCancellationCause::Interrupted
            | pl_protocol::TurnCancellationCause::Recovery => CoreTurnState::Interrupted,
            _ => CoreTurnState::Cancelled,
        }),
        WireTurnState::Failed(failed) => Some(CoreTurnState::Failed {
            description: failed.failure().message.clone(),
        }),
        WireTurnState::Queued(_) | WireTurnState::Running(_) => None,
    }
}

async fn run(
    projector: ObservationServices,
    id: String,
    thread: ThreadHandle,
    state: &WorkerState,
    recovered_through: u64,
) {
    let mut updates = thread.subscribe();
    let mut current = thread.snapshot();
    let mut projected = None;
    loop {
        use futures::FutureExt;
        let result = if projected == Some((current.commit_sequence, current.lifecycle)) {
            Ok(())
        } else {
            // The projection stays isolated behind `catch_unwind`, so a panicking projection is
            // still reported as an error instead of taking down the observation worker.
            std::panic::AssertUnwindSafe(project(
                &projector,
                &id,
                &thread,
                &current,
                state,
                recovered_through,
            ))
            .catch_unwind()
            .await
            .unwrap_or_else(|_| Err(anyhow::anyhow!("Thread product projection panicked")))
        };
        match result {
            Ok(()) => {
                projected = Some((current.commit_sequence, current.lifecycle));
                let finished = current.lifecycle == ThreadLifecycle::Closed;
                state.progress.send_replace(Progress {
                    initialized: true,
                    applied: current.commit_sequence,
                    error: None,
                    finished: false,
                });
                if finished {
                    // 固定目标 ticket：只等待本次观察已受理的产品写入，不等待整个系统空闲。
                    let target = projector.writer.admitted_ticket();
                    match projector.writer.flush_through(target).await {
                        Ok(()) => {
                            state
                                .progress
                                .send_modify(|progress| progress.finished = true);
                            return;
                        }
                        Err(error) => {
                            state.progress.send_modify(|progress| {
                                progress.error = Some(Arc::new(anyhow::Error::new(error)));
                                progress.finished = false;
                            });
                        }
                    }
                }
            }
            Err(error) => {
                state
                    .progress
                    .send_modify(|progress| progress.error = Some(Arc::new(error)));
            }
        }
        if current.lifecycle == ThreadLifecycle::Closed {
            state.retry.notified().await;
            current = thread.snapshot();
            continue;
        }
        tokio::select! {
            () = state.retry.notified() => current = thread.snapshot(),
            update = updates.next() => match update {
                Some(snapshot) => current = snapshot,
                None => {
                    state.progress.send_modify(|progress| progress.error = Some(Arc::new(anyhow::anyhow!("Thread subscription ended before its final projection"))));
                    state.retry.notified().await;
                    current = thread.snapshot();
                }
            }
        }
    }
}

async fn project(
    projector: &ObservationServices,
    id: &str,
    thread: &ThreadHandle,
    snapshot: &ThreadSnapshot,
    state: &WorkerState,
    recovered_through: u64,
) -> Result<()> {
    let applied = state.progress.borrow().applied;
    let mut effects = Vec::<Arc<ThreadEffectBatch>>::new();
    let mut after = applied;
    while after < snapshot.commit_sequence {
        let limit = NonZeroUsize::new(128).expect("constant is nonzero");
        let page = match thread.effect_page(after, limit).await {
            Ok(page) => page,
            Err(ThreadError::InvalidContext) => {
                // The live effect window is a transient observation channel: it retains only
                // commits that are not durable yet, so a stalled observer resynchronizes from
                // durable history/calls instead of blocking product projection forever. The
                // skipped range must be durable before it can be read back, and terminal child
                // facts the released effects carried are rebuilt from durable history so the skip
                // never silently loses a continuation.
                thread.flush().await?;
                let Some(head) = thread
                    .effect_window_start()
                    .filter(|head| *head > after + 1)
                else {
                    bail!("Thread {id} effect window did not reach observed watermark");
                };
                tracing::warn!(
                    thread_id = id,
                    after,
                    head,
                    "Thread product observer rebuilding terminal facts the live window no longer retains"
                );
                rebuild_skipped_facts(projector, id, snapshot, &effects, after + 1..=head - 1)
                    .await?;
                after = head - 1;
                continue;
            }
            Err(error) => return Err(error.into()),
        };
        let before = after;
        for effect in page
            .into_iter()
            .take_while(|effect| effect.sequence <= snapshot.commit_sequence)
        {
            after = effect.sequence;
            effects.push(effect);
        }
        if before == after {
            bail!("Thread {id} effect window did not reach observed watermark");
        }
    }
    // History before snapshot publication: the core history for everything this projection is
    // about to publish as a product fact must be durable first. Reading the live window above does
    // not need that fence, so the flush stays here instead of releasing the window before the
    // observer has consumed the commits it is projecting.
    thread.flush().await?;
    let mut product = match projector.events.thread_snapshot(id) {
        Some(thread) => thread,
        None => {
            // An archived hot entry may disappear before its original registration reaches SQLite.
            // Drain the accepted directory writes before resolving the cold association. The target
            // is fixed at admission time; later unrelated writes do not extend this wait.
            let target = projector.writer.admitted_ticket();
            projector.writer.flush_through(target).await?;
            let association = projector
                .store
                .read_thread_association(id)
                .await?
                .context("observed Thread has no product association")?;
            pl_protocol::Thread::from(association)
        }
    };
    for effect in effects.iter().filter(|effect| effect.sequence > applied) {
        billing::record(&projector.performance, &product.root_thread_id, effect)?;
        if let Some(turn) = effect
            .turn
            .as_ref()
            .filter(|turn| turn.state != pl_core::thread::TurnState::Running)
        {
            reports::publish_terminal(
                projector,
                &product,
                snapshot,
                turn,
                effect.sequence,
                effect.sequence > recovered_through,
            )
            .await?;
        }
    }
    product.status = crate::studio::thread_projection::status(snapshot);
    product.updated_at = product
        .updated_at
        .max(effects.last().map_or(0, |effect| effect.committed_at));
    directory::publish(projector, product, snapshot).await?;
    // 本投影已受理的目录/性能/计费写入必须落到固定 ticket 之后，才让 `synchronize` 看到
    // 终态；目标在调用时固定，不等待其他 Thread 或后续写入。
    let target = projector.writer.admitted_ticket();
    projector.writer.flush_through(target).await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use pl_core::{
        context::ContextContent,
        model::{
            DynModelSession, ModelError, ModelRequest, ModelSession, ModelStepOutput,
            PreparedModelCall,
        },
    };
    use pretty_assertions::assert_eq;
    use std::sync::atomic::{AtomicUsize, Ordering};

    struct Reply(Arc<AtomicUsize>);
    impl ModelSession for Reply {
        async fn prepare(
            &mut self,
            request: ModelRequest,
        ) -> Result<PreparedModelCall, ModelError> {
            let calls = self.0.clone();
            Ok(PreparedModelCall::new(async move {
                calls.fetch_add(1, Ordering::SeqCst);
                Ok(ModelStepOutput {
                    attempt_id: request.attempt_id,
                    base_context_revision: request.context.revision,
                    content: vec![ContextContent::Text {
                        text: Arc::from("saved reply"),
                    }],
                    tool_calls: Vec::new(),
                    private_context: None,
                    usage: pl_core::model::ModelUsage {
                        input_tokens: Some(11),
                        output_tokens: Some(7),
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
    async fn failed_directory_projection_retries_without_model_replay_and_closes_after_billing_save()
     {
        let store = StudioStore::open_memory().await.unwrap();
        let writer = crate::studio::agent_host::ThreadWriteBehindWriter::new(store.clone());
        let events = ProductEventBus::new(store.clone(), writer.clone());
        let performance = ModelPerformanceOwner::new(store.clone(), events.clone());
        let observers = ThreadObservations::new(ObservationServices {
            store: store.clone(),
            events: events.clone(),
            performance: performance.clone(),
            threads: crate::thread_assembler::StudioThreadAssembler::default(),
            writer: writer.clone(),
        });
        let workspace = tempfile::tempdir().unwrap();
        let project = store.upsert_project(workspace.path()).await.unwrap();
        let (_, product) = crate::studio::store::directory::DirectoryDelta::register_root_thread(
            crate::studio::ids::new_id("thread"),
            &project.id,
            "task",
            pl_protocol::ThreadModeId::simple(),
            pl_protocol::ThreadWorkspaceMode::Local,
            project.path.clone(),
        );
        let calls = Arc::new(AtomicUsize::new(0));
        let thread = ThreadHandle::start(
            product.id.clone(),
            DynModelSession::new(Reply(calls.clone())),
        )
        .unwrap();
        observers.observe(product.id.clone(), thread.clone());
        let observation = observers
            .0
            .observations
            .lock()
            .unwrap()
            .get(&product.id)
            .unwrap()[0]
            .clone();
        let mut progress = observation.state.progress.subscribe();
        while progress.borrow().error.is_none() {
            progress.changed().await.unwrap();
        }
        assert_eq!(progress.borrow().initialized, false);
        events
            .apply_thread_delta(vec![product.clone()], Vec::new())
            .await
            .unwrap();
        observers.synchronize(Some(&product.id)).await.unwrap();
        assert_eq!(calls.load(Ordering::SeqCst), 0);
        thread
            .step(pl_core::thread::StepInput {
                turn_id: "turn".into(),
                attempt_id: "attempt".into(),
                content: vec![ContextContent::Text {
                    text: Arc::from("question"),
                }],
                cancellation: tokio_util::sync::CancellationToken::new(),
            })
            .await
            .unwrap();
        observers.synchronize(Some(&product.id)).await.unwrap();
        let billed = performance.snapshot().await;
        assert_eq!(billed.revision, 1);
        observers.synchronize(Some(&product.id)).await.unwrap();
        assert_eq!(performance.snapshot().await, billed);
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        thread.close().await.unwrap();
        observers.finish().await.unwrap();
        assert_eq!(
            events.thread_snapshot(&product.id).unwrap().status,
            pl_protocol::ThreadStatus::Closed
        );
        assert_eq!(writer.pending_commit_count(), 0);
        let reloaded = ModelPerformanceOwner::new(store.clone(), events);
        reloaded.load_cache().await.unwrap();
        assert_eq!(reloaded.snapshot().await, billed);
        writer.shutdown().await.unwrap();
    }
}
