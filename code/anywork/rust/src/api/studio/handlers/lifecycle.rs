use crate::api::studio::bridge_runtime::{
    BridgeRuntime, active_bridge, arm_exit_latch, fix_exit_deadline, install_bridge_runtime,
    installed_bridge, late_install_refused, lifecycle_gate, remaining_until,
};
use crate::api::studio::convert::runtime::{
    bridge_shutdown_report, runtime_shutdown_issue, runtime_snapshot,
};
use crate::api::studio::types::{
    BridgeError, BridgePendingPersistence, BridgeShutdownIssue, BridgeShutdownOutcome,
    BridgeShutdownReport, ProjectDto, RuntimeSnapshot,
};
use anyhow::Context;
use flutter_rust_bridge::frb;
use pl_studio_runtime::StudioRuntimeStateKind;
use std::time::{Duration, Instant};

use super::updater::{
    broadcast_cancel_update_operations, broadcast_cancel_update_operations_excluding,
    observe_update_operations,
};
// ── Runtime lifecycle ──

/// Budget for cancelling bridge-owned subscriptions inside the update install's strict shutdown.
const UPDATE_SHUTDOWN_BUDGET: Duration = Duration::from_secs(5);

/// native 首次 close 的早期桥端广播（订阅关闭帧与更新操作取消）观测到的真实失败。
///
/// 由随后同一 `shutdown_runtime` collector 播种为成功前置条件：不吞、不伪成功，也不引入第二套
/// 结论。std `Mutex`，仅在无 await 的短临界区内持有。
static BRIDGE_EARLY_ISSUES: std::sync::OnceLock<std::sync::Mutex<Vec<BridgeShutdownIssue>>> =
    std::sync::OnceLock::new();

fn bridge_early_issues_slot() -> &'static std::sync::Mutex<Vec<BridgeShutdownIssue>> {
    BRIDGE_EARLY_ISSUES.get_or_init(|| std::sync::Mutex::new(Vec::new()))
}

fn record_bridge_early_issues(issues: Vec<BridgeShutdownIssue>) {
    if issues.is_empty() {
        return;
    }
    let mut slot = match bridge_early_issues_slot().lock() {
        Ok(slot) => slot,
        Err(poisoned) => poisoned.into_inner(),
    };
    slot.extend(issues);
}

fn take_bridge_early_issues() -> Vec<BridgeShutdownIssue> {
    let mut slot = match bridge_early_issues_slot().lock() {
        Ok(slot) => slot,
        Err(poisoned) => poisoned.into_inner(),
    };
    std::mem::take(&mut *slot)
}

#[allow(unexpected_cfgs)]
#[frb(init)]
pub fn init_app() {
    crate::diagnostics::initialize();
}

/// 发放一次新的启动尝试令牌；可在 bridge 安装前（pre-installed）调用。
///
/// 同一时刻只有一个活跃令牌：start 在途期间明确拒绝；已 Ready（runtime 运行中）同样
/// 拒绝——重启前必须先 shutdown。新令牌发放后旧令牌立即过期。shutdown 开始后本进程
/// 不再支持新的启动尝试：bridge 的 OnceCell 与 shutdown 令牌都是一次性资源。
pub async fn prepare_startup_attempt() -> Result<u64, BridgeError> {
    STARTUP.prepare()
}

/// 以指定令牌启动 Studio runtime。
///
/// `attempt` 必须是 [`prepare_startup_attempt`] 发放且未过期的令牌；在途期间的重复
/// start、旧令牌与失败/停机后的复用都被明确拒绝。已 Ready 的同令牌调用幂等成功：
/// 返回前核对真实 runtime 仍处于 Ready 且 shutdown 未开始，否则以 `runtime_stopped`
/// 明确失败而不是虚报快照；不重开 runtime、不虚发 `OpeningStorage`，进度流保持
/// `Ready` 终态。start 在途的失败与任务取消都把本尝试落为 `Failed` 终态，不会永久
/// 卡在 in-progress。
pub async fn start_studio_runtime(attempt: u64) -> Result<RuntimeSnapshot, BridgeError> {
    match STARTUP.begin_start(attempt)? {
        StartClaim::AlreadyReady => {
            let bridge = installed_bridge()?;
            // 幂等窗口必须锚定真实 runtime：shutdown 在途/完成后不得返回假成功。
            if bridge.shutdown.is_cancelled() {
                return Err(BridgeError::runtime_stopped());
            }
            let snapshot = bridge.studio.runtime_snapshot().await?;
            if snapshot.state.kind() != StudioRuntimeStateKind::Ready
                || bridge.shutdown.is_cancelled()
            {
                return Err(BridgeError::runtime_stopped());
            }
            Ok(runtime_snapshot(snapshot))
        }
        StartClaim::Execute => {
            let mut demote = StartupDemote::new(attempt);
            let bridge = match install_bridge_runtime().await {
                Ok(bridge) => bridge,
                // 安装失败也是启动失败：守卫降级把进度流推到 Failed 终态。
                Err(error) => return Err(error.into()),
            };
            // main 的唯一初始化入口已在安装时返回 Ready runtime：这里只读取 canonical 快照，
            // 不再调用第二套 start。
            let snapshot = bridge.studio.runtime_snapshot().await?;
            if bridge.shutdown.is_cancelled() || !STARTUP.attempt_active(attempt) {
                return Err(BridgeError::runtime_stopped());
            }
            demote.disarm();
            STARTUP.finish_start(attempt);
            Ok(runtime_snapshot(snapshot))
        }
    }
}

/// Early application-exit entry: seal the one-way exit latch and fence the installed runtime
/// **before** the caller performs its own bounded cancellations.
///
/// The native host owns the single 30s deadline. When the first close arrives it must call this
/// entry first so that Rust stops admitting new work immediately, instead of waiting until the
/// Dart-side product/Thread/progress cancellations have all settled before the first
/// `shutdown_runtime` call. This entry:
///
/// - arms the bridge exit latch synchronously (never waiting on the lifecycle gate), so a late
///   `start_studio_runtime` can never publish a runtime once exit has begun;
/// - fixes the single first deadline from the caller's remaining cleanup budget, so a later
///   `shutdown_runtime` never extends it;
/// - invalidates the startup attempt token synchronously (before any await and independent of an
///   installed runtime) and wakes a pre-active startup observer, so a concurrent attempt can never
///   publish a runtime once exit has begun;
/// - when a runtime is already installed, synchronously fences domain admission and broadcasts
///   cancellation to the runtime's independent owners.
///
/// It never triggers initialization (an uninstalled runtime stays `NotStarted` for the final
/// report), never runs the real service stops/joins, never publishes `Stopped` and never releases
/// the instance lock. Those remain the job of the single `shutdown_runtime` orchestration, which
/// folds the same bridge/external issues into the same collector. Failures observed here are kept
/// on the runtime's early-exit slot and logged; they are neither swallowed nor faked as success.
///
/// `remaining_ms` is the caller's remaining cleanup budget (native's single deadline minus the
/// time already spent), not a fresh window.
pub async fn begin_runtime_exit(remaining_ms: u32) -> Result<(), BridgeError> {
    // Seal the latch and observe the published runtime atomically with the close decision, so a
    // runtime is never published after closing and an already-published runtime is always observed.
    let bridge = arm_exit_latch();
    // Fix the single first deadline; a later `shutdown_runtime` can only reuse or shorten it.
    let _ = fix_exit_deadline(remaining_ms);
    // Exit has begun: invalidate the startup token and wake any not-yet-settled pre-active startup
    // observer synchronously, before the first await below and independent of whether a runtime is
    // installed, so a later `start_studio_runtime` attempt can never publish.
    STARTUP.begin_shutdown();
    if let Some(bridge) = bridge {
        // Broadcast native subscription cancellation early (a synchronous token cancel with no
        // await/join): the content streams wind down so Dart's later cancel ACK completes faster.
        // The shutdown-progress stream uses its own token and is deliberately left running until
        // its explicit cancel in `shutdown_runtime`.
        bridge.shutdown.cancel();
        // Broadcast update cancellation without joining; real errors are retained for the same
        // collector and never block the remaining broadcasts.
        record_bridge_early_issues(broadcast_cancel_update_operations().await);
        // Fence admission + broadcast independent cancels. Bounded internally; issues are retained
        // on the runtime and seeded by the same collector in `shutdown_runtime`.
        let _ = bridge.studio.fence_exit_admission().await;
    }
    Ok(())
}

/// Application-exit shutdown with a caller-supplied cleanup budget in milliseconds.
///
/// The desktop host owns the single 30s deadline and passes its remaining cleanup time; this
/// entry caps it at 28s, never extends the deadline, and returns a typed report even when the
/// run is `Degraded` or times out.
///
/// The one-way exit latch is closed synchronously before any await, so a late install can never
/// publish a runtime even if initialization is hanging and the lifecycle gate cannot be taken.
/// Every exit step (latch, gate observation, subscription/update cancellation, runtime stages and
/// diagnostics) shares this single first deadline; repeated close never extends it.
///
/// `external_issues` carries the cancellations the caller (native host / Dart) already performed
/// and observed before this call — bridge- and Dart-owned subscriptions (including the shutdown
/// progress stream) and update operations whose owner did not confirm. They are fed into the
/// **same** runtime shutdown orchestration as success preconditions, so the runtime itself refuses
/// `Clean`, never publishes `Stopped` and never releases the instance lock when any owner is
/// unresolved. There is no post-hoc report merge and no second cleanup path.
pub async fn shutdown_runtime(
    remaining_ms: u32,
    external_issues: Vec<BridgeShutdownIssue>,
) -> Result<BridgeShutdownReport, BridgeError> {
    // Close the exit latch synchronously first (and observe the published runtime atomically with
    // that decision): never wait on the gate before refusing late installs, and never miss a
    // runtime that a concurrent install published first.
    let bridge = arm_exit_latch();
    let deadline = fix_exit_deadline(remaining_ms);
    // Invalidate the startup token and wake a pre-active observer before the first await (the
    // bounded lifecycle-gate lock below): once exit has begun no attempt may still publish.
    STARTUP.begin_shutdown();
    let started = Instant::now();

    // Bounded observation of the lifecycle gate: if initialization is still in flight we give up
    // waiting, but never report `NotStarted` for resources it already created.
    let gate_acquired = tokio::time::timeout(remaining_until(deadline), lifecycle_gate().lock())
        .await
        .is_ok();

    let mut bridge_issues: Vec<BridgeShutdownIssue> = Vec::new();
    let mut owner_id: Option<String> = None;
    let mut save_watermark: Option<u64> = None;
    let external_issues_present = !external_issues.is_empty();
    let report = match bridge {
        Some(bridge) => {
            // Capture real persistence facts before shutdown so the emergency diagnostic is not a
            // blanket `None`; the frozen report shape itself carries no such fields.
            let facts = bridge.studio.persistence_diagnostics();
            owner_id = facts.0;
            save_watermark = facts.1;
            // Signal every independent owner before any bounded join, so a slow join of one owner
            // can never consume the shared first deadline before another owner has been cancelled.
            // Their errors become runtime shutdown preconditions below.
            bridge.shutdown.cancel();
            let pending_subscriptions = bridge.subscriptions.broadcast_cancel(deadline).await;
            let pending_updates = broadcast_cancel_update_operations_excluding(None, true).await;
            // Two independent owner groups (bridge-owned subscriptions and update operations) are
            // observed **concurrently** under the same first deadline: a slow join of one group must
            // not consume the budget before the other group has been observed. Broadcast already
            // happened above, so this is join-only.
            let (subscription_issues, update_issues) = futures::join!(
                bridge
                    .subscriptions
                    .join_cancelled(pending_subscriptions, deadline),
                observe_update_operations(pending_updates, deadline),
            );
            bridge_issues.extend(subscription_issues);
            bridge_issues.extend(update_issues);
            // Fold the caller-observed external failures, the early-exit bridge broadcast failures
            // and the bridge-owned ones into the runtime orchestration itself. The runtime finalizer
            // treats any of them as a failed success precondition, so `Clean` (and therefore
            // `Stopped` + instance-lock release) is only reachable when both the bridge and the
            // runtime settled every owner.
            let mut issues = external_issues;
            issues.extend(take_bridge_early_issues());
            issues.append(&mut bridge_issues);
            let remaining = remaining_ms_u32(deadline);
            bridge_shutdown_report(
                bridge
                    .studio
                    .shutdown_runtime_for_exit(
                        remaining,
                        issues.into_iter().map(runtime_shutdown_issue).collect(),
                    )
                    .await,
            )
        }
        None if gate_acquired && !late_install_refused() && !external_issues_present => {
            BridgeShutdownReport::not_started()
        }
        None => {
            // Either initialization is still in flight (gate not acquired) or it started and was
            // refused after exit began, or the caller already observed an unresolved external
            // owner. None of these may masquerade as `NotStarted`.
            let mut issues = external_issues;
            issues.extend(take_bridge_early_issues());
            issues.push(bridge_issue(
                "installation",
                "deadlineExceeded",
                "Studio runtime initialization did not commit before the exit deadline; created resources are being reclaimed",
                true,
            ));
            BridgeShutdownReport {
                outcome: BridgeShutdownOutcome::Degraded,
                issues,
                persistence: BridgePendingPersistence::Unknown,
            }
        }
    };
    record_shutdown_report(
        &report,
        started.elapsed(),
        owner_id.as_deref(),
        save_watermark,
    );
    Ok(report)
}

fn remaining_ms_u32(deadline: Instant) -> u32 {
    remaining_until(deadline).as_millis().min(u32::MAX as u128) as u32
}

fn bridge_issue(stage: &str, code: &str, message: &str, retryable: bool) -> BridgeShutdownIssue {
    BridgeShutdownIssue {
        stage: stage.to_string(),
        code: code.to_string(),
        message: message.to_string(),
        retryable,
        correlation_id: pl_protocol::studio::StudioError::internal().correlation_id,
    }
}

/// Finalizes diagnostics independent of runtime initialization.
///
/// Idempotent and safe without an installed runtime; the GUI calls it before a safe dispose or
/// before exiting so diagnostics are flushed even when startup never completed. The synchronous
/// flush (which joins the non-blocking log worker) runs on the blocking pool so it cannot hold a
/// Tokio worker or the cleanup deadline while runtime owners are still being polled.
pub async fn finish_shutdown_diagnostics() -> Result<(), BridgeError> {
    tokio::task::spawn_blocking(crate::diagnostics::shutdown)
        .await
        .map_err(|error| anyhow::anyhow!("diagnostics shutdown task failed: {error}"))?;
    Ok(())
}

pub(super) async fn shutdown_runtime_for_update(
    bridge: &'static BridgeRuntime,
    handoff_operation_id: u64,
) -> Result<bool, BridgeError> {
    // The idle check stays atomic with the transition away from `Ready` inside the runtime. The
    // bridge-owned subscription cancellation runs as an adapter hook **inside the same strict
    // orchestration**: it is invoked only after the idle check commits and before finalization, so
    // a busy runtime is never cancelled and its ACK is a precondition of the same `Clean`
    // decision — a subscription that does not stop keeps the runtime `Failed`, never publishing
    // `Stopped` or releasing the instance lock. No post-`Stopped` merge, no second cleanup path.
    //
    // Any *other* live update operation is an independent external owner of the same shutdown and
    // must be signalled here and acknowledged within the shared deadline, before the runtime may
    // report `Clean`. Every owner — the bridge-owned subscriptions and the other live update
    // operations — is signalled before any bounded join, so a slow join of one owner can never
    // consume the shared budget before another owner has been cancelled. The operation that owns
    // this handoff (`handoff_operation_id`) is excluded: it is the caller, currently holding the
    // install command lock and the install-active flag, so cancelling it would self-cancel the
    // handoff and awaiting it would wait on this very hook. No competing install blocks on those
    // locks (both acquisitions are non-blocking), so the bounded observation cannot deadlock.
    let deadline = Instant::now() + UPDATE_SHUTDOWN_BUDGET;
    let hook: pl_studio_runtime::ShutdownExternalHook = Box::new(move || {
        Box::pin(async move {
            // The runtime has committed the strict shutdown when this hook runs: invalidate the
            // startup token synchronously as the first step, before any await / cancel / join of the
            // hook, so a not-yet-settled pre-active startup observer is woken immediately. The idle
            // check itself never invalidates the token: a busy runtime that refuses to shut down
            // keeps its current attempt.
            STARTUP.begin_shutdown();
            bridge.shutdown.cancel();
            // Phase 1: signal every independent owner (no joins).
            let pending_subscriptions = bridge.subscriptions.broadcast_cancel(deadline).await;
            let pending_updates =
                broadcast_cancel_update_operations_excluding(Some(handoff_operation_id), false)
                    .await;
            // Phase 2: bounded joins under the same first deadline. The two independent owner
            // groups are observed concurrently, so a slow join of one never consumes the shared
            // budget before the other has been observed.
            let (subscription_issues, update_issues) = futures::join!(
                bridge
                    .subscriptions
                    .join_cancelled(pending_subscriptions, deadline),
                observe_update_operations(pending_updates, deadline),
            );
            let mut issues: Vec<pl_studio_runtime::StudioShutdownIssue> = subscription_issues
                .into_iter()
                .map(runtime_shutdown_issue)
                .collect();
            issues.extend(update_issues.into_iter().map(runtime_shutdown_issue));
            issues
        })
    });
    match bridge.studio.shutdown_runtime_if_idle(Some(hook)).await {
        Ok(Some(_)) => {
            tracing::info!("Studio runtime shutdown completed for update");
            Ok(true)
        }
        // 空闲检查未通过：没有发生 shutdown，不得凭猜测失效当前令牌。
        Ok(None) => Ok(false),
        Err(error) => Err(error.into()),
    }
}

fn record_shutdown_report(
    report: &BridgeShutdownReport,
    elapsed: std::time::Duration,
    owner_id: Option<&str>,
    save_watermark: Option<u64>,
) {
    let pending = match &report.persistence {
        BridgePendingPersistence::Unknown => "unknown".to_string(),
        BridgePendingPersistence::Pending { count } => format!("pending:{count}"),
        BridgePendingPersistence::Drained => "drained".to_string(),
    };
    if !report.is_clean() && report.issues.is_empty() {
        crate::diagnostics::record_shutdown_diagnostic(crate::diagnostics::ShutdownDiagnostic {
            stage: "shutdown",
            code: "degraded",
            message: "Studio runtime shutdown did not reach a clean state",
            retryable: true,
            correlation_id: "none",
            pending: &pending,
            owner_id,
            save_watermark,
            elapsed_ms: elapsed.as_millis(),
        });
    }
    for issue in &report.issues {
        crate::diagnostics::record_shutdown_diagnostic(crate::diagnostics::ShutdownDiagnostic {
            stage: &issue.stage,
            code: &issue.code,
            message: &issue.message,
            retryable: issue.retryable,
            correlation_id: &issue.correlation_id,
            pending: &pending,
            owner_id,
            save_watermark,
            elapsed_ms: elapsed.as_millis(),
        });
    }
}

// ── Studio commands ──

pub async fn open_project(path: String) -> Result<ProjectDto, BridgeError> {
    let bridge = active_bridge().await?;
    let project = bridge.studio.open_project(path).await?;
    Ok(project.into())
}

pub async fn activate_project(project_id: String) -> Result<(), BridgeError> {
    let bridge = active_bridge().await?;
    bridge.studio.activate_project(&project_id).await?;
    Ok(())
}

pub async fn archive_project(project_id: String) -> Result<Option<ProjectDto>, BridgeError> {
    let bridge = active_bridge().await?;
    let archived = bridge
        .studio
        .archive_project(&project_id)
        .await?
        .context("selected project not found")?;
    Ok(Some(archived.into()))
}

/// Changes a Project display name, preserving its path and connection.
pub async fn rename_project(project_id: String, name: String) -> Result<ProjectDto, BridgeError> {
    let bridge = super::super::bridge_runtime::active_bridge().await?;
    Ok(bridge
        .studio
        .rename_project(&project_id, &name)
        .await?
        .into())
}

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, MutexGuard};

/// 启动进度帧：阶段只归属其 attempt；旧订阅者据此终止而不是接收新一代阶段。
pub(crate) struct StartupFrame {
    pub(crate) attempt: u64,
    pub(crate) stage: super::super::types::BridgeStartupStage,
}

/// 单一启动 owner：同一时刻至多一个活跃尝试令牌。
struct StartupOwner {
    sender: tokio::sync::watch::Sender<StartupFrame>,
    phase: Mutex<StartupPhase>,
    next_attempt: AtomicU64,
}

/// 启动尝试令牌的 owner 状态机；prepare 与 start 的竞态都在这里串行裁决。
enum StartupPhase {
    /// 进程尚未发放任何令牌。
    Initial,
    /// 令牌已发放、start 未执行；新 prepare 取代旧令牌，旧令牌立即过期。
    Prepared { attempt: u64 },
    /// start 在途；prepare 与重复 start 都被拒绝。
    Starting { attempt: u64 },
    /// 本次尝试已 Ready；同令牌 start 幂等成功（返回前核对真实 runtime）。
    Ready { attempt: u64 },
    /// 本次尝试失败；重新启动需要新的 prepare。
    Failed { attempt: u64 },
    /// shutdown 已开始（无论后续成败）；进程不支持同进程新 startup，旧令牌全部失效。
    Stopped { attempt: u64 },
}

/// start 占用结果：执行启动，或同令牌已 Ready 的幂等窗口。
enum StartClaim {
    Execute,
    AlreadyReady,
}

/// start 在途守卫：任务取消（Drop）时把在途尝试落为 Failed，不永久卡 in-progress。
struct StartupDemote(Option<u64>);

impl StartupDemote {
    fn new(attempt: u64) -> Self {
        Self(Some(attempt))
    }

    fn disarm(&mut self) {
        self.0 = None;
    }
}

impl Drop for StartupDemote {
    fn drop(&mut self) {
        if let Some(attempt) = self.0.take() {
            STARTUP.fail_start(attempt);
        }
    }
}

static STARTUP: std::sync::LazyLock<StartupOwner> = std::sync::LazyLock::new(StartupOwner::new);

impl StartupOwner {
    fn new() -> Self {
        let (sender, _) = tokio::sync::watch::channel(StartupFrame {
            attempt: 0,
            stage: super::super::types::BridgeStartupStage::OpeningStorage,
        });
        Self {
            sender,
            phase: Mutex::new(StartupPhase::Initial),
            next_attempt: AtomicU64::new(1),
        }
    }

    fn lock(&self) -> MutexGuard<'_, StartupPhase> {
        self.phase.lock().unwrap_or_else(|error| error.into_inner())
    }

    fn prepare(&self) -> Result<u64, BridgeError> {
        let mut phase = self.lock();
        match *phase {
            StartupPhase::Starting { .. } => Err(BridgeError::invalid_argument(
                "a startup attempt is already in progress; wait for Ready or Failed first",
            )),
            StartupPhase::Ready { .. } => Err(BridgeError::invalid_argument(
                "Studio runtime is already running; shutdown before preparing a new attempt",
            )),
            // shutdown 令牌与 bridge OnceCell 都是一次性进程资源：真实重开不可达，
            // 不得对 Stopped 发新令牌或虚报 OpeningStorage。
            StartupPhase::Stopped { .. } => Err(BridgeError::invalid_argument(
                "Studio runtime has been shut down; same-process restart is not supported",
            )),
            StartupPhase::Initial | StartupPhase::Prepared { .. } | StartupPhase::Failed { .. } => {
                let attempt = self.next_attempt.fetch_add(1, Ordering::Relaxed);
                *phase = StartupPhase::Prepared { attempt };
                // 新尝试的站立阶段：订阅首帧是本尝试的真实阶段，绝不会先拿到旧终态。
                self.sender.send_replace(StartupFrame {
                    attempt,
                    stage: super::super::types::BridgeStartupStage::OpeningStorage,
                });
                Ok(attempt)
            }
        }
    }

    fn subscribe(
        &self,
        attempt: u64,
    ) -> Result<tokio::sync::watch::Receiver<StartupFrame>, BridgeError> {
        let phase = self.lock();
        let current = match *phase {
            StartupPhase::Initial => {
                return Err(BridgeError::invalid_argument(
                    "prepare a startup attempt before subscribing to startup progress",
                ));
            }
            StartupPhase::Prepared { attempt: current }
            | StartupPhase::Starting { attempt: current }
            | StartupPhase::Ready { attempt: current } => current,
            StartupPhase::Failed { attempt: current } => {
                if attempt == current {
                    return Err(BridgeError::invalid_argument(
                        "startup attempt already finished; prepare a new attempt to retry",
                    ));
                }
                return Err(BridgeError::invalid_argument(
                    "unknown or expired startup attempt",
                ));
            }
            StartupPhase::Stopped { attempt: current } => {
                if attempt == current {
                    return Err(BridgeError::invalid_argument(
                        "startup attempt stopped with the runtime shutdown; same-process restart is not supported",
                    ));
                }
                return Err(BridgeError::invalid_argument(
                    "unknown or expired startup attempt",
                ));
            }
        };
        if attempt != current {
            return Err(BridgeError::invalid_argument(
                "unknown or expired startup attempt",
            ));
        }
        Ok(self.sender.subscribe())
    }

    fn begin_start(&self, attempt: u64) -> Result<StartClaim, BridgeError> {
        let mut phase = self.lock();
        match *phase {
            StartupPhase::Prepared { attempt: current } if current == attempt => {
                *phase = StartupPhase::Starting { attempt };
                Ok(StartClaim::Execute)
            }
            StartupPhase::Ready { attempt: current } if current == attempt => {
                Ok(StartClaim::AlreadyReady)
            }
            StartupPhase::Starting { .. } => Err(BridgeError::invalid_argument(
                "a startup attempt is already in progress",
            )),
            StartupPhase::Initial => Err(BridgeError::invalid_argument(
                "prepare a startup attempt before starting the runtime",
            )),
            StartupPhase::Stopped { attempt: current } => {
                if attempt == current {
                    // shutdown 已开始：明确 runtime_stopped，不落回过期令牌话术。
                    return Err(BridgeError::runtime_stopped());
                }
                Err(BridgeError::invalid_argument(
                    "unknown or expired startup attempt",
                ))
            }
            StartupPhase::Prepared { .. }
            | StartupPhase::Ready { .. }
            | StartupPhase::Failed { .. } => Err(BridgeError::invalid_argument(
                "unknown or expired startup attempt",
            )),
        }
    }

    /// start 成功：本尝试进入 Ready 并发布终态帧（runtime 观察者可能已发布过 Ready）。
    fn finish_start(&self, attempt: u64) {
        let mut phase = self.lock();
        if matches!(*phase, StartupPhase::Starting { attempt: current } if current == attempt) {
            *phase = StartupPhase::Ready { attempt };
            self.sender.send_replace(StartupFrame {
                attempt,
                stage: super::super::types::BridgeStartupStage::Ready,
            });
        }
    }

    /// start 失败或被取消：本尝试落 Failed 终态，重新启动需要新的 prepare。
    fn fail_start(&self, attempt: u64) {
        let mut phase = self.lock();
        if matches!(*phase, StartupPhase::Starting { attempt: current } if current == attempt) {
            *phase = StartupPhase::Failed { attempt };
            self.sender.send_replace(StartupFrame {
                attempt,
                stage: super::super::types::BridgeStartupStage::Failed,
            });
        }
    }

    /// shutdown 开始即失效当前令牌，并以 Failed 终态唤醒 pre-active 观察者。
    /// 进度流交付 Ready 前仍以 [`StartupOwner::attempt_active`] 拒绝已失效尝试。
    fn begin_shutdown(&self) {
        let mut phase = self.lock();
        match *phase {
            StartupPhase::Initial => {}
            StartupPhase::Prepared { attempt }
            | StartupPhase::Starting { attempt }
            | StartupPhase::Ready { attempt }
            | StartupPhase::Failed { attempt }
            | StartupPhase::Stopped { attempt } => {
                *phase = StartupPhase::Stopped { attempt };
                // 唤醒尚未读到终态的 pre-active 观察者；它们不依赖 runtime 订阅注册表取消。
                self.sender.send_replace(StartupFrame {
                    attempt,
                    stage: super::super::types::BridgeStartupStage::Failed,
                });
            }
        }
    }

    /// 进度流交付 `Ready` 终态前的令牌有效性检查：shutdown 已把尝试落为 Stopped 后，
    /// 未读的终态不得作为新成功交付。
    fn attempt_active(&self, attempt: u64) -> bool {
        let phase = self.lock();
        matches!(
            *phase,
            StartupPhase::Starting { attempt: current }
            | StartupPhase::Ready { attempt: current }
            if current == attempt
        )
    }

    /// runtime 阶段回调只归属当前在途尝试；其他世代来源的阶段不发布。
    fn publish_stage(&self, stage: pl_studio_runtime::StudioStartupStage) {
        let phase = self.lock();
        if let StartupPhase::Starting { attempt } = *phase {
            self.sender.send_replace(StartupFrame {
                attempt,
                stage: bridge_startup_stage(stage),
            });
        }
    }
}

/// runtime 的阶段回调入口；由 bridge 安装时注入 runtime 构造。
pub(crate) fn publish_startup_stage(stage: pl_studio_runtime::StudioStartupStage) {
    STARTUP.publish_stage(stage);
}

/// 启动进度 watch 的订阅入口；`RustLib.init` 后、runtime 安装前即可使用。
pub(crate) fn subscribe_startup_attempt(
    attempt: u64,
) -> Result<tokio::sync::watch::Receiver<StartupFrame>, BridgeError> {
    STARTUP.subscribe(attempt)
}

/// startup 进度流在交付 `Ready` 终态前核对令牌仍有效（shutdown 开始即失效）。
pub(crate) fn startup_attempt_active(attempt: u64) -> bool {
    STARTUP.attempt_active(attempt)
}

fn bridge_startup_stage(
    stage: pl_studio_runtime::StudioStartupStage,
) -> super::super::types::BridgeStartupStage {
    use super::super::types::BridgeStartupStage as Target;
    use pl_studio_runtime::StudioStartupStage as Source;
    match stage {
        Source::Preparing => Target::Preparing,
        Source::WaitingForSteps => Target::WaitingForSteps,
        Source::ClosingResources => Target::ClosingResources,
        Source::BackingUp => Target::BackingUp,
        Source::Resetting => Target::Resetting,
        Source::StartingServices => Target::StartingServices,
        Source::OpeningStorage => Target::OpeningStorage,
        Source::LoadingConfiguration => Target::LoadingConfiguration,
        Source::ReadingProjects => Target::ReadingProjects,
        Source::PreparingResources => Target::PreparingResources,
        Source::Ready => Target::Ready,
        Source::Failed => Target::Failed,
    }
}

pub async fn read_recovery_state()
-> Result<super::super::types::BridgeRecoveryStateSnapshot, BridgeError> {
    let bridge = active_bridge().await?;
    Ok(super::super::convert::runtime::bridge_recovery_state(
        bridge.studio.read_recovery_state().state,
    ))
}

pub async fn retry_recovery()
-> Result<super::super::types::BridgeRecoveryStateSnapshot, BridgeError> {
    let bridge = active_bridge().await?;
    Ok(super::super::convert::runtime::bridge_recovery_state(
        bridge.studio.retry_recovery().await?.state,
    ))
}
