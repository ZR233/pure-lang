//! 统一关机阶段编排与退出期限。
//!
//! 严格关闭（进程内 server / 更新前的空闲关闭）与应用退出共用同一套阶段编排，只按
//! `ShutdownBudget` 与调用契约区分：严格路径要求真实 `Clean`，退出路径接受 `Degraded`
//! 并按剩余期限终止。任何阶段失败都不阻断其余彼此独立的安全清理。
//!
//! 本次退出只在首个调用者处固定一个绝对期限；退出编排运行在真正 owned 的 spawned task 上，
//! 即使等待方超时或全部离开也会被运行时继续 poll 到结束。每个阶段先广播取消（或 abort），
//! 再做有界 join；等待超时只放弃等待，从不 drop 在途的存储 / writer / JoinHandle。

use std::future::Future;
use std::panic::AssertUnwindSafe;
use std::sync::Arc;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use anyhow::Result;
use futures::FutureExt;
use futures::future::BoxFuture;
use futures::stream::{FuturesUnordered, StreamExt};
use tokio::sync::watch;
use tokio::task::JoinHandle;

use crate::studio::{
    StudioRuntime, StudioRuntimeCommand, StudioRuntimeSnapshot, StudioRuntimeStateKind,
    unix_seconds,
};
use crate::{
    StudioPendingPersistence, StudioShutdownIssue, StudioShutdownOutcome, StudioShutdownReport,
};

/// 应用退出的清理预算上限；Dart 传入的 native 剩余时间超过它时按它裁剪。
pub(crate) const EXIT_CLEANUP_BUDGET: Duration = Duration::from_secs(28);

/// 单个外部服务阶段的上限；多个完整超时不得简单相加超出总预算。
const SERVICE_STAGE_LIMIT: Duration = Duration::from_secs(5);

/// 只读事实观测（runtime 快照、持久化计数）的上限：绝不允许在阶段之外无界等待。
const FACTS_LIMIT: Duration = Duration::from_secs(2);

/// 单个诊断日志里最多列出的未收束 Thread 标识数量，避免超长 Thread 列表挤占日志。
const UNRESOLVED_THREAD_LOG_LIMIT: usize = 8;

/// 单向关闭终止闩：一旦进入关闭，本进程余下生命周期内拒绝迟到安装。
#[derive(Clone, Default)]
pub(crate) struct ShutdownLatch {
    committed: Arc<AtomicBool>,
}

impl ShutdownLatch {
    pub(crate) fn arm(&self) {
        self.committed.store(true, Ordering::SeqCst);
    }

    pub(crate) fn is_committed(&self) -> bool {
        self.committed.load(Ordering::SeqCst)
    }
}

/// Adapter hook invoked inside the single strict orchestration to collect external
/// (bridge / Dart) owner acknowledgements.
///
/// It runs only after the runtime has committed to shutdown — the atomic idle check passed and
/// mutation is fenced — and before finalization. The returned issues are folded into the same
/// finalize success precondition, so a subscription/update owner that does not confirm keeps the
/// runtime `Failed`: the strict path never publishes `Stopped` and never releases the instance
/// lock. Keeping the hook inside the orchestration is what makes the idle check atomic with the
/// cancellation (no cancel-before-check and no post-`Stopped` merge).
pub type ShutdownExternalHook =
    Box<dyn FnOnce() -> futures::future::BoxFuture<'static, Vec<StudioShutdownIssue>> + Send>;

/// 同一次退出关闭运行的共享完成句柄。
///
/// `report` 是一个共享的 `watch` 完成状态，不引用 runtime，因此不会与持有它的
/// `StudioRuntime` 形成 `Arc` 环。编排本身运行在 owned spawned task 上：重复退出只 join
/// 同一个任务、不重复执行阶段，也不因新的调用延长已经固定的期限；等待方超时返回后任务
/// 仍被运行时继续 poll 到结束。
#[derive(Clone)]
pub(crate) struct ShutdownRun {
    report: Arc<watch::Sender<Option<StudioShutdownReport>>>,
    started: Arc<AtomicBool>,
    deadline: Arc<OnceLock<Instant>>,
}

impl Default for ShutdownRun {
    fn default() -> Self {
        let (report, _receiver) = watch::channel::<Option<StudioShutdownReport>>(None);
        Self {
            report: Arc::new(report),
            started: Arc::new(AtomicBool::new(false)),
            deadline: Arc::new(OnceLock::new()),
        }
    }
}

impl ShutdownRun {
    /// 首次退出固定绝对期限；重复退出只复用，不延长。
    fn fixed_deadline(&self, requested: Duration) -> Instant {
        *self
            .deadline
            .get_or_init(|| Instant::now() + requested.min(EXIT_CLEANUP_BUDGET))
    }

    /// 启动唯一一次 owned 关闭编排任务；已启动则直接返回。
    ///
    /// 首个调用者的 `external_issues` 与预算一起被固定进这次唯一编排；后续重复退出只 join
    /// 同一任务，不重复执行阶段，也不延长已经固定的期限。
    fn spawn_once(
        &self,
        runtime: &StudioRuntime,
        budget: ShutdownBudget,
        external_issues: Vec<StudioShutdownIssue>,
    ) {
        if self.started.swap(true, Ordering::SeqCst) {
            return;
        }
        let runtime = runtime.clone();
        let report = Arc::clone(&self.report);
        tokio::spawn(async move {
            // 严格关闭与退出关闭经同一 lifecycle_lock 串行，不并行竞跑第二套编排；该等待也
            // 受同一绝对期限约束，不能在锁上无界阻塞。
            let guard = match budget.deadline() {
                Some(deadline) => {
                    let remaining = deadline.saturating_duration_since(Instant::now());
                    tokio::time::timeout(remaining, runtime.lifecycle_lock.lock())
                        .await
                        .ok()
                }
                None => Some(runtime.lifecycle_lock.lock().await),
            };
            let Some(_lifecycle_guard) = guard else {
                let _ = report.send_replace(Some(StudioShutdownReport::deadline_exceeded(
                    "lifecycleLock",
                    pl_protocol::studio::StudioError::internal().correlation_id,
                )));
                return;
            };
            let outcome = AssertUnwindSafe(runtime.execute_shutdown(budget, external_issues, None))
                .catch_unwind()
                .await;
            let value = match outcome {
                Ok(report) => report,
                Err(payload) => {
                    tracing::error!(
                        pid = std::process::id(),
                        panic = %panic_text(&payload),
                        "Studio shutdown orchestration task panicked; reporting Degraded"
                    );
                    StudioShutdownReport::panicked()
                }
            };
            let _ = report.send_replace(Some(value));
        });
    }

    /// 等到编排写入完成报告，或到达共享期限。
    async fn wait(&self, deadline: Option<Instant>) -> Option<StudioShutdownReport> {
        let mut receiver = self.report.subscribe();
        loop {
            let current: Option<StudioShutdownReport> = (*receiver.borrow()).clone();
            if let Some(report) = current {
                return Some(report);
            }
            match deadline {
                None => {
                    if receiver.changed().await.is_err() {
                        return None;
                    }
                }
                Some(deadline) => {
                    let remaining = deadline.saturating_duration_since(Instant::now());
                    if remaining.is_zero() {
                        return None;
                    }
                    match tokio::time::timeout(remaining, receiver.changed()).await {
                        Ok(Ok(())) => {}
                        Ok(Err(_)) | Err(_) => return None,
                    }
                }
            }
        }
    }
}

fn panic_text(payload: &(dyn std::any::Any + Send)) -> String {
    if let Some(message) = payload.downcast_ref::<&str>() {
        (*message).to_string()
    } else if let Some(message) = payload.downcast_ref::<String>() {
        message.clone()
    } else {
        "shutdown orchestration task panicked".to_string()
    }
}

/// 本次关闭可用的时间预算；`deadline` 为绝对上限，多个阶段共享同一剩余时间。
///
/// `None` 表示真正无界（严格路径）：不是用一个巨大的假魔数冒充“无上限”。
#[derive(Debug, Clone, Copy)]
pub(super) struct ShutdownBudget {
    deadline: Option<Instant>,
}

impl ShutdownBudget {
    pub(super) fn at_deadline(deadline: Instant) -> Self {
        Self {
            deadline: Some(deadline),
        }
    }

    /// 严格路径不设墙钟上限：失败保留 owner 并允许显式重试。
    pub(super) fn unbounded() -> Self {
        Self { deadline: None }
    }

    pub(super) fn deadline(&self) -> Option<Instant> {
        self.deadline
    }

    pub(super) fn remaining(&self) -> Option<Duration> {
        self.deadline
            .map(|deadline| deadline.saturating_duration_since(Instant::now()))
    }

    pub(super) fn expired(&self) -> bool {
        self.deadline
            .is_some_and(|deadline| Instant::now() >= deadline)
    }

    /// 单个阶段可等待的上限：无界预算下等价于阶段上限；有界预算下再取与总剩余的最小值。
    /// 预算已耗尽时返回 `None`。
    fn observe_wait(&self, limit: Duration) -> Option<Duration> {
        match self.remaining() {
            None => Some(limit),
            Some(remaining) if remaining.is_zero() => None,
            Some(remaining) => Some(limit.min(remaining)),
        }
    }

    /// 阶段的等待策略。
    ///
    /// `limit = None` 表示该阶段在严格（无界）预算下必须真实收束：不人为制造墙钟超时，避免
    /// 把正常的大量历史 drain 误判为失败；退出（有界）预算下仍与同一绝对期限取最小。
    /// `limit = Some(_)` 是外部服务自身收束上界，严格与退出路径都遵守。
    fn stage_policy(&self, limit: Option<Duration>) -> StagePolicy {
        match self.deadline {
            None => match limit {
                Some(limit) => StagePolicy::Bounded(limit),
                None => StagePolicy::Unbounded,
            },
            Some(deadline) => {
                let remaining = deadline.saturating_duration_since(Instant::now());
                if remaining.is_zero() {
                    return StagePolicy::Skipped;
                }
                match limit {
                    Some(limit) => StagePolicy::Bounded(limit.min(remaining)),
                    None => StagePolicy::Bounded(remaining),
                }
            }
        }
    }
}

/// 单个阶段的等待策略。
#[derive(Debug, Clone, Copy)]
enum StagePolicy {
    /// 在给定上限内等待。
    Bounded(Duration),
    /// 真实等待，不设本地墙钟上限（严格路径的存储 / Thread / writer 收束）。
    Unbounded,
    /// 预算已耗尽：取消已广播，跳过等待但保留 owner/handle。
    Skipped,
}

/// 一次关闭观测到的可靠持久化事实；缺失即为未知，绝不用零掩盖。
#[derive(Debug, Clone, Default)]
struct PersistenceFacts {
    pending: usize,
    save_watermark: Option<u64>,
    owner_id: Option<String>,
    /// 仍有待提交、故障或未收束水位的 Thread 标识。
    unresolved_thread_ids: Vec<String>,
    /// 所有 owner 中最保守（最小）的 admitted/durable 历史水位：全局确认要求每个 owner 都
    /// 至少推进到该值，因此用最小值而不是最大值，绝不用单个 owner 的最大值冒充全局确认。
    history_admitted_min: Option<u64>,
    history_durable_min: Option<u64>,
    fault_generation: Option<u64>,
}

/// 阶段诊断聚合器。
struct ShutdownCollector {
    budget: ShutdownBudget,
    started: Instant,
    issues: Vec<StudioShutdownIssue>,
    persistence: StudioPendingPersistence,
    store_closed: bool,
    /// 持久化 writer 是否已在本次编排中实际 ack 并移交（成功 flush 之后）。
    writer_reclaimed: bool,
    owner_id: Option<String>,
    save_watermark: Option<u64>,
    pending_hint: Option<usize>,
    unresolved_thread_ids: Vec<String>,
    history_admitted_min: Option<u64>,
    history_durable_min: Option<u64>,
    fault_generation: Option<u64>,
}

impl ShutdownCollector {
    fn new(budget: ShutdownBudget) -> Self {
        Self {
            budget,
            started: Instant::now(),
            issues: Vec::new(),
            persistence: StudioPendingPersistence::Unknown,
            store_closed: false,
            writer_reclaimed: false,
            owner_id: None,
            save_watermark: None,
            pending_hint: None,
            unresolved_thread_ids: Vec::new(),
            history_admitted_min: None,
            history_durable_min: None,
            fault_generation: None,
        }
    }

    fn expired(&self) -> bool {
        self.budget.expired()
    }

    fn has_issues(&self) -> bool {
        !self.issues.is_empty()
    }

    fn elapsed_ms(&self) -> u128 {
        self.started.elapsed().as_millis()
    }

    fn pending_label(&self) -> String {
        match self.pending_hint {
            Some(count) => format!("pending:{count}"),
            None => "unknown".to_string(),
        }
    }

    fn unresolved_label(&self) -> String {
        if self.unresolved_thread_ids.is_empty() {
            return "-".to_string();
        }
        let shown = self
            .unresolved_thread_ids
            .iter()
            .take(UNRESOLVED_THREAD_LOG_LIMIT)
            .cloned()
            .collect::<Vec<_>>()
            .join(",");
        let hidden = self
            .unresolved_thread_ids
            .len()
            .saturating_sub(UNRESOLVED_THREAD_LOG_LIMIT);
        if hidden == 0 {
            shown
        } else {
            format!("{shown},+{hidden}")
        }
    }

    fn push(&mut self, issue: StudioShutdownIssue) {
        self.issues.push(issue);
    }

    /// 播种调用方在进入编排前已经确定的失败（桥端 / Dart 侧订阅、更新取消未确认）。
    ///
    /// 它们与 runtime 自身阶段共享同一 `issues`，因此直接决定 `finalize_shutdown` 的成功
    /// 前置条件：只要存在任何一条，收束阶段就不会判定 `Clean`，也不会发布 `Stopped` 或释放
    /// 实例锁。诊断保留真实 `code` / `correlation_id`，不伪造 runtime 阶段的收束事实。
    fn seed_external_issues(&mut self, issues: Vec<StudioShutdownIssue>) {
        for issue in issues {
            tracing::error!(
                pid = std::process::id(),
                stage = %issue.stage,
                code = %issue.code,
                correlation_id = %issue.correlation_id,
                retryable = issue.retryable,
                elapsed_ms = self.elapsed_ms(),
                detail = %issue.message,
                "Studio shutdown precondition failed before runtime finalization"
            );
            self.push(issue);
        }
    }

    /// 记录一个阶段/子步骤的真实失败。
    ///
    /// `stage_elapsed_ms` 是该阶段自身的耗时（非阶段调用点传总耗时），与总 `elapsed_ms` 一并输出，
    /// 便于从日志确认独立阶段确实并发收束而不是被逐个串行等待。
    fn record_error(&mut self, stage: &str, stage_elapsed_ms: u128, error: &anyhow::Error) {
        let mapped = crate::error_mapping::studio_error_from_anyhow_ref(error);
        // Diagnostics keep only redacted, structure-level facts: the shared `StudioError` message,
        // `std::io` kind/OS code and the database failure category/code. The raw source chain can
        // embed prompt bodies or credentials, so its message bodies are never written verbatim;
        // the backtrace is force-captured so the stack is a real fact rather than an `anyhow`
        // backtrace that may be disabled.
        tracing::error!(
            pid = std::process::id(),
            stage,
            code = mapped.code.label(),
            correlation_id = %mapped.correlation_id,
            retryable = mapped.retryable,
            pending = %self.pending_label(),
            stage_elapsed_ms,
            elapsed_ms = self.elapsed_ms(),
            owner_id = self.owner_id.as_deref().unwrap_or("-"),
            save_watermark = self.save_watermark,
            history_admitted_min = self.history_admitted_min,
            history_durable_min = self.history_durable_min,
            fault_generation = self.fault_generation,
            unresolved_threads = %self.unresolved_label(),
            error_bytes = error.to_string().len(),
            error_chain = %redacted_chain(error),
            backtrace = %std::backtrace::Backtrace::force_capture(),
            detail = %mapped.message,
            "Studio shutdown stage failed"
        );
        self.push(StudioShutdownIssue {
            stage: stage.to_string(),
            code: mapped.code.label().to_string(),
            message: mapped.message,
            retryable: mapped.retryable,
            correlation_id: mapped.correlation_id,
        });
    }

    fn record_join_error(
        &mut self,
        stage: &str,
        stage_elapsed_ms: u128,
        error: &tokio::task::JoinError,
    ) {
        let message = if error.is_panic() {
            format!("shutdown stage task panicked: {error}")
        } else {
            format!("shutdown stage task failed: {error}")
        };
        self.record_error(stage, stage_elapsed_ms, &anyhow::anyhow!(message));
    }

    /// 记录外部服务关闭任务的真实失败，保留底层原因链（不吞错、不伪成功）。
    fn record_service_failure(
        &mut self,
        stage: &str,
        stage_elapsed_ms: u128,
        failure: &ServiceStopFailure,
    ) {
        match failure {
            ServiceStopFailure::Failed(error) => self.record_error(stage, stage_elapsed_ms, error),
            ServiceStopFailure::Panicked(join) => {
                self.record_join_error(stage, stage_elapsed_ms, join)
            }
        }
    }

    fn record_timeout(&mut self, stage: &str, stage_elapsed_ms: u128) {
        let mapped = pl_protocol::studio::StudioError::new(
            pl_protocol::studio::StudioErrorCode::Busy,
            "shutdown stage exceeded its time budget",
            true,
        );
        tracing::error!(
            pid = std::process::id(),
            stage,
            code = "timeout",
            correlation_id = %mapped.correlation_id,
            pending = %self.pending_label(),
            stage_elapsed_ms,
            elapsed_ms = self.elapsed_ms(),
            owner_id = self.owner_id.as_deref().unwrap_or("-"),
            save_watermark = self.save_watermark,
            history_admitted_min = self.history_admitted_min,
            history_durable_min = self.history_durable_min,
            fault_generation = self.fault_generation,
            unresolved_threads = %self.unresolved_label(),
            "Studio shutdown stage timed out; owner/handle retained"
        );
        self.push(StudioShutdownIssue {
            stage: stage.to_string(),
            code: "timeout".to_string(),
            message: mapped.message,
            retryable: true,
            correlation_id: mapped.correlation_id,
        });
    }

    fn record_skipped(&mut self, stage: &str, stage_elapsed_ms: u128) {
        let mapped = pl_protocol::studio::StudioError::new(
            pl_protocol::studio::StudioErrorCode::Busy,
            "shutdown budget elapsed before this stage could be joined",
            true,
        );
        tracing::warn!(
            pid = std::process::id(),
            stage,
            code = "deadlineExceeded",
            correlation_id = %mapped.correlation_id,
            stage_elapsed_ms,
            elapsed_ms = self.elapsed_ms(),
            "Studio shutdown stage not joined within budget; cancellation already broadcast"
        );
        self.push(StudioShutdownIssue {
            stage: stage.to_string(),
            code: "deadlineExceeded".to_string(),
            message: mapped.message,
            retryable: true,
            correlation_id: mapped.correlation_id,
        });
    }

    /// 记录一个并发阶段的真实结果并统一聚合。
    ///
    /// 阶段在各自分支内只产生 `StageOutcome`，回到编排线程后按固定顺序聚合：这样多个独立阶段
    /// 可以真正并发发起与并发等待，而不会被共享的 `ShutdownCollector` 串行化，报告顺序仍稳定。
    fn record_stage(&mut self, outcome: StageOutcome) {
        let StageOutcome {
            stage,
            status,
            elapsed,
            ..
        } = outcome;
        let stage_elapsed_ms = elapsed.as_millis();
        match status {
            StageStatus::Done => {
                tracing::info!(
                    pid = std::process::id(),
                    stage,
                    resource = "runtime",
                    stage_elapsed_ms,
                    elapsed_ms = self.elapsed_ms(),
                    "Studio shutdown stage completed"
                );
            }
            StageStatus::Failed(error) => {
                self.record_error(stage, stage_elapsed_ms, &error);
            }
            StageStatus::Panicked(join) => {
                self.record_join_error(stage, stage_elapsed_ms, &join);
            }
            StageStatus::TimedOut => self.record_timeout(stage, stage_elapsed_ms),
            StageStatus::Skipped => self.record_skipped(stage, stage_elapsed_ms),
            StageStatus::ServiceFailure(failure) => {
                self.record_service_failure(stage, stage_elapsed_ms, &failure);
            }
        }
    }

    fn report(self) -> StudioShutdownReport {
        let clean = !self.has_issues()
            && matches!(self.persistence, StudioPendingPersistence::Drained)
            && self.store_closed;
        StudioShutdownReport {
            outcome: if clean {
                StudioShutdownOutcome::Clean
            } else {
                StudioShutdownOutcome::Degraded
            },
            issues: self.issues,
            persistence: self.persistence,
        }
    }
}

/// 已脱敏的错误来源链摘要。
///
/// 关机诊断不得写入正文或凭据，但也不能只留一个 generic message 或纯字节数——仅凭字节数无法
/// 定位 SQLite / OS 失败原因。这里保留**结构性的事实**：typed `StudioError` 的稳定诊断码与
/// 已脱敏 message、`std::io::Error` 的 kind 与原始 OS 错误码、`sea_orm::DbErr` 的失败分类与
/// 可公开的 SQL 错误码；未知类型只保留规模。所有分支都不写正文或凭据。
fn redacted_chain(error: &anyhow::Error) -> String {
    let mut parts = Vec::new();
    for cause in error.chain() {
        if let Some(studio) = cause.downcast_ref::<pl_protocol::studio::StudioError>() {
            parts.push(format!("[{}] {}", studio.code.label(), studio.message));
        } else if let Some(io) = cause.downcast_ref::<std::io::Error>() {
            let os_code = io
                .raw_os_error()
                .map_or_else(|| "-".to_string(), |code| code.to_string());
            parts.push(format!("io/{:?}#{}", io.kind(), os_code));
        } else if let Some(db) = cause.downcast_ref::<sea_orm::DbErr>() {
            parts.push(db_error_summary(db));
        } else {
            // 未知 cause：保留规模而不写内容，避免把可能的正文或凭据带入日志。
            parts.push(format!("<unclassified {} bytes>", cause.to_string().len()));
        }
    }
    parts.join(" <- ")
}

/// Safe, structure-only summary of a SeaORM database error (no SQL or row content).
fn db_error_summary(error: &sea_orm::DbErr) -> String {
    use std::ops::Deref;

    use sea_orm::{DbErr, RuntimeErr};

    let kind = match error {
        DbErr::Conn(_) => "connection",
        DbErr::Exec(_) => "exec",
        DbErr::Query(_) => "query",
        _ => "other",
    };
    // The backend error code is a stable, non-sensitive identifier (e.g. SQLite/Postgres code).
    let code = if let DbErr::Query(RuntimeErr::SqlxError(error))
    | DbErr::Exec(RuntimeErr::SqlxError(error)) = error
        && let sea_orm::sqlx::Error::Database(database) = error.deref()
    {
        database.code().map(|code| code.into_owned())
    } else {
        None
    };
    match code {
        Some(code) => format!("db/{kind}#{code}"),
        None => format!("db/{kind}"),
    }
}

/// 一次外部服务关闭任务真实结果；可多次观测，不因等待超时丢失。
pub(crate) enum ServiceStopFailure {
    /// 服务自身确认失败（含 worker 明确拒绝）。
    Failed(anyhow::Error),
    /// owned 关闭任务 panic / 被取消。
    Panicked(tokio::task::JoinError),
}

/// 外部服务关闭的共享完成句柄。
///
/// 关闭任务在 runtime 中只启动一次并常驻于 `ServiceStopSlot`；重复严格关闭 / 重试只再次观测
/// 同一个共享 completion，不重启 stop，也不因等待超时而丢失 owner 或句柄。底层 owned task 与
/// completion 都由 slot 常驻持有，运行时会持续 poll 到结束。
type ServiceStopCompletion = futures::future::Shared<
    futures::future::BoxFuture<'static, Result<(), Arc<ServiceStopFailure>>>,
>;

#[derive(Clone)]
pub(crate) struct ServiceStopJob {
    completion: ServiceStopCompletion,
}

impl ServiceStopJob {
    pub(crate) fn spawn<Fut>(future: Fut) -> Self
    where
        Fut: std::future::Future<Output = anyhow::Result<()>> + Send + 'static,
    {
        let handle = tokio::spawn(future);
        let completion = async move {
            match handle.await {
                Ok(Ok(())) => Ok(()),
                Ok(Err(error)) => Err(Arc::new(ServiceStopFailure::Failed(error))),
                Err(join) => Err(Arc::new(ServiceStopFailure::Panicked(join))),
            }
        }
        .boxed()
        .shared();
        Self { completion }
    }

    /// 观测关闭结果；多个调用者共享同一 completion，超时只放弃等待。
    pub(crate) async fn join(&self) -> Result<(), Arc<ServiceStopFailure>> {
        self.completion.clone().await
    }
}

/// 单个服务关闭任务的常驻槽位；被 runtime 持有，跨阶段与跨重试保留。
pub(crate) type ServiceStopSlot = Arc<tokio::sync::Mutex<Option<ServiceStopJob>>>;

/// runtime 常驻的三个外部服务关闭槽位。
#[derive(Clone, Default)]
pub(crate) struct ServiceStopSlots {
    pub(crate) mcp: ServiceStopSlot,
    pub(crate) lsp: ServiceStopSlot,
    pub(crate) ssh: ServiceStopSlot,
}

/// 本次编排要 join 的服务关闭共享句柄（按需从常驻槽位 ensure）。
///
/// 只含与 SSH transport 无依赖的远端服务：MCP 与 LSP 的 stop 可并发发起。SSH transport 承载
/// 远端 MCP/LSP/工具的通信，必须在这些依赖收束之后才请求回收，因此不在本结构内，而在
/// `run_shutdown_stages` 的依赖闸门之后单独从 `service_stops.ssh` 槽位 ensure。
struct ServiceStops {
    mcp: ServiceStopJob,
    lsp: ServiceStopJob,
}

/// 一个并发阶段的真实结果。
///
/// 阶段在各自分支内产生本结构，回到编排线程后按 `order` 统一聚合。它不持有
/// `ShutdownCollector`，因此多个独立阶段可以真正并发发起与并发等待，而不是被一把共享锁串行化。
/// `elapsed` 是该阶段自身的收束耗时，用于日志确认独立阶段确实并发完成。
struct StageOutcome {
    order: usize,
    stage: &'static str,
    status: StageStatus,
    elapsed: Duration,
}

impl StageOutcome {
    /// 该阶段是否真实确认收束（用于写入链的成功判定）。
    fn is_done(&self) -> bool {
        matches!(&self.status, StageStatus::Done)
    }
}

/// 并发阶段的终态；失败原因保留 typed / JoinError 事实，留待统一聚合时脱敏记录。
enum StageStatus {
    /// 阶段真实完成并确认收束。
    Done,
    /// 阶段自身返回错误。
    Failed(anyhow::Error),
    /// 阶段的 owned task panic / 被取消。
    Panicked(tokio::task::JoinError),
    /// 外部服务关闭任务确认失败。
    ServiceFailure(Arc<ServiceStopFailure>),
    /// 阶段未在上限内收束；只放弃等待，owner/handle 仍被保留。
    TimedOut,
    /// 共享预算已耗尽，取消已广播但未 join。
    Skipped,
}

/// 一次并发阶段推进的汇总结果；回到 `execute_shutdown` 后再写入单一 collector。
///
/// 阶段本身不直接修改 collector，因此它们可以真正并发；这里的字段与 issue 一并由编排线程按
/// 稳定顺序聚合，保证 writer ack 与持久化判定仍由同一 finalize 前置条件决定。
struct StageReport {
    outcomes: Vec<StageOutcome>,
    writer_reclaimed: bool,
    persistence: StudioPendingPersistence,
}

/// 一个独立阶段分支的并发句柄：完成后返回它贡献的阶段结果。
type StageBranch = BoxFuture<'static, Vec<StageOutcome>>;

/// 并发 poll 一组阶段分支直至全部收束，收集它们贡献的阶段结果。
///
/// `FuturesUnordered` 首次 poll 时会把全部已入队的分支一起推进，因此同一依赖层内的阶段在同一
/// 时刻发起，并共享同一首次绝对期限的剩余预算。
async fn drain_stage_branches(branches: &mut FuturesUnordered<StageBranch>) -> Vec<StageOutcome> {
    let mut outcomes = Vec::new();
    while let Some(mut produced) = branches.next().await {
        outcomes.append(&mut produced);
    }
    outcomes
}

/// 并发运行的单个 runtime 阶段：先 spawn 为 owned task，再按共享预算有界 join。
///
/// 独立阶段各自调用一次，`FuturesUnordered` 会并发 poll 全部阶段，因此它们的 owned task 在几乎
/// 同一时刻启动，并共享同一首次绝对期限的剩余预算——不会出现先 join 一小部分、用它们的等待
/// 耗尽后续阶段预算的串行缺陷。超时 / 预算耗尽只放弃等待，绝不 abort / drop 在途的存储、writer
/// 或 JoinHandle 所有者。
async fn run_stage_owned<Fut>(
    runtime: StudioRuntime,
    budget: ShutdownBudget,
    order: usize,
    stage: &'static str,
    limit: Option<Duration>,
    // factory 被持有在 async fn 状态里跨 await，必须 `Send`，否则整个 owned 阶段 future 不是
    // `Send`，无法放进要求跨线程的 `BoxFuture`（`StageBranch`）与 `tokio::spawn`。
    make: impl FnOnce(StudioRuntime) -> Fut + Send,
) -> StageOutcome
where
    Fut: Future<Output = Result<()>> + Send + 'static,
{
    tracing::info!(
        pid = std::process::id(),
        stage,
        resource = "runtime",
        "Studio shutdown stage started"
    );
    let started = Instant::now();
    let mut task: JoinHandle<Result<()>> = tokio::spawn(make(runtime));
    let status = match budget.stage_policy(limit) {
        StagePolicy::Skipped => StageStatus::Skipped,
        StagePolicy::Bounded(wait) => match tokio::time::timeout(wait, &mut task).await {
            Ok(Ok(Ok(()))) => StageStatus::Done,
            Ok(Ok(Err(error))) => StageStatus::Failed(error),
            Ok(Err(join)) => StageStatus::Panicked(join),
            Err(_) => StageStatus::TimedOut,
        },
        StagePolicy::Unbounded => match task.await {
            Ok(Ok(())) => StageStatus::Done,
            Ok(Err(error)) => StageStatus::Failed(error),
            Err(join) => StageStatus::Panicked(join),
        },
    };
    StageOutcome {
        order,
        stage,
        status,
        elapsed: started.elapsed(),
    }
}

/// 并发观测 `ensure_service_stops` 启动的常驻关闭 job；消费真实 typed 结果。
///
/// 多个服务的 join 由调用方用 `FuturesUnordered` 并发 poll，因此它们的等待共享同一首次绝对期限
/// 的剩余预算，一个服务的挂起不占用其他服务的收束预算。超时 / 预算耗尽只放弃等待：共享 completion
/// 与 owned task 由 runtime 槽位常驻持有，后续严格重试可再次观测同一结果，不重启 stop。
async fn join_service_outcome(
    budget: ShutdownBudget,
    order: usize,
    stage: &'static str,
    job: ServiceStopJob,
    limit: Option<Duration>,
) -> StageOutcome {
    tracing::info!(
        pid = std::process::id(),
        stage,
        resource = "service",
        "Studio shutdown stage started"
    );
    let started = Instant::now();
    let wait = job.join();
    let status = match budget.stage_policy(limit) {
        StagePolicy::Skipped => StageStatus::Skipped,
        StagePolicy::Bounded(limit) => match tokio::time::timeout(limit, wait).await {
            Ok(Ok(())) => StageStatus::Done,
            Ok(Err(failure)) => StageStatus::ServiceFailure(failure),
            Err(_) => StageStatus::TimedOut,
        },
        StagePolicy::Unbounded => match wait.await {
            Ok(()) => StageStatus::Done,
            Err(failure) => StageStatus::ServiceFailure(failure),
        },
    };
    StageOutcome {
        order,
        stage,
        status,
        elapsed: started.elapsed(),
    }
}

/// 有界收束注入的外部 owner ACK（bridge adapter hook），其 issue 与 runtime 阶段共享同一成功
/// 前置条件。hook 在 runtime 已完成「封闭准入 + 广播独立取消」之后运行，并且与 runtime 阶段
/// 并发：它只返回 issue，由调用方在同一 finalize 之前聚合，不直接持有 collector。
async fn collect_external_hook_issues(
    hook: Option<ShutdownExternalHook>,
) -> Vec<StudioShutdownIssue> {
    match hook {
        Some(hook) => hook().await,
        None => Vec::new(),
    }
}

/// 报告内阶段的稳定排序权重；与完成先后无关，保证并发收束下报告顺序可复现。
mod stage_order {
    pub(super) const AGENT_FRAMEWORK: usize = 0;
    pub(super) const PERSISTENCE: usize = 1;
    pub(super) const MODEL_CATALOG: usize = 2;
    pub(super) const RECOVERY: usize = 3;
    pub(super) const MODEL_REFRESH: usize = 4;
    pub(super) const TITLE: usize = 5;
    pub(super) const MCP_STARTUP_RECONCILE: usize = 6;
    pub(super) const MCP_HEALTH: usize = 7;
    pub(super) const MCP_STOP: usize = 8;
    pub(super) const MCP_STOPPED: usize = 9;
    pub(super) const LSP_STATE: usize = 10;
    pub(super) const LSP_STOP: usize = 11;
    pub(super) const LSP_STOPPED: usize = 12;
    pub(super) const TOOL_REFRESH: usize = 13;
    pub(super) const PERSISTENCE_OBSERVER: usize = 14;
    pub(super) const SSH: usize = 15;
}

/// 在常驻槽位上只启动一次真正的 stop 工作；已存在则返回同一共享句柄。
async fn ensure_service_stop(
    slot: &ServiceStopSlot,
    start: impl FnOnce() -> ServiceStopJob,
) -> ServiceStopJob {
    let mut guard = slot.lock().await;
    if let Some(job) = guard.as_ref() {
        return job.clone();
    }
    let job = start();
    *guard = Some(job.clone());
    job
}

impl StudioRuntime {
    /// 只读持久化诊断事实：保留中的 writer owner 标识与已 durable 保存水位。
    ///
    /// 只读取现有事实，不改变任何 owner；锁竞争或 owner 缺失时返回 `"未知"`，不阻塞、
    /// 绝不用零掩盖未知。
    pub fn persistence_diagnostics(&self) -> (Option<String>, Option<u64>) {
        match self.agent_facility.persistence.try_lock() {
            Ok(slot) => match slot.as_ref() {
                Some(repository) => (
                    Some("write-behind-writer".to_string()),
                    Some(repository.saved_watermark()),
                ),
                None => (None, None),
            },
            Err(_) => (None, None),
        }
    }

    /// 严格关闭：所有阶段成功才算成功，失败保留 owner 与重试入口。
    pub(in crate::studio::runtime) async fn execute_strict_shutdown(
        &self,
    ) -> Result<StudioShutdownReport> {
        self.execute_strict_shutdown_with_hook(None).await
    }

    /// 严格关闭，可注入收集外部 owner ACK 的 adapter hook。
    ///
    /// hook 在 runtime 提交（空闲原子检查通过、准入封闭）之后、收尾之前于同一编排内运行，并与
    /// runtime 阶段**并发**收束（共享同一首次绝对期限的剩余预算，谁都不会耗尽对方尚未开始的等待
    /// 预算）；其返回的 issue 与 runtime 阶段共享同一成功前置条件，因此外部 owner 未确认时不会
    /// 发布 `Stopped`，也不释放实例锁。
    pub(in crate::studio::runtime) async fn execute_strict_shutdown_with_hook(
        &self,
        hook: Option<ShutdownExternalHook>,
    ) -> Result<StudioShutdownReport> {
        let report = self
            .execute_shutdown(ShutdownBudget::unbounded(), Vec::new(), hook)
            .await;
        if report.is_clean() {
            Ok(report)
        } else {
            Err(shutdown_report_error(&report))
        }
    }

    /// 应用退出关闭：返回 typed 报告，接受 `Degraded`。
    ///
    /// 首次调用的剩余预算固定唯一 deadline；重复调用只 join 同一个 owned 编排任务，不延长期限。
    ///
    /// `external_issues` 是调用方（native / Dart 与 bridge 自身）在进入 runtime 编排之前
    /// 已经确定的失败，例如桥端或 Dart 侧订阅、更新取消未确认。它们被就地播种为这次关闭的
    /// 成功前置条件，绝不事后合并：只要存在任何一条，收束阶段就不会判定 `Clean`，也就不会
    /// 发布 `Stopped` 或释放实例锁。
    pub(in crate::studio::runtime) async fn execute_exit_shutdown(
        &self,
        remaining_ms: u32,
        external_issues: Vec<StudioShutdownIssue>,
    ) -> StudioShutdownReport {
        // 同步封闭 runtime 准入闩：先于任何 await 与锁等待，退出期间拒绝迟到安装与新 mutation。
        self.shutdown_latch.arm();
        let requested = Duration::from_millis(u64::from(remaining_ms));
        let deadline = self.shutdown_run.fixed_deadline(requested);
        let budget = ShutdownBudget::at_deadline(deadline);
        self.shutdown_run.spawn_once(self, budget, external_issues);
        match self.shutdown_run.wait(Some(deadline)).await {
            Some(report) => report,
            None => StudioShutdownReport::deadline_exceeded(
                "deadline",
                pl_protocol::studio::StudioError::internal().correlation_id,
            ),
        }
    }

    /// 应用退出的早期封闭步骤（native 首次 close 时调用）。
    ///
    /// 与随后的 `shutdown_runtime_for_exit` 共用同一关闭 coordinator：本步只做两件在途安装
    /// 无法越过的封闭动作——
    ///
    /// 1. arm 单向终止闩（先于任何 await），并同步把可关闭状态推进到 `ShuttingDown`，封闭新
    ///    mutation 与迟到安装；
    /// 2. 向所有彼此独立的在途 owner（后台 watcher、title 任务、持久化 observer）广播一次
    ///    取消 / abort。
    ///
    /// 它绝不启动 MCP/LSP/SSH 的真正 stop、绝不 join、绝不关闭数据库 / 释放实例锁，也不发布
    /// `Stopped`——这些仍由随后的同一编排完成，早期 arm 不会被误认为最终终态。早期观测到的诊断
    /// 被写入常驻槽位，由 `execute_shutdown` 就地播种为成功前置条件，既不吞错也不伪成功。
    ///
    /// 本步有界：独立 owner 的取消广播各自带锁上限，不做任何可能无界阻塞的 join；状态观测与
    /// 推进只持有无 await 的短临界区，绝不在任何锁上无界等待。
    pub async fn fence_exit_admission(&self) -> Vec<StudioShutdownIssue> {
        self.shutdown_latch.arm();
        let mut issues = Vec::new();
        let snapshot = self.runtime_state.snapshot();
        // 只对可关闭状态推进为 `ShuttingDown`；未安装 / 初始化中 / 已进入关闭或终态既不触发
        // 初始化，也不重复推进状态。
        if matches!(
            snapshot.state.kind(),
            StudioRuntimeStateKind::Ready | StudioRuntimeStateKind::Failed
        ) && let Err(error) = self
            .runtime_state
            .apply(StudioRuntimeCommand::BeginShutdown {
                expected_revision: snapshot.revision,
                at: unix_seconds(),
            })
        {
            issues.push(shutdown_issue_from_error(
                "beginShutdown",
                &anyhow::Error::from(error),
            ));
        }
        self.broadcast_cancel().await;
        if !issues.is_empty() {
            for issue in &issues {
                tracing::error!(
                    pid = std::process::id(),
                    stage = %issue.stage,
                    code = %issue.code,
                    resource = "runtimeState",
                    correlation_id = %issue.correlation_id,
                    retryable = issue.retryable,
                    detail = %issue.message,
                    backtrace = %std::backtrace::Backtrace::force_capture(),
                    "Studio runtime early exit fence reported a failure"
                );
            }
            let mut slot = match self.early_exit_issues.lock() {
                Ok(slot) => slot,
                Err(poisoned) => poisoned.into_inner(),
            };
            slot.extend(issues.iter().cloned());
        }
        issues
    }

    /// 取走早期退出封闭观测到的诊断，交由同一编排播种（只取一次，重试不重复播种）。
    fn take_early_exit_issues(&self) -> Vec<StudioShutdownIssue> {
        let mut slot = match self.early_exit_issues.lock() {
            Ok(slot) => slot,
            Err(poisoned) => poisoned.into_inner(),
        };
        std::mem::take(&mut *slot)
    }

    /// 统一阶段编排；严格与退出路径都经由它，不维护第二套实现。
    ///
    /// `external_issues` 作为同一编排的最终成功前置条件被就地播种，不是事后合并的第二事实源。
    async fn execute_shutdown(
        &self,
        budget: ShutdownBudget,
        external_issues: Vec<StudioShutdownIssue>,
        hook: Option<ShutdownExternalHook>,
    ) -> StudioShutdownReport {
        let mut collector = ShutdownCollector::new(budget);

        // 只读持久化事实先于快照观测，让后续任何早期失败的诊断都带真实 owner / 水位。
        self.capture_persistence_facts(&mut collector).await;
        let current = self.observe_snapshot(&mut collector).await;

        // 播种调用方（Dart / bridge）在进入编排前已经确定的失败，作为这次关闭的成功前置条件；
        // 放在事实观测之后，使诊断带上真实 owner 与保存水位。
        collector.seed_external_issues(external_issues);
        // native 首次 close 的早期封闭（准入封闭 + 独立取消广播）遗留的诊断同样作为这次关闭的
        // 成功前置条件播种：早期 arm 未被误认为最终终态，早期错误也不被吞掉。
        collector.seed_external_issues(self.take_early_exit_issues());

        // 从未安装 / 已终态：不触发初始化、不做阶段编排。但调用方与 bridge 持有的外部 owner
        // 仍可能未确认，必须按同一成功前置条件收束（运行注入的 adapter hook），绝不能以
        // `Clean`/`NotStarted` 掩盖未收束的 owner。
        let never_installed = current.as_ref().is_some_and(|current| {
            matches!(current.state.kind(), StudioRuntimeStateKind::Uninitialized)
        });
        let already_stopped = current
            .as_ref()
            .is_some_and(|current| current.state.is_stopped());
        if never_installed || already_stopped {
            collector.seed_external_issues(collect_external_hook_issues(hook).await);
            return if collector.has_issues() {
                collector.report()
            } else if never_installed {
                // 从未安装：不触发初始化、不武装闩；没有待保存事实。
                StudioShutdownReport::not_started()
            } else {
                // 已经处于终态的 runtime 只有在没有任何（含 hook 收束的）issue 时才可报 Clean。
                StudioShutdownReport {
                    outcome: StudioShutdownOutcome::Clean,
                    issues: Vec::new(),
                    persistence: StudioPendingPersistence::Drained,
                }
            };
        }

        // 单向终止闩：先于任何阶段武装，拒绝迟到安装；快照不可用时也照样封闭，绝不因只读观测
        // 失败而漏掉独立取消（准入封闭与取消不依赖 runtime 快照）。
        self.shutdown_latch.arm();

        if let Some(current) = current.as_ref()
            && matches!(current.state.kind(), StudioRuntimeStateKind::Initializing)
        {
            // 初始化未完成：不得伪报 NotStarted；记录诊断后仍继续安全清理已创建资源。
            let stage_elapsed_ms = collector.elapsed_ms();
            collector.record_error(
                "installation",
                stage_elapsed_ms,
                &anyhow::anyhow!("runtime initialization is still in progress"),
            );
        }

        // BeginShutdown（best-effort）：只接受 Ready/Failed 的 typed 命令；快照缺失或初始化中仍
        // 封闭准入并继续独立清理，失败本身被记为诊断而不会阻断取消与收尾。
        let begin = match current.as_ref() {
            Some(current)
                if matches!(
                    current.state.kind(),
                    StudioRuntimeStateKind::Ready | StudioRuntimeStateKind::Failed
                ) =>
            {
                self.runtime_state
                    .apply(StudioRuntimeCommand::BeginShutdown {
                        expected_revision: current.revision,
                        at: unix_seconds(),
                    })
                    .map(|_| ())
            }
            Some(_) => Ok(()),
            None => {
                let revision = self.runtime_state.snapshot().revision;
                self.runtime_state
                    .apply(StudioRuntimeCommand::BeginShutdown {
                        expected_revision: revision,
                        at: unix_seconds(),
                    })
                    .map(|_| ())
            }
        };
        if let Err(error) = begin {
            // BeginShutdown 失败不可吞：直接进入 Degraded 但继续安全清理。
            let stage_elapsed_ms = collector.elapsed_ms();
            collector.record_error("beginShutdown", stage_elapsed_ms, &error.into());
        }

        // 提交准入后**先**向所有独立 owner 广播取消，并启动/常驻保留 MCP/LSP/SSH 的 owned 关闭
        // job。任何外部 ACK 的有界收束都必须发生在这之后：这样注入的 hook 即使卡住，也不会阻止
        // runtime 的独立资源收到取消。真正的阶段 join 与 finalize 仍在后续同一编排内、复用同一批 job。
        self.broadcast_cancel().await;
        let services = self.ensure_service_stops().await;

        // 再有界收束注入的外部 owner ACK（adapter hook）与 runtime 阶段**并发**收束：两者共享
        // 同一首次绝对期限的剩余预算，谁都不会耗掉对方尚未开始的等待预算（此前 hook 的 join 会
        // 抢在 runtime 独立资源之前耗尽期限）。hook 的 issue 仍进入同一成功前置条件，并在同一
        // finalize 之前聚合，不引入第二套编排。
        let (hook_issues, stages) = futures::join!(
            collect_external_hook_issues(hook),
            self.run_shutdown_stages(budget, collector.pending_hint, &services),
        );
        collector.seed_external_issues(hook_issues);
        collector.writer_reclaimed = stages.writer_reclaimed;
        collector.persistence = stages.persistence;
        for outcome in stages.outcomes {
            collector.record_stage(outcome);
        }

        self.finalize_shutdown(&mut collector).await;
        collector.report()
    }

    async fn observe_snapshot(
        &self,
        collector: &mut ShutdownCollector,
    ) -> Option<StudioRuntimeSnapshot> {
        let wait = collector
            .budget
            .observe_wait(FACTS_LIMIT)
            .unwrap_or(Duration::ZERO);
        match tokio::time::timeout(wait, self.runtime_snapshot()).await {
            Ok(Ok(snapshot)) => Some(snapshot),
            Ok(Err(error)) => {
                let stage_elapsed_ms = collector.elapsed_ms();
                collector.record_error("snapshot", stage_elapsed_ms, &error);
                None
            }
            Err(_) => {
                let stage_elapsed_ms = collector.elapsed_ms();
                collector.record_timeout("snapshot", stage_elapsed_ms);
                None
            }
        }
    }

    /// 观测真实持久化事实（可未知）；只读，不改变 owner。
    async fn capture_persistence_facts(&self, collector: &mut ShutdownCollector) {
        let wait = collector
            .budget
            .observe_wait(FACTS_LIMIT)
            .unwrap_or(Duration::ZERO);
        match tokio::time::timeout(wait, self.persistence_facts()).await {
            Ok(Ok(facts)) => {
                collector.pending_hint = Some(facts.pending);
                collector.save_watermark = facts.save_watermark;
                collector.owner_id = facts.owner_id;
                collector.unresolved_thread_ids = facts.unresolved_thread_ids;
                collector.history_admitted_min = facts.history_admitted_min;
                collector.history_durable_min = facts.history_durable_min;
                collector.fault_generation = facts.fault_generation;
            }
            Ok(Err(error)) => {
                let stage_elapsed_ms = collector.elapsed_ms();
                collector.record_error("persistenceFacts", stage_elapsed_ms, &error);
            }
            Err(_) => {
                let stage_elapsed_ms = collector.elapsed_ms();
                collector.record_timeout("persistenceFacts", stage_elapsed_ms);
            }
        }
    }

    async fn persistence_facts(&self) -> Result<PersistenceFacts> {
        let repository = self.agent_facility.persistence.lock().await.clone();
        let (writer_pending, save_watermark, owner_id) = match repository {
            Some(repository) => (
                repository.pending_commit_count(),
                Some(repository.saved_watermark()),
                Some("write-behind-writer".to_string()),
            ),
            None => (0, None, None),
        };

        // ThreadHistoryCoordinator 的真实水位：逐 Thread 的 admitted/durable/fault 与未收束标识。
        // 全局确认水位取所有 owner 的**最小值**：只有每个 owner 都推进到该值才能对外声明
        // “已确认到此处”，绝不用某个 owner 的最大值冒充全局确认。
        let snapshot = self.store.thread_persistence().snapshot();
        let mut unresolved_thread_ids = Vec::new();
        let mut history_admitted_min: Option<u64> = None;
        let mut history_durable_min: Option<u64> = None;
        let mut fault_generation: Option<u64> = None;
        for thread in &snapshot.threads {
            if thread.pending_operations > 0
                || thread.in_flight_bytes > 0
                || thread.last_error.is_some()
                || thread.fault.is_some()
            {
                unresolved_thread_ids.push(thread.thread_id.clone());
            }
            if let Some(admitted) = thread.history_admitted_sequence {
                history_admitted_min =
                    Some(history_admitted_min.map_or(admitted, |prev| prev.min(admitted)));
            }
            if let Some(durable) = thread.history_durable_sequence {
                history_durable_min =
                    Some(history_durable_min.map_or(durable, |prev| prev.min(durable)));
            }
            if thread.fault.is_some() {
                fault_generation = Some(fault_generation.map_or(thread.fault_generation, |prev| {
                    prev.max(thread.fault_generation)
                }));
            }
        }

        Ok(PersistenceFacts {
            pending: writer_pending
                + usize::try_from(snapshot.pending_commits).unwrap_or(usize::MAX),
            save_watermark,
            owner_id,
            unresolved_thread_ids,
            history_admitted_min,
            history_durable_min,
            fault_generation,
        })
    }

    /// 在阶段 join 之前，向所有独立 owner 广播取消 / abort。
    ///
    /// 背景 watcher 用 abort 即刻停摆；title 任务用一次性取消信号。此步不做任何可能无界阻塞的
    /// 服务 join，保证所有独立资源先收到取消。
    async fn broadcast_cancel(&self) {
        use super::super::background_task;
        background_task::signal(&self.settings_refresh).await;
        background_task::signal(&self.recovery_task).await;
        background_task::signal(&self.external_runtimes.mcp_startup_reconcile).await;
        background_task::signal(&self.external_runtimes.mcp_health_watcher).await;
        background_task::signal(&self.external_runtimes.lsp_state_watcher).await;
        background_task::signal(&self.persistence_observer).await;
        self.title_tasks.signal_cancel().await;
        // 活动 Turn / 模型 / 工具：通过既有的纯取消入口取消每个驻留 Thread 当前执行代次。
        // 这里只发信号，不 join、也不 close——真正的收束仍由后续唯一 `agentFramework` 阶段完成，
        // 因此不会重复 close，也绝不提前释放 writer 或实例锁。先前已封闭准入，取消只是加速在途
        // 执行的退出，满足「先封闭准入并广播取消（含 Turn）再 join」。
        for (_, handle) in self.threads.observed_threads() {
            handle.interrupt();
        }
    }

    /// 确保与 SSH transport 无依赖的远端服务（MCP、LSP）各自唯一一次的真实 stop 工作已在常驻
    /// 槽位启动。SSH transport 依赖它们，改由 `run_shutdown_stages` 的依赖闸门之后单独 ensure。
    ///
    /// 每个服务只调用一次 `shutdown`（MCP worker 的首个 `Shutdown` 即退出）；job 与 completion
    /// 常驻于 runtime 槽位，阶段只 join 同一共享完成句柄。预算耗尽或超时只放弃等待，owner 与
    /// 句柄仍被 runtime 保留；严格重试再次 join 同一 job，不重启 stop。
    async fn ensure_service_stops(&self) -> ServiceStops {
        let slots = &self.service_stops;
        // MCP 与 LSP 是彼此独立的远端服务，stop 并发发起：各自的 owned 关闭 job 在同一时刻
        // spawn，而不是等前一个服务的 stop 启动后才轮到下一个。每个服务仍只启动一次（常驻槽位
        // 保证幂等）。SSH transport 不是与它们独立的资源：它承载远端 MCP/LSP/工具的通信，必须在
        // 这些依赖的停止请求与结果处理之后才回收，因此在依赖闸门之后单独 ensure（见
        // `run_shutdown_stages`）。
        let mcp = self.external_runtimes.mcp.clone();
        let lsp = self.external_runtimes.lsp.clone();
        let (mcp, lsp) = futures::join!(
            ensure_service_stop(&slots.mcp, move || {
                ServiceStopJob::spawn(
                    async move { mcp.shutdown().await.map_err(anyhow::Error::from) },
                )
            }),
            ensure_service_stop(&slots.lsp, move || {
                ServiceStopJob::spawn(
                    async move { lsp.shutdown().await.map_err(anyhow::Error::from) },
                )
            }),
        );
        ServiceStops { mcp, lsp }
    }

    async fn run_shutdown_stages(
        &self,
        budget: ShutdownBudget,
        pending_before: Option<usize>,
        services: &ServiceStops,
    ) -> StageReport {
        let progress = self.shutdown_progress.clone();

        // 取消已在 `execute_shutdown` 中先于任何 join 广播完毕。这里的进度事件只表达"以下独立
        // 阶段都已并发在途"，不是串行契约：全部独立阶段在同一时刻发起、共享同一首次绝对期限的
        // 剩余预算，任何一个阶段的等待都不会消耗其他阶段尚未开始的预算。
        progress.emit(crate::StudioShutdownProgress::StoppingSubscriptions(
            Default::default(),
        ));
        progress.emit(crate::StudioShutdownProgress::CancellingTurns(
            Default::default(),
        ));
        progress.emit(crate::StudioShutdownProgress::StoppingAgents(
            Default::default(),
        ));
        progress.emit(crate::StudioShutdownProgress::StoppingMcp(
            Default::default(),
        ));
        progress.emit(crate::StudioShutdownProgress::StoppingLsp(
            Default::default(),
        ));

        // 写入生产者：它们是可靠保存的前提，其停止必须先于持久化 drain 与 writer 停止——编排在
        // 生产者收束观测结束后才推进持久化（有界退出路径下个别生产者超时也继续，但缺 ACK 会成为
        // issue、不会伪报 Clean）。这些阶段彼此独立，因此并发发起、并发等待（不会用前一个的等待
        // 耗尽后续阶段预算）。
        let mut producers: FuturesUnordered<StageBranch> = FuturesUnordered::new();
        producers.push(self.stage_branch(
            budget,
            stage_order::MODEL_CATALOG,
            "modelCatalog",
            Some(SERVICE_STAGE_LIMIT),
            |runtime| async move { runtime.stop_model_catalog_probes().await },
        ));
        producers.push(self.stage_branch(
            budget,
            stage_order::RECOVERY,
            "recovery",
            Some(SERVICE_STAGE_LIMIT),
            |runtime| async move { runtime.stop_recovery_scan().await },
        ));
        producers.push(self.stage_branch(
            budget,
            stage_order::MODEL_REFRESH,
            "modelRefresh",
            Some(SERVICE_STAGE_LIMIT),
            |runtime| async move { runtime.stop_model_refresh().await },
        ));
        producers.push(self.stage_branch(
            budget,
            stage_order::TITLE,
            "title",
            Some(SERVICE_STAGE_LIMIT),
            |runtime| async move {
                runtime.title_tasks.cancel_and_wait().await;
                Ok::<(), anyhow::Error>(())
            },
        ));
        // Thread/审计收束是可靠的保存前提：严格路径真实等待，退出路径仍受同一期限裁剪。
        producers.push(self.stage_branch(
            budget,
            stage_order::AGENT_FRAMEWORK,
            "agentFramework",
            None,
            |runtime| async move { runtime.shutdown_agent_framework().await },
        ));

        // 远端服务依赖组（独立外部 owner）：与写入收束链（生产者 → 持久化）并发发起、并发等待，
        // 不被持久化 drain 拖后，也不反过来抢占生产者尚未开始的等待预算。这些 owner 直接承载或
        // 观测远端 MCP/LSP/工具通信，SSH transport 必须在它们的停止请求与结果处理**之后**才关闭
        // （见下方 `ssh_branch` 的依赖闸门）；它们彼此独立，因此在同一组内并发发起、并发收束，
        // 而不是串行等待，每个分支都有界（见各自 limit）。
        let mut remote_dependents: FuturesUnordered<StageBranch> = FuturesUnordered::new();
        remote_dependents.push(self.stage_branch(
            budget,
            stage_order::MCP_STARTUP_RECONCILE,
            "mcpStartupReconcile",
            Some(SERVICE_STAGE_LIMIT),
            |runtime| async move { runtime.stop_mcp_startup_reconcile().await },
        ));
        remote_dependents.push(self.stage_branch(
            budget,
            stage_order::MCP_HEALTH,
            "mcpHealth",
            Some(SERVICE_STAGE_LIMIT),
            |runtime| async move { runtime.stop_mcp_health_watcher().await },
        ));
        remote_dependents.push(self.stage_branch(
            budget,
            stage_order::TOOL_REFRESH,
            "toolRefresh",
            Some(SERVICE_STAGE_LIMIT),
            |runtime| async move { runtime.stop_tool_refresh().await },
        ));

        // MCP/LSP 只 join `ensure_service_stops` 已启动的那一次关闭，消费真实 typed 结果；
        // 只有确认回收才发布 `stopped`，失败绝不伪成功。job 常驻 runtime，超时不丢句柄。
        // stop 与随后的 `stopped` 发布是同一服务内的有向顺序，整体仍与其他独立资源并发。
        let mcp_job = services.mcp.clone();
        let mcp_runtime = self.clone();
        remote_dependents.push(
            async move {
                let stop = join_service_outcome(
                    budget,
                    stage_order::MCP_STOP,
                    "mcpStop",
                    mcp_job,
                    Some(SERVICE_STAGE_LIMIT),
                )
                .await;
                let acked = stop.is_done();
                let mut outcomes = vec![stop];
                if acked {
                    outcomes.push(
                        run_stage_owned(
                            mcp_runtime,
                            budget,
                            stage_order::MCP_STOPPED,
                            "mcpStopped",
                            Some(SERVICE_STAGE_LIMIT),
                            |runtime| async move { runtime.publish_mcp_stopped().await },
                        )
                        .await,
                    );
                }
                outcomes
            }
            .boxed(),
        );
        let lsp_job = services.lsp.clone();
        let lsp_runtime = self.clone();
        remote_dependents.push(
            async move {
                let stop = join_service_outcome(
                    budget,
                    stage_order::LSP_STOP,
                    "lspStop",
                    lsp_job,
                    Some(SERVICE_STAGE_LIMIT),
                )
                .await;
                let acked = stop.is_done();
                let mut outcomes = vec![stop];
                if acked {
                    outcomes.push(
                        run_stage_owned(
                            lsp_runtime,
                            budget,
                            stage_order::LSP_STOPPED,
                            "lspStopped",
                            Some(SERVICE_STAGE_LIMIT),
                            |runtime| async move { runtime.external_runtimes.lsp_state.stopped().await },
                        )
                        .await,
                    );
                }
                outcomes
            }
            .boxed(),
        );

        // 与 SSH transport 无依赖的本地资源：本地 LSP 状态 watcher 与持久化 observer。它们不被
        // SSH 依赖闸门等待，只与写入链、远端依赖组并发收束（各自有界），不拖后 SSH 回收。
        let mut independent: FuturesUnordered<StageBranch> = FuturesUnordered::new();
        independent.push(self.stage_branch(
            budget,
            stage_order::LSP_STATE,
            "lspState",
            Some(SERVICE_STAGE_LIMIT),
            |runtime| async move { runtime.stop_lsp_state_watcher().await },
        ));
        independent.push(self.stage_branch(
            budget,
            stage_order::PERSISTENCE_OBSERVER,
            "persistenceObserver",
            Some(SERVICE_STAGE_LIMIT),
            |runtime| async move {
                super::super::background_task::stop(&runtime.persistence_observer)
                    .await
                    .map_err(|error| anyhow::anyhow!(error.to_string()))
            },
        ));

        // 写入生产者依赖的共享完成信号：写入链在生产者收束观测结束后继续持久化；SSH 依赖同一份
        // 生产者事实——远端工具由 Turn / Agent 等生产者驱动，它们未收束时不得先切断 SSH 通信——
        // 但 SSH 只等这条依赖边，不等与它无关的持久化 / 数据库提交。信号载荷刻意区分两件事实：
        // 收到信号 = 依赖已结束观测（含失败 / 有界超时 / 预算耗尽），载荷 `bool` = 实际是否全部
        // 成功 ACK。信号不携带 outcome，避免复制不可 Clone 的诊断，也不引入第二份事实源：写入链
        // 仍是生产者 outcome 的唯一持有与记录方。
        let (producers_settled_tx, producers_settled_rx) = tokio::sync::oneshot::channel::<bool>();

        // SSH transport 关闭的依赖闸门：并发等待全部远端依赖（MCP/LSP stop 链、工具刷新、
        // MCP watcher）的停止请求与结果处理，以及写入生产者的收束观测，之后才在常驻槽位请求
        // SSH 回收；仍只 join 已启动的那一次 owned task，真实结果不被 `let _ =` 吞掉。依赖等待与
        // 真正的 SSH stop 分别计时：依赖等待只做闸门，SSH stop 的耗时由 `join_service_outcome` 的
        // `elapsed` 单独给出。依赖超时 / 失败 / 预算耗尽都不拦下回收请求（有界降级）：
        // `ensure_service_stop` 照常调用，只是 `join_service_outcome` 可能在预算耗尽时返回
        // `Skipped`，owner 仍由常驻槽位保留、可重试。闸门只记录事实，不据此伪报成功。
        let ssh_runtime = self.clone();
        let ssh_branch = async move {
            let dependents_started = Instant::now();
            let (mut outcomes, producers_signal) = futures::join!(
                drain_stage_branches(&mut remote_dependents),
                producers_settled_rx,
            );
            let dependents_elapsed_ms = dependents_started.elapsed().as_millis() as u64;
            // 区分「依赖已结束观测」与「实际全部成功 ACK」：前者只要信号送达即成立，后者只由载荷
            // `true` 表达。任一为假都只作为日志事实，既不放行伪成功，也不阻止照样请求 SSH 回收。
            let producers_observed = producers_signal.is_ok();
            let producers_acked = matches!(&producers_signal, Ok(true));
            tracing::info!(
                pid = std::process::id(),
                stage = "sshTransport",
                resource = "service",
                dependents_settled = outcomes.len(),
                dependents_elapsed_ms,
                producers_observed,
                producers_acked,
                "Studio shutdown SSH dependency gate released; requesting transport recycle"
            );
            let ssh_job = ensure_service_stop(&ssh_runtime.service_stops.ssh, {
                let ssh = ssh_runtime.ssh_manager.clone();
                move || {
                    ServiceStopJob::spawn(async move {
                        ssh.shutdown().await.map_err(anyhow::Error::from)
                    })
                }
            })
            .await;
            outcomes.push(
                join_service_outcome(
                    budget,
                    stage_order::SSH,
                    "ssh",
                    ssh_job,
                    Some(SERVICE_STAGE_LIMIT),
                )
                .await,
            );
            outcomes
        };

        // 写入收束链：生产者停止观测先于可靠持久化 drain/writer 停止（有向顺序，不可颠倒）；
        // 真正的停止 ACK 由各生产者 outcome 决定并在收尾判定，缺 ACK 不会伪报 Clean。整条链与上面
        // 的独立资源并发进行——独立服务不会被持久化 drain 的等待拖后。
        let write_chain = {
            let progress = progress.clone();
            let runtime = self.clone();
            async move {
                // 先并发收束全部写入生产者；无论成功、失败还是有界超时，都在它们的收束观测结束后才
                // 推进持久化 drain/writer——持久化不早于生产者停止观测，但也不会因个别生产者不可用而
                // 永久阻塞其余安全清理（缺 ACK 会作为 issue 阻止最终 Clean，不在此处伪成功）。
                let mut outcomes = drain_stage_branches(&mut producers).await;
                // 放行 SSH 依赖闸门并如实携带「实际是否全部成功 ACK」：写入链仍是 outcome 唯一记录方，
                // 这里只传事实、不复制诊断。成功与有界失败都照样请求 SSH 回收。
                let producers_acked = outcomes.iter().all(StageOutcome::is_done);
                let _ = producers_settled_tx.send(producers_acked);
                let agent_ok = outcomes
                    .iter()
                    .any(|outcome| outcome.stage == "agentFramework" && outcome.is_done());
                // 只在真实已知时发布 pending count；未知不伪造 0。
                if let Some(pending_before) = pending_before {
                    progress.emit(crate::StudioShutdownProgress::FlushingPersistence(
                        crate::FlushingPersistenceProgress::new(pending_before as u64),
                    ));
                }
                // 可靠保存在严格路径真实等待（大量历史 drain 不是失败）；退出路径共享同一期限。
                let persistence = run_stage_owned(
                    runtime.clone(),
                    budget,
                    stage_order::PERSISTENCE,
                    "persistence",
                    None,
                    |runtime| async move { runtime.flush_persistence().await },
                )
                .await;
                let flush_ok = persistence.is_done();
                outcomes.push(persistence);
                // 观测真实剩余待保存事实；有界只读，未知即 Unknown，绝不用 0 冒充。
                let pending_after = runtime.observe_pending(budget).await;
                (outcomes, agent_ok, flush_ok, pending_after)
            }
        };

        // 写入链、非 SSH 依赖的本地资源、以及依赖闸门后的 SSH 回收三者同时推进：SSH 内只等它的
        // 两条依赖边（`remote_dependents` 的远端服务/工具停止与写入生产者停止 ACK），不等与它
        // 无关的持久化 / 数据库提交——因此保存锁阻塞时 MCP/LSP、远端依赖与 SSH transport 仍能
        // 各自及时收束。
        let independent_branch = async move { drain_stage_branches(&mut independent).await };
        let (
            (write_outcomes, agent_ok, flush_ok, pending_after),
            mut independent_outcomes,
            mut ssh_outcomes,
        ) = futures::join!(write_chain, independent_branch, ssh_branch);

        // 汇总所有阶段的真实结果：按稳定权重排序，报告顺序不随完成先后抖动。
        let mut outcomes = write_outcomes;
        outcomes.append(&mut independent_outcomes);
        outcomes.append(&mut ssh_outcomes);
        outcomes.sort_by_key(|outcome| outcome.order);

        let persistence = match (agent_ok, flush_ok, pending_after) {
            (true, true, Some(0)) => {
                progress.emit(crate::StudioShutdownProgress::FlushingPersistence(
                    crate::FlushingPersistenceProgress::new(0),
                ));
                StudioPendingPersistence::Drained
            }
            (_, _, Some(count)) => StudioPendingPersistence::Pending {
                count: count as u64,
            },
            (_, _, None) => StudioPendingPersistence::Unknown,
        };

        StageReport {
            outcomes,
            writer_reclaimed: flush_ok,
            persistence,
        }
    }

    async fn observe_pending(&self, budget: ShutdownBudget) -> Option<usize> {
        let wait = budget.observe_wait(FACTS_LIMIT).unwrap_or(Duration::ZERO);
        tokio::time::timeout(wait, self.pending_persistence_commits())
            .await
            .ok()
    }

    async fn finalize_shutdown(&self, collector: &mut ShutdownCollector) {
        // 仍有运行中的 writer 时不得关闭数据库，也不得释放实例锁。
        let writer_retained = self
            .agent_facility
            .persistence
            .try_lock()
            .map(|slot| slot.is_some())
            .unwrap_or(true);
        if writer_retained {
            let stage_elapsed_ms = collector.elapsed_ms();
            collector.record_error(
                "persistenceOwnerRetained",
                stage_elapsed_ms,
                &anyhow::anyhow!("persistence writer owner is still retained"),
            );
        }
        let drained = matches!(collector.persistence, StudioPendingPersistence::Drained);
        let can_close = !collector.has_issues()
            && drained
            && collector.writer_reclaimed
            && !writer_retained
            && !collector.expired();
        if can_close {
            // store.close 是 owned 操作：作为独立 task 运行，超时只放弃等待，绝不 drop 在途关闭。
            let store = self.store.clone();
            let mut task = tokio::spawn(async move { store.close().await });
            match collector.budget.stage_policy(None) {
                StagePolicy::Bounded(wait) => match tokio::time::timeout(wait, &mut task).await {
                    Ok(Ok(Ok(()))) => collector.store_closed = true,
                    Ok(Ok(Err(error))) => {
                        let stage_elapsed_ms = collector.elapsed_ms();
                        collector.record_error("store", stage_elapsed_ms, &error);
                    }
                    Ok(Err(join_error)) => {
                        let stage_elapsed_ms = collector.elapsed_ms();
                        collector.record_join_error("store", stage_elapsed_ms, &join_error);
                    }
                    Err(_) => {
                        let stage_elapsed_ms = collector.elapsed_ms();
                        collector.record_timeout("store", stage_elapsed_ms);
                    }
                },
                StagePolicy::Unbounded => match task.await {
                    Ok(Ok(())) => collector.store_closed = true,
                    Ok(Err(error)) => {
                        let stage_elapsed_ms = collector.elapsed_ms();
                        collector.record_error("store", stage_elapsed_ms, &error);
                    }
                    Err(join_error) => {
                        let stage_elapsed_ms = collector.elapsed_ms();
                        collector.record_join_error("store", stage_elapsed_ms, &join_error);
                    }
                },
                StagePolicy::Skipped => {
                    let stage_elapsed_ms = collector.elapsed_ms();
                    collector.record_skipped("store", stage_elapsed_ms);
                }
            }
        }

        let clean = !collector.has_issues()
            && matches!(collector.persistence, StudioPendingPersistence::Drained)
            && collector.store_closed;
        if clean {
            if let Err(error) = self
                .runtime_state
                .apply(StudioRuntimeCommand::FinishShutdown {
                    expected_revision: self.runtime_state.snapshot().revision,
                    at: unix_seconds(),
                })
            {
                // FinishShutdown 失败不可吞：退回 Degraded，不发布 Stopped、不释放锁。
                let stage_elapsed_ms = collector.elapsed_ms();
                collector.record_error("finishShutdown", stage_elapsed_ms, &error.into());
            } else {
                self.shutdown_progress
                    .emit(crate::StudioShutdownProgress::Stopped(Default::default()));
                self.instance_lock.release();
                return;
            }
        }

        // Degraded：不发布 Stopped，也不释放实例锁；进程退出由 OS 语义兜底。
        let message = collector
            .issues
            .first()
            .map(|issue| format!("{}: {}", issue.stage, issue.code))
            .unwrap_or_else(|| "shutdown did not reach a clean state".to_string());
        if let Err(error) = self
            .runtime_state
            .apply(StudioRuntimeCommand::FailShutdown {
                expected_revision: self.runtime_state.snapshot().revision,
                at: unix_seconds(),
                error: pl_protocol::StateError {
                    code: "studioShutdownDegraded".to_string(),
                    message,
                    retryable: true,
                },
            })
        {
            let stage_elapsed_ms = collector.elapsed_ms();
            collector.record_error("failShutdown", stage_elapsed_ms, &error.into());
        }
    }

    /// 构造一个阶段的并发分支。
    ///
    /// 分支只返回 `StageOutcome`，不触碰 `ShutdownCollector`，因此多个阶段可以真正并发发起与
    /// 并发等待，而不会被共享聚合器串行化；其结果由 `run_shutdown_stages` 统一按稳定权重聚合。
    /// `limit = None` 表示该阶段在严格路径下必须真实收束（不制造本地墙钟超时）。
    fn stage_branch<Fut>(
        &self,
        budget: ShutdownBudget,
        order: usize,
        stage: &'static str,
        limit: Option<Duration>,
        // 同 `run_stage_owned`：factory 跨 await 持有，需 `Send` 才能让返回的 `BoxFuture` 满足其
        // 跨线程 `Send` 不变量；不做 `unsafe`/`allow`/本地 boxed 规避。
        make: impl FnOnce(StudioRuntime) -> Fut + Send + 'static,
    ) -> StageBranch
    where
        Fut: Future<Output = Result<()>> + Send + 'static,
    {
        run_stage_owned(self.clone(), budget, order, stage, limit, make)
            .map(|outcome| vec![outcome])
            .boxed()
    }
}

/// 把一个编排前的错误（如早期封闭的状态推进失败）投影为脱敏的关闭诊断。
fn shutdown_issue_from_error(stage: &str, error: &anyhow::Error) -> StudioShutdownIssue {
    let mapped = crate::error_mapping::studio_error_from_anyhow_ref(error);
    StudioShutdownIssue {
        stage: stage.to_string(),
        code: mapped.code.label().to_string(),
        message: mapped.message,
        retryable: mapped.retryable,
        correlation_id: mapped.correlation_id,
    }
}

fn shutdown_report_error(report: &StudioShutdownReport) -> anyhow::Error {
    let issue = report.issues.first();
    let message = issue
        .map(|issue| {
            let stage = issue.stage.as_str();
            let code = issue.code.as_str();
            let correlation_id = issue.correlation_id.as_str();
            format!(
                "Studio runtime shutdown did not complete cleanly ({stage}: {code}, correlation {correlation_id})"
            )
        })
        .unwrap_or_else(|| {
            let persistence = report.persistence;
            format!(
                "Studio runtime shutdown did not complete cleanly (persistence: {persistence:?})"
            )
        });
    anyhow::anyhow!(message)
}
