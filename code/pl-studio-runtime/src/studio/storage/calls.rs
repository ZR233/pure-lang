//! 全局模型/工具调用事实库（`calls.sqlite`）。
//!
//! 本库是每次实际 attempt 的独立权威诊断与用量来源：调用开始与终态更新共享同一调用身份，
//! 显式重试形成新的 attempt；关联（Thread/Turn/Attempt/retry_of/tool）、状态、时延、token、
//! 价格摘要结构化保存，请求/响应等大正文以内容寻址 calls blob 文件保存，数据库只保留引用。
//!
//! 单库只有一个逻辑 writer：`commit`（attempt effect）与 `record_billing`（计费观察）即使来自
//! 不同调用点，也只能把不可变 mutation 受理进同一条有界队列，由本进程内唯一的后台 task 批量
//! 落库。瞬时错误按有界指数退避自动重试并保留事实；不变量错误显式回给等待方。每次受理分配
//! 单调 `ticket`，`flush_through(ticket)` 只等待调用时固定的目标而不是整个系统空闲。
//!
//! 调用库不参与 Thread 恢复或 Timeline 排序；计费与性能统计从本库的查询/聚合投影读取，不扫描
//! 会话历史（见 design/15 §15.7/§15.8）。
//!
//! 打开时会校验 `calls_meta.schema_version`：低版本数据保全地迁移到当前外置 blobs 格式（补齐
//! 当前列/表、把历史内联正文改写为内容寻址 blob 并回填引用），未来版本显式失败并保留原字节。

use std::collections::{BTreeSet, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use anyhow::{Context, Result, bail, ensure};
use pl_core::model::ModelUsage;
use pl_core::thread::journal::AttemptUpdate;
use pl_core::thread::{AttemptOutcome, ThreadEffectBatch, ToolDelivery, ToolOutcome};
use pl_protocol::{InferenceBillingRecord, RuntimeCostAmount};
use sea_orm::sqlx::sqlite::{SqliteJournalMode, SqliteSynchronous};
use sea_orm::{
    ConnectOptions, ConnectionTrait, Database, DatabaseBackend, DatabaseConnection, QueryResult,
    Statement, TransactionTrait, Value,
};
use sha2::{Digest, Sha256};
use tokio::sync::{Notify, oneshot, watch};

use crate::hash::merge_costs;

pub(crate) const CALLS_SCHEMA_VERSION: i64 = 3;

/// calls 正文的内容寻址文件根目录名（`calls/blobs`）。
const CALLS_BLOBS_DIR_NAME: &str = "blobs";
/// 单批最多应用的 mutation 数。
const MAX_BATCH_MUTATIONS: usize = 64;
/// 单批最多聚合的字节预算。
const MAX_BATCH_BYTES: usize = 4 * 1024 * 1024;
/// 队列条目数软上限；达到后受理方等待 writer 腾出空间。
const MAX_QUEUE_MUTATIONS: usize = 4_096;
/// 队列保留字节软上限。
const MAX_QUEUE_BYTES: usize = 64 * 1024 * 1024;
/// 进入有界指数退避前允许的快速重试次数。
const FAST_RETRIES: usize = 3;
/// 瞬时失败的重试退避基值。
const RETRY_BACKOFF: Duration = Duration::from_millis(100);
/// 退化后的最大自动重试间隔。
const MAX_RETRY_BACKOFF: Duration = Duration::from_secs(30);
/// 受限读取允许返回的最大正文长度。
pub(crate) const MAX_CALL_BODY_READ: usize = 8 * 1024 * 1024;

/// 调用结果类别；持久化为稳定字符串。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CallStatus {
    Running,
    Committed,
    Rejected,
    Cancelled,
    Failed,
    Interrupted,
    Completed,
}

impl CallStatus {
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::Running => "running",
            Self::Committed => "committed",
            Self::Rejected => "rejected",
            Self::Cancelled => "cancelled",
            Self::Failed => "failed",
            Self::Interrupted => "interrupted",
            Self::Completed => "completed",
        }
    }

    pub(crate) const fn is_terminal(self) -> bool {
        !matches!(self, Self::Running)
    }

    /// Parses one persisted status string; unknown text is rejected instead of guessing a state.
    ///
    /// A reader must never present an unrecognized durable status as a finished result.
    pub(crate) fn parse(value: &str) -> Option<Self> {
        Some(match value {
            "running" => Self::Running,
            "committed" => Self::Committed,
            "rejected" => Self::Rejected,
            "cancelled" => Self::Cancelled,
            "failed" => Self::Failed,
            "interrupted" => Self::Interrupted,
            "completed" => Self::Completed,
            _ => return None,
        })
    }
}

/// 一次已受理工具调用的 durable 身份与状态（不含正文）。
///
/// core 的有界任务账本和 effect 窗口淘汰后，重复的只读任务查询用它回答“这个身份究竟是什么
/// 状态”；`terminal` 为假表示交付尚未 durable，调用方不得把它当作完成态。
#[derive(Debug, Clone)]
pub(crate) struct DurableToolCall {
    pub(crate) call_id: String,
    pub(crate) turn_id: String,
    pub(crate) tool_id: String,
    pub(crate) revision: u64,
    pub(crate) status: CallStatus,
    pub(crate) terminal: bool,
    /// Whether the durable task lifecycle already recorded a cancellation request.
    pub(crate) cancel_requested: bool,
}

/// 写入 retention：区分主循环调用与内部/辅助推理。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CallRetention {
    Turn,
    Internal,
}

impl CallRetention {
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::Turn => "turn",
            Self::Internal => "internal",
        }
    }
}

/// 结构化调用事实，供性能与计费投影按 root 会话或 Thread 读取。
#[allow(dead_code)]
#[derive(Debug, Clone, Default)]
pub(crate) struct ModelCallFact {
    /// Effect sequence that admitted this call; the durable recovery cursor.
    pub(crate) revision: u64,
    pub(crate) thread_id: String,
    pub(crate) root_thread_id: Option<String>,
    pub(crate) call_id: String,
    pub(crate) turn_id: String,
    pub(crate) retry_of: Option<String>,
    pub(crate) status: String,
    pub(crate) terminal: bool,
    pub(crate) retention: Option<String>,
    pub(crate) purpose: Option<String>,
    pub(crate) provider_instance_id: Option<String>,
    pub(crate) provider_display_name: Option<String>,
    pub(crate) configured_model: Option<String>,
    pub(crate) sent_model: Option<String>,
    pub(crate) reported_model: Option<String>,
    pub(crate) reasoning_effort: Option<String>,
    pub(crate) input_tokens: Option<u64>,
    pub(crate) output_tokens: Option<u64>,
    pub(crate) cache_read_tokens: Option<u64>,
    pub(crate) cache_write_tokens: Option<u64>,
    pub(crate) reasoning_tokens: Option<u64>,
    pub(crate) total_tokens: Option<u64>,
    pub(crate) ttft_millis: Option<u64>,
    pub(crate) decode_millis: Option<u64>,
    pub(crate) response_millis: Option<u64>,
    pub(crate) cost_currency: Option<String>,
    pub(crate) cost_amount: Option<f64>,
    pub(crate) has_unpriced_usage: bool,
    pub(crate) started_at: i64,
    pub(crate) finished_at: Option<i64>,
}

/// 一次计费/性能事实的幂等写入结果。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum BillingWrite {
    Inserted,
    Identical,
}

/// 单个 root 会话的费用聚合投影（由调用库 SQL 聚合得到）。
#[derive(Debug, Clone, Default)]
pub(crate) struct SessionCostRollup {
    pub(crate) root_thread_id: String,
    pub(crate) estimated_costs: Vec<RuntimeCostAmount>,
    pub(crate) purpose_costs: Vec<PurposeCostRollup>,
    pub(crate) has_unpriced_usage: bool,
}

/// 单个 purpose 的费用聚合投影。
#[derive(Debug, Clone, Default)]
pub(crate) struct PurposeCostRollup {
    pub(crate) purpose: Option<String>,
    pub(crate) estimated_costs: Vec<RuntimeCostAmount>,
    pub(crate) has_unpriced_usage: bool,
}

/// 一条性能历史样本；只有形成正 decode 时长的已终态调用才产生样本。
#[derive(Debug, Clone)]
pub(crate) struct PerformanceSampleRow {
    pub(crate) completed_at: i64,
    pub(crate) provider_instance_id: String,
    pub(crate) provider_display_name: String,
    pub(crate) configured_model: Option<String>,
    pub(crate) sent_model: String,
    pub(crate) reported_model: Option<String>,
    pub(crate) reasoning_effort: Option<String>,
    pub(crate) completion_tokens: u64,
    pub(crate) ttft_millis: u64,
    pub(crate) decode_millis: u64,
    pub(crate) response_millis: u64,
}

/// 按（provider instance、实际发送模型、reasoning effort）的数据库聚合分组。
#[derive(Debug, Clone)]
pub(crate) struct PerformanceSummaryRow {
    pub(crate) provider_instance_id: String,
    pub(crate) provider_display_name: String,
    pub(crate) sent_model: String,
    pub(crate) reasoning_effort: Option<String>,
    pub(crate) sample_count: u64,
    pub(crate) completion_tokens: u64,
    pub(crate) total_ttft_millis: u64,
    pub(crate) total_decode_millis: u64,
    pub(crate) total_response_millis: u64,
}

/// 受理一次调用写入的不可变工作单元。
enum CallMutation {
    Effect(Box<ThreadEffectBatch>),
    Billing {
        root_thread_id: String,
        thread_id: String,
        retention: CallRetention,
        billing: Box<InferenceBillingRecord>,
    },
}

impl CallMutation {
    /// 估算条目保留字节，用于队列与批次的压力预算。
    fn estimated_bytes(&self) -> usize {
        match self {
            Self::Effect(effect) => effect
                .encode()
                .map_or(1024, |payload| payload.content().len()),
            Self::Billing { billing, .. } => {
                serde_json::to_vec(billing.as_ref()).map_or(512, |bytes| bytes.len())
            }
        }
    }
}

/// 一次受理的完成信号；等待方通过它拿到已提交或已拒绝的显式结果。
enum CallCompletion {
    Effect(oneshot::Sender<Result<()>>),
    Billing(oneshot::Sender<Result<BillingWrite>>),
}

/// 单个 mutation 的应用结果。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MutationOutcome {
    Effect,
    Billing(BillingWrite),
}

struct QueuedCallMutation {
    ticket: u64,
    accepted_at: tokio::time::Instant,
    bytes: usize,
    mutation: CallMutation,
    completion: Option<CallCompletion>,
}

/// 不可重试的批量失败；保留消息用于错误上报与诊断。
#[derive(Debug)]
struct BatchFailure {
    message: String,
    retryable: bool,
}

#[derive(Debug, Clone, Copy, Default)]
struct QueuePressure {
    mutations: usize,
    bytes: usize,
    oldest_accepted_at: Option<tokio::time::Instant>,
}

/// 单一逻辑 writer 的共享状态：唯一 SQLite 连接、唯一队列与水位。
struct CallsWriter {
    db: DatabaseConnection,
    blobs_dir: PathBuf,
    queue: Mutex<VecDeque<QueuedCallMutation>>,
    work_notify: Notify,
    retry_notify: Notify,
    space_notify: Notify,
    admitted_ticket: AtomicU64,
    durable_ticket: watch::Sender<u64>,
    /// Encoded bytes of the batch currently being written; a read-only pressure observation.
    in_flight_bytes: AtomicU64,
    stopping: AtomicBool,
    last_error: Mutex<Option<String>>,
}

/// 全局调用库句柄；clone 共享同一条队列、水位与后台 writer。
#[derive(Clone)]
pub(crate) struct CallsStore {
    writer: Arc<CallsWriter>,
}

impl CallsStore {
    pub(crate) async fn open(path: &Path) -> Result<Self> {
        if let Some(parent) = path.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }
        let blobs_dir = path
            .parent()
            .unwrap_or_else(|| Path::new("."))
            .join(CALLS_BLOBS_DIR_NAME);
        tokio::fs::create_dir_all(&blobs_dir).await?;
        let mut options = ConnectOptions::new(crate::studio::paths::sqlite_url(path));
        options
            .max_connections(1)
            .min_connections(1)
            .connect_timeout(Duration::from_secs(8))
            .acquire_timeout(Duration::from_secs(8))
            .map_sqlx_sqlite_opts(|options| {
                options
                    .journal_mode(SqliteJournalMode::Wal)
                    .synchronous(SqliteSynchronous::Full)
                    .busy_timeout(Duration::from_secs(5))
                    .foreign_keys(true)
            })
            .sqlx_logging(false);
        let db = Database::connect(options).await?;
        initialize(&db, &blobs_dir).await?;
        let (durable_ticket, _) = watch::channel(0u64);
        let writer = Arc::new(CallsWriter {
            db,
            blobs_dir,
            queue: Mutex::new(VecDeque::new()),
            work_notify: Notify::new(),
            retry_notify: Notify::new(),
            space_notify: Notify::new(),
            admitted_ticket: AtomicU64::new(0),
            durable_ticket,
            in_flight_bytes: AtomicU64::new(0),
            stopping: AtomicBool::new(false),
            last_error: Mutex::new(None),
        });
        tokio::spawn(supervise_writer(writer.clone()));
        Ok(Self { writer })
    }

    /// 当前已受理的最高 ticket；`flush_through` 的目标上界。
    pub(crate) fn admitted_ticket(&self) -> u64 {
        self.writer.admitted_ticket.load(Ordering::Acquire)
    }

    /// 当前尚未落库的 mutation 数；关机进度与压力观测使用。
    pub(crate) fn pending_count(&self) -> usize {
        self.writer
            .queue
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .len()
    }

    /// 最近一次调用写入错误（瞬时重试与显式拒绝都会记录，直到一次成功写入清除）。
    pub(crate) fn last_error(&self) -> Option<String> {
        self.writer
            .last_error
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .clone()
    }

    /// 正在写入的一批 mutation 的编码字节数；只读观测，不参与写路径。
    pub(crate) fn in_flight_bytes(&self) -> usize {
        usize::try_from(self.writer.in_flight_bytes.load(Ordering::Acquire)).unwrap_or(usize::MAX)
    }

    /// 调用库是否因队列压力停止接受新的计费/调用事实。
    ///
    /// 门槛与 `admit_billing` 的拒绝条件一致，因此这是"下一步受理会不会被拒绝"的真实观测，
    /// 不是占位值。
    pub(crate) fn pressure_paused(&self) -> bool {
        let queue = self
            .writer
            .queue
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        queue.len() >= MAX_QUEUE_MUTATIONS || queue_bytes(&queue) >= MAX_QUEUE_BYTES
    }

    /// 当前已落库的最高 ticket，作为调用库 durable sequence 的观测值。
    pub(crate) fn durable_ticket(&self) -> u64 {
        *self.writer.durable_ticket.borrow()
    }

    /// 排队 mutation 的编码字节数合计，作为调用库 pending bytes 的观测值。
    pub(crate) fn pending_bytes(&self) -> usize {
        let queue = self
            .writer
            .queue
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        queue_bytes(&queue)
    }

    /// 最老排队 mutation 的等待时长；无排队时为 `None`。
    pub(crate) fn oldest_pending_age(&self) -> Option<std::time::Duration> {
        let queue = self
            .writer
            .queue
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        queue.front().map(|entry| entry.accepted_at.elapsed())
    }

    /// 把一次已提交 effect 的模型/工具调用事实写入调用库。
    ///
    /// 调用开始（Running）与终态更新共享同一 `(thread_id, call_id)` 身份；显式重试由
    /// `retry_of` 关联新 attempt；较早的观察不能覆盖已写入的终态。返回时该事实已 durable。
    pub(crate) async fn commit(&self, effect: &ThreadEffectBatch) -> Result<()> {
        let (sender, receiver) = oneshot::channel();
        self.admit(
            CallMutation::Effect(Box::new(effect.clone())),
            CallCompletion::Effect(sender),
        )
        .await?;
        receiver
            .await
            .map_err(|_| anyhow::anyhow!("call writer dropped the admitted effect"))??;
        Ok(())
    }

    /// 幂等写入一次模型调用计费/性能事实。
    ///
    /// 调用身份为 `(thread_id, billing.inference_id)`；同一身份同一正文幂等，不同正文明确失败。
    /// 已写入的 `billing_ref` 不被不同观察覆盖。
    #[allow(dead_code)]
    pub(crate) async fn record_billing(
        &self,
        root_thread_id: &str,
        thread_id: &str,
        billing: &InferenceBillingRecord,
        retention: CallRetention,
    ) -> Result<BillingWrite> {
        let (sender, receiver) = oneshot::channel();
        self.admit(
            CallMutation::Billing {
                root_thread_id: root_thread_id.to_owned(),
                thread_id: thread_id.to_owned(),
                retention,
                billing: Box::new(billing.clone()),
            },
            CallCompletion::Billing(sender),
        )
        .await?;
        // The completion channel for a billing admission already carries `Result<BillingWrite>`: the
        // writer rejects a mismatched outcome itself, so the caller just propagates the explicit result.
        receiver
            .await
            .map_err(|_| anyhow::anyhow!("call writer dropped the admitted billing record"))?
    }

    /// 同步受理一次计费观察并把其 ticket 交回调用方。
    ///
    /// 受理只入队、不等待 durability：计费观察来自同步投影路径，durability 由调用方在
    /// Thread 关闭/Studio shutdown 时用固定的 ticket 等待（`flush_through`）。队列满或写入器
    /// 停机时显式失败，让调用方暂停新的有费用执行，而不是静默丢弃事实。
    pub(crate) fn admit_billing(
        &self,
        root_thread_id: &str,
        thread_id: &str,
        billing: &InferenceBillingRecord,
        retention: CallRetention,
    ) -> Result<u64> {
        if self.writer.stopping.load(Ordering::Acquire) {
            bail!("call writer is stopping");
        }
        let mutation = CallMutation::Billing {
            root_thread_id: root_thread_id.to_owned(),
            thread_id: thread_id.to_owned(),
            retention,
            billing: Box::new(billing.clone()),
        };
        let bytes = mutation.estimated_bytes();
        let ticket = {
            let mut queue = self
                .writer
                .queue
                .lock()
                .unwrap_or_else(|error| error.into_inner());
            ensure!(
                queue.len() < MAX_QUEUE_MUTATIONS
                    && queue_bytes(&queue).saturating_add(bytes) <= MAX_QUEUE_BYTES,
                "call writer queue is full"
            );
            let ticket = self.next_ticket();
            queue.push_back(QueuedCallMutation {
                ticket,
                accepted_at: tokio::time::Instant::now(),
                bytes,
                mutation,
                completion: None,
            });
            ticket
        };
        self.writer.work_notify.notify_one();
        Ok(ticket)
    }

    /// 等待当前已受理的全部 mutation 落库；只使用受理时固定的 ticket。
    pub(crate) async fn flush(&self) -> Result<()> {
        self.flush_through(self.admitted_ticket()).await
    }

    /// 只等待调用时固定的目标 ticket 变为 durable，不等待整个系统空闲。
    pub(crate) async fn flush_through(&self, ticket: u64) -> Result<()> {
        if ticket == 0 || *self.writer.durable_ticket.borrow() >= ticket {
            return Ok(());
        }
        let mut progress = self.writer.durable_ticket.subscribe();
        loop {
            if *progress.borrow_and_update() >= ticket {
                return Ok(());
            }
            if self.writer.stopping.load(Ordering::Acquire) && queue_is_empty(&self.writer) {
                bail!("call writer stopped before ticket {ticket}");
            }
            progress
                .changed()
                .await
                .map_err(|_| anyhow::anyhow!("call writer durability channel closed"))?;
        }
    }

    /// 排空队列并停止受理；保存失败保留事实并返回错误。
    pub(crate) async fn shutdown(&self) -> Result<()> {
        self.writer.stopping.store(true, Ordering::Release);
        self.writer.work_notify.notify_one();
        self.writer.retry_notify.notify_one();
        let target = self.admitted_ticket();
        let mut progress = self.writer.durable_ticket.subscribe();
        loop {
            let durable = *progress.borrow_and_update();
            if durable >= target {
                return Ok(());
            }
            if queue_is_empty(&self.writer) {
                bail!(
                    "call writer stopped with {} pending mutations",
                    target.saturating_sub(durable)
                );
            }
            progress
                .changed()
                .await
                .map_err(|_| anyhow::anyhow!("call writer durability channel closed"))?;
        }
    }

    /// 受理一次 mutation：队列有界，满时等待 writer 腾出空间；返回时只保证已入队。
    async fn admit(&self, mutation: CallMutation, completion: CallCompletion) -> Result<()> {
        let mut pending = Some((mutation, completion));
        loop {
            let notified = self.writer.space_notify.notified();
            let bytes = {
                let (mutation, _) = pending.as_ref().expect("pending admission");
                mutation.estimated_bytes()
            };
            let admitted = {
                let mut queue = self
                    .writer
                    .queue
                    .lock()
                    .unwrap_or_else(|error| error.into_inner());
                if queue.len() < MAX_QUEUE_MUTATIONS
                    && queue_bytes(&queue).saturating_add(bytes) <= MAX_QUEUE_BYTES
                {
                    let ticket = self.next_ticket();
                    let (mutation, completion) = pending.take().expect("pending admission");
                    queue.push_back(QueuedCallMutation {
                        ticket,
                        accepted_at: tokio::time::Instant::now(),
                        bytes,
                        mutation,
                        completion: Some(completion),
                    });
                    true
                } else {
                    false
                }
            };
            if admitted {
                self.writer.work_notify.notify_one();
                return Ok(());
            }
            if self.writer.stopping.load(Ordering::Acquire) {
                bail!("call writer is stopping");
            }
            notified.await;
        }
    }

    fn next_ticket(&self) -> u64 {
        self.writer
            .admitted_ticket
            .fetch_add(1, Ordering::AcqRel)
            .wrapping_add(1)
    }

    /// 读取 root 会话的模型调用事实，供计费/性能投影按 root 聚合。
    #[allow(dead_code)]
    pub(crate) async fn session_model_calls(
        &self,
        root_thread_id: &str,
        limit: u32,
    ) -> Result<Vec<ModelCallFact>> {
        let rows = self
            .writer
            .db
            .query_all_raw(statement(
                "SELECT * FROM model_calls WHERE root_thread_id=?
                 ORDER BY finished_at DESC, started_at DESC LIMIT ?",
                vec![root_thread_id.into(), i64::from(limit).into()],
            ))
            .await?;
        rows.iter().map(model_fact).collect()
    }

    /// 读取单个 Thread 的模型调用诊断，顺序为调用开始时间。
    #[allow(dead_code)]
    pub(crate) async fn thread_model_calls(&self, thread_id: &str) -> Result<Vec<ModelCallFact>> {
        let rows = self
            .writer
            .db
            .query_all_raw(statement(
                "SELECT * FROM model_calls WHERE thread_id=? ORDER BY started_at, call_id",
                vec![thread_id.into()],
            ))
            .await?;
        rows.iter().map(model_fact).collect()
    }

    /// 有界读取一个 Thread 在给定 effect 序号区间内已 durable 的模型调用事实。
    ///
    /// 产品观察在 effect 窗口缺口后用它恢复尚未计费的调用事实：已有 `billing_ref`
    /// 保存了完整计费正文，摘要列无法无损重建它，重复提交会触发身份冲突。按 revision
    /// 游标分页，根 Thread 与子 Thread 走同一条只读路径。
    pub(crate) async fn model_call_facts_between(
        &self,
        thread_id: &str,
        after_revision: u64,
        through_revision: u64,
        limit: usize,
    ) -> Result<Vec<ModelCallFact>> {
        if through_revision < after_revision {
            return Ok(Vec::new());
        }
        let rows = self
            .writer
            .db
            .query_all_raw(statement(
                "SELECT * FROM model_calls
                 WHERE thread_id=? AND revision > ? AND revision <= ? AND billing_ref IS NULL
                 ORDER BY revision ASC, call_id ASC LIMIT ?",
                vec![
                    thread_id.to_owned().into(),
                    i64::try_from(after_revision)?.into(),
                    i64::try_from(through_revision)?.into(),
                    i64::try_from(limit.clamp(1, 4096))?.into(),
                ],
            ))
            .await?;
        rows.iter().map(model_fact).collect()
    }

    /// 按 root 会话聚合费用；调用库是唯一事实源，不依赖进程内缓存。
    pub(crate) async fn session_cost_rollups(&self) -> Result<Vec<SessionCostRollup>> {
        let mut rollups: Vec<SessionCostRollup> = Vec::new();
        let roots = self
            .writer
            .db
            .query_all_raw(statement(
                "SELECT root_thread_id, MAX(has_unpriced_usage) AS unpriced
                 FROM model_calls
                 WHERE terminal=1 AND root_thread_id IS NOT NULL
                 GROUP BY root_thread_id
                 ORDER BY root_thread_id",
                vec![],
            ))
            .await?;
        for row in &roots {
            rollups.push(SessionCostRollup {
                root_thread_id: row.try_get("", "root_thread_id")?,
                estimated_costs: Vec::new(),
                purpose_costs: Vec::new(),
                has_unpriced_usage: row.try_get::<i64>("", "unpriced")? != 0,
            });
        }
        let costs = self
            .writer
            .db
            .query_all_raw(statement(
                "SELECT root_thread_id, purpose, cost_currency, SUM(cost_amount) AS amount
                 FROM model_calls
                 WHERE terminal=1 AND root_thread_id IS NOT NULL
                   AND cost_currency IS NOT NULL AND cost_amount IS NOT NULL
                 GROUP BY root_thread_id, purpose, cost_currency
                 ORDER BY root_thread_id, purpose IS NOT NULL, purpose, cost_currency",
                vec![],
            ))
            .await?;
        for row in &costs {
            let root_thread_id: String = row.try_get("", "root_thread_id")?;
            let purpose: Option<String> = row.try_get("", "purpose")?;
            let cost = RuntimeCostAmount {
                currency: row.try_get("", "cost_currency")?,
                amount: row.try_get("", "amount")?,
            };
            let index = match rollups
                .iter()
                .position(|rollup| rollup.root_thread_id == root_thread_id)
            {
                Some(index) => index,
                None => {
                    rollups.push(SessionCostRollup {
                        root_thread_id: root_thread_id.clone(),
                        ..Default::default()
                    });
                    rollups.len() - 1
                }
            };
            let rollup = &mut rollups[index];
            merge_costs(&mut rollup.estimated_costs, std::slice::from_ref(&cost));
            let purpose_index = rollup
                .purpose_costs
                .iter()
                .position(|entry| entry.purpose == purpose);
            match purpose_index {
                Some(position) => merge_costs(
                    &mut rollup.purpose_costs[position].estimated_costs,
                    std::slice::from_ref(&cost),
                ),
                None => rollup.purpose_costs.push(PurposeCostRollup {
                    purpose,
                    estimated_costs: vec![cost],
                    has_unpriced_usage: false,
                }),
            }
        }
        for rollup in &mut rollups {
            for entry in &mut rollup.purpose_costs {
                entry.has_unpriced_usage = rollup.has_unpriced_usage;
            }
            if rollup.purpose_costs.is_empty()
                && (!rollup.estimated_costs.is_empty() || rollup.has_unpriced_usage)
            {
                rollup.purpose_costs.push(PurposeCostRollup {
                    purpose: None,
                    estimated_costs: rollup.estimated_costs.clone(),
                    has_unpriced_usage: rollup.has_unpriced_usage,
                });
            }
        }
        Ok(rollups)
    }

    /// 读取最近性能历史样本（新到旧），供产品快照读取。
    pub(crate) async fn recent_performance_samples(
        &self,
        limit: u32,
    ) -> Result<Vec<PerformanceSampleRow>> {
        let rows = self
            .writer
            .db
            .query_all_raw(statement(
                "SELECT COALESCE(finished_at, started_at) AS completed_at,
                        provider_instance_id, provider_display_name, configured_model,
                        sent_model, reported_model, reasoning_effort, output_tokens,
                        ttft_millis, decode_millis, response_millis
                 FROM model_calls
                 WHERE terminal=1 AND provider_instance_id IS NOT NULL
                   AND sent_model IS NOT NULL AND decode_millis IS NOT NULL AND decode_millis > 0
                 ORDER BY finished_at DESC, started_at DESC, call_id DESC
                 LIMIT ?",
                vec![i64::from(limit).into()],
            ))
            .await?;
        rows.iter().map(performance_sample_row).collect()
    }

    /// 按 provider instance/发送模型/effort 聚合性能汇总（数据库聚合投影）。
    pub(crate) async fn performance_summary_rows(&self) -> Result<Vec<PerformanceSummaryRow>> {
        let rows = self
            .writer
            .db
            .query_all_raw(statement(
                "SELECT provider_instance_id,
                        MAX(provider_display_name) AS provider_display_name,
                        sent_model,
                        reasoning_effort,
                        COUNT(*) AS sample_count,
                        COALESCE(SUM(output_tokens), 0) AS completion_tokens,
                        COALESCE(SUM(ttft_millis), 0) AS total_ttft_millis,
                        COALESCE(SUM(decode_millis), 0) AS total_decode_millis,
                        COALESCE(SUM(response_millis), 0) AS total_response_millis
                 FROM model_calls
                 WHERE terminal=1 AND provider_instance_id IS NOT NULL
                   AND sent_model IS NOT NULL AND decode_millis IS NOT NULL AND decode_millis > 0
                 GROUP BY provider_instance_id, sent_model, reasoning_effort
                 ORDER BY provider_instance_id, sent_model,
                          reasoning_effort IS NOT NULL, reasoning_effort",
                vec![],
            ))
            .await?;
        rows.iter().map(performance_summary_row).collect()
    }

    /// 受限读取一次调用正文。
    ///
    /// 引用必须是内容寻址摘要，路径只解析到 calls blob 根目录内的普通文件；超过 `limit`
    /// 字节、非普通文件或摘要不匹配都显式失败，不返回部分正文。
    #[allow(dead_code)]
    pub(crate) async fn read_body(&self, body_ref: &str, limit: usize) -> Result<Option<String>> {
        let name = blob_file_name(body_ref).context("invalid call body reference")?;
        let path = self.writer.blobs_dir.join(name);
        let metadata = match tokio::fs::symlink_metadata(&path).await {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(error.into()),
        };
        ensure!(
            metadata.file_type().is_file(),
            "call body reference is not a regular file"
        );
        let limit = limit.min(MAX_CALL_BODY_READ);
        ensure!(
            metadata.len() <= limit as u64,
            "call body exceeds the permitted read length"
        );
        let bytes = tokio::fs::read(&path).await?;
        ensure!(
            bytes.len() <= limit,
            "call body exceeds the permitted read length"
        );
        ensure!(
            pl_core::context::content_hash(&bytes) == body_ref,
            "call body content does not match its reference"
        );
        Ok(Some(String::from_utf8(bytes)?))
    }

    /// Reads one completed tool delivery by `(thread_id, call_id)` from the global calls store.
    ///
    /// This is the durable authority for a finished tool result after the core's transient effect
    /// window has released it: the terminal `tool_calls` row holds the content-addressed reference
    /// to the full `ToolDelivery`, so the caller gets the exact committed result without copying a
    /// second live history into core and without re-executing the tool.
    ///
    /// A call that is not terminal (still running, rejected or cancelled before delivery) or a
    /// different Thread returns `None`: the caller must report that explicitly instead of treating
    /// it as a completed result or admitting it as new work. Reads are bounded by
    /// [`MAX_CALL_BODY_READ`] and fail closed on a `body_ref` that is not a content-addressed
    /// regular blob.
    pub(crate) async fn tool_delivery(
        &self,
        thread_id: &str,
        call_id: &str,
    ) -> Result<Option<ToolDelivery>> {
        let row = self
            .writer
            .db
            .query_one_raw(statement(
                "SELECT body_ref FROM tool_calls
                 WHERE thread_id=? AND call_id=? AND terminal=1
                 ORDER BY revision DESC LIMIT 1",
                vec![thread_id.to_owned().into(), call_id.to_owned().into()],
            ))
            .await?;
        let Some(row) = row else {
            return Ok(None);
        };
        let Some(reference) = row.try_get::<Option<String>>("", "body_ref")? else {
            return Ok(None);
        };
        let Some(body) = self.read_body(&reference, MAX_CALL_BODY_READ).await? else {
            return Ok(None);
        };
        Ok(Some(serde_json::from_str(&body)?))
    }

    /// Reads one admitted tool call's durable identity and status by its core task identity.
    ///
    /// The argument may be either the core task id (`task:{call_id}`) or the bare call id. This is
    /// a primary-key read of `tool_calls` only: it never loads the result body, so a repeated
    /// status query stays a bounded durable lookup instead of pulling the full result back into
    /// memory. An unknown persisted status is reported as "not durably known" (`None`) rather than
    /// being guessed into a terminal state.
    pub(crate) async fn tool_task(
        &self,
        thread_id: &str,
        task_id: &str,
    ) -> Result<Option<DurableToolCall>> {
        let call_id = task_id.strip_prefix("task:").unwrap_or(task_id);
        let Some(row) = self
            .writer
            .db
            .query_one_raw(statement(
                "SELECT call_id,turn_id,tool_id,revision,status,terminal,cancel_requested
                 FROM tool_calls
                 WHERE thread_id=? AND call_id=? ORDER BY revision DESC LIMIT 1",
                vec![thread_id.to_owned().into(), call_id.to_owned().into()],
            ))
            .await?
        else {
            return Ok(None);
        };
        let status: String = row.try_get("", "status")?;
        let Some(status) = CallStatus::parse(&status) else {
            return Ok(None);
        };
        let revision = u64::try_from(row.try_get::<i64>("", "revision")?)
            .map_err(|_| anyhow::anyhow!("tool call revision is outside the supported range"))?;
        Ok(Some(DurableToolCall {
            call_id: row.try_get("", "call_id")?,
            turn_id: row.try_get("", "turn_id")?,
            tool_id: row.try_get("", "tool_id")?,
            revision,
            status,
            terminal: row.try_get::<i32>("", "terminal")? != 0,
            cancel_requested: row.try_get::<i32>("", "cancel_requested")? != 0,
        }))
    }
}

async fn initialize(db: &DatabaseConnection, blobs_dir: &Path) -> Result<()> {
    ensure_calls_schema(db, blobs_dir).await
}

/// Ensures the current calls schema exists, upgrading any older revision in place.
///
/// It is shared by [`CallsStore::open`] and the one-time migration boundary so both normalize a
/// legacy `calls.sqlite` through exactly the same additive, data-preserving upgrader instead of
/// growing a second format definition. Additive `ALTER` upgrades keep every existing fact; a schema
/// only ever moves forward, and a future revision fails closed with the original bytes preserved.
async fn ensure_calls_schema(db: &DatabaseConnection, blobs_dir: &Path) -> Result<()> {
    db.execute_unprepared(
        "CREATE TABLE IF NOT EXISTS calls_meta (
            id INTEGER PRIMARY KEY CHECK (id = 1),
            schema_version INTEGER NOT NULL,
            database_id TEXT NOT NULL
        );
        CREATE TABLE IF NOT EXISTS call_watermarks (
            thread_id TEXT PRIMARY KEY,
            admitted_write_seq INTEGER NOT NULL,
            durable_write_seq INTEGER NOT NULL
        );
        CREATE TABLE IF NOT EXISTS model_calls (
            thread_id TEXT NOT NULL,
            call_id TEXT NOT NULL,
            root_thread_id TEXT,
            turn_id TEXT NOT NULL,
            attempt_id TEXT NOT NULL,
            retry_of TEXT,
            revision INTEGER NOT NULL,
            admitted_at INTEGER NOT NULL,
            started_at INTEGER NOT NULL,
            finished_at INTEGER,
            status TEXT NOT NULL,
            terminal INTEGER NOT NULL,
            retention TEXT,
            purpose TEXT,
            provider_instance_id TEXT,
            provider_display_name TEXT,
            configured_model TEXT,
            sent_model TEXT,
            reported_model TEXT,
            reasoning_effort TEXT,
            input_tokens INTEGER,
            output_tokens INTEGER,
            cache_read_tokens INTEGER,
            cache_write_tokens INTEGER,
            reasoning_tokens INTEGER,
            total_tokens INTEGER,
            ttft_millis INTEGER,
            decode_millis INTEGER,
            response_millis INTEGER,
            cost_currency TEXT,
            cost_amount REAL,
            has_unpriced_usage INTEGER NOT NULL DEFAULT 0,
            body_ref TEXT,
            billing_ref TEXT,
            PRIMARY KEY(thread_id, call_id)
        );
        CREATE TABLE IF NOT EXISTS tool_calls (
            thread_id TEXT NOT NULL,
            call_id TEXT NOT NULL,
            turn_id TEXT NOT NULL,
            tool_id TEXT NOT NULL,
            revision INTEGER NOT NULL,
            admitted_at INTEGER NOT NULL,
            started_at INTEGER NOT NULL,
            finished_at INTEGER,
            status TEXT NOT NULL,
            terminal INTEGER NOT NULL,
            body_ref TEXT,
            cancel_requested INTEGER NOT NULL DEFAULT 0,
            PRIMARY KEY(thread_id, call_id)
        );
        CREATE TABLE IF NOT EXISTS call_bodies (
            body_ref TEXT PRIMARY KEY,
            byte_length INTEGER NOT NULL,
            created_at INTEGER NOT NULL
        );
        CREATE INDEX IF NOT EXISTS model_calls_by_thread
            ON model_calls(thread_id, started_at);
        CREATE INDEX IF NOT EXISTS model_calls_by_root
            ON model_calls(root_thread_id, finished_at);
        CREATE INDEX IF NOT EXISTS tool_calls_by_thread
            ON tool_calls(thread_id, started_at);",
    )
    .await?;
    // 工具调用任务来源状态列：当前 schema 版本的旧库也必须原地补齐，因此放在无条件执行的
    // 建表之后（重复添加被显式容忍），而不是只挂在按版本运行的迁移里，否则已存在的库会缺列
    // 而让取消回执查询失败。
    if let Err(error) = db
        .execute_unprepared(
            "ALTER TABLE tool_calls ADD COLUMN cancel_requested INTEGER NOT NULL DEFAULT 0",
        )
        .await
    {
        ensure!(
            error.to_string().contains("duplicate column name"),
            "tool call task column cannot be added: {error}"
        );
    }
    // Older `calls.sqlite` revisions share this file with the current layout. Additive `ALTER`
    // upgrades keep every existing fact; a schema only ever moves forward.
    ensure_columns(
        db,
        "calls_meta",
        &[
            ("schema_version", "INTEGER NOT NULL DEFAULT 0"),
            ("database_id", "TEXT"),
        ],
    )
    .await?;
    let row = db
        .query_one_raw(statement(
            "SELECT schema_version,database_id FROM calls_meta WHERE id=1",
            vec![],
        ))
        .await?;
    let Some(row) = row else {
        db.execute_raw(statement(
            "INSERT INTO calls_meta(id,schema_version,database_id) VALUES(1,?,?)",
            vec![
                CALLS_SCHEMA_VERSION.into(),
                crate::studio::new_id("calls-db").into(),
            ],
        ))
        .await?;
        return Ok(());
    };
    let version: i64 = row.try_get("", "schema_version")?;
    ensure!(
        version <= CALLS_SCHEMA_VERSION,
        "unsupported future calls schema {version}; existing data preserved"
    );
    let database_id: Option<String> = row.try_get("", "database_id")?;
    if database_id.as_deref().is_none_or(str::is_empty) {
        db.execute_raw(statement(
            "UPDATE calls_meta SET database_id=? WHERE id=1",
            vec![crate::studio::new_id("calls-db").into()],
        ))
        .await?;
    }
    if version == CALLS_SCHEMA_VERSION {
        return Ok(());
    }
    tracing::warn!(
        from = version,
        to = CALLS_SCHEMA_VERSION,
        "migrating calls schema to the external-blob layout"
    );
    migrate_calls_schema(db, blobs_dir, version).await
}

/// 当前 schema 之前的内联正文列名集合；外置 blobs 之前的任一版本都可能写入这些列。
const LEGACY_BODY_COLUMNS: &[&str] = &["body", "body_text", "body_json"];
/// 计费观察在旧格式中使用的内联正文列名。
const LEGACY_BILLING_COLUMNS: &[&str] = &["billing_body", "billing_json"];

/// 把旧版 `calls.sqlite` 数据保全地迁移到当前外置 blobs 格式。
///
/// 迁移只做加法：补齐当前列与表、把历史内联正文改写为内容寻址 blob 并在 `model_calls`/`tool_calls`
/// 上回填引用，最后前移 `schema_version`。旧的内联列保留原值、不再被读取，因此任何一步失败都
/// 不会丢失事实。
async fn migrate_calls_schema(
    db: &DatabaseConnection,
    blobs_dir: &Path,
    from_version: i64,
) -> Result<()> {
    ensure_columns(
        db,
        "call_watermarks",
        &[
            ("admitted_write_seq", "INTEGER NOT NULL DEFAULT 0"),
            ("durable_write_seq", "INTEGER NOT NULL DEFAULT 0"),
        ],
    )
    .await?;
    let model_columns = ensure_columns(
        db,
        "model_calls",
        &[
            ("root_thread_id", "TEXT"),
            ("turn_id", "TEXT"),
            ("attempt_id", "TEXT"),
            ("retry_of", "TEXT"),
            ("retention", "TEXT"),
            ("revision", "INTEGER NOT NULL DEFAULT 0"),
            ("admitted_at", "INTEGER NOT NULL DEFAULT 0"),
            ("started_at", "INTEGER NOT NULL DEFAULT 0"),
            ("finished_at", "INTEGER"),
            ("status", "TEXT"),
            ("terminal", "INTEGER NOT NULL DEFAULT 0"),
            ("purpose", "TEXT"),
            ("provider_instance_id", "TEXT"),
            ("provider_display_name", "TEXT"),
            ("configured_model", "TEXT"),
            ("sent_model", "TEXT"),
            ("reported_model", "TEXT"),
            ("reasoning_effort", "TEXT"),
            ("input_tokens", "INTEGER"),
            ("output_tokens", "INTEGER"),
            ("cache_read_tokens", "INTEGER"),
            ("cache_write_tokens", "INTEGER"),
            ("reasoning_tokens", "INTEGER"),
            ("total_tokens", "INTEGER"),
            ("ttft_millis", "INTEGER"),
            ("decode_millis", "INTEGER"),
            ("response_millis", "INTEGER"),
            ("cost_currency", "TEXT"),
            ("cost_amount", "REAL"),
            ("has_unpriced_usage", "INTEGER NOT NULL DEFAULT 0"),
            ("body_ref", "TEXT"),
            ("billing_ref", "TEXT"),
        ],
    )
    .await?;
    let tool_columns = ensure_columns(
        db,
        "tool_calls",
        &[
            ("turn_id", "TEXT"),
            ("tool_id", "TEXT"),
            ("revision", "INTEGER NOT NULL DEFAULT 0"),
            ("admitted_at", "INTEGER NOT NULL DEFAULT 0"),
            ("started_at", "INTEGER NOT NULL DEFAULT 0"),
            ("finished_at", "INTEGER"),
            ("status", "TEXT"),
            ("terminal", "INTEGER NOT NULL DEFAULT 0"),
            ("body_ref", "TEXT"),
        ],
    )
    .await?;
    // 更早的格式可能把正文内联在 `call_bodies` 中；先补齐这些引用指向的 blob 文件。
    externalize_call_bodies(db, blobs_dir).await?;
    for column in LEGACY_BODY_COLUMNS {
        for (table, columns) in [
            ("model_calls", &model_columns),
            ("tool_calls", &tool_columns),
        ] {
            externalize_inline_body(db, blobs_dir, table, columns, column, "body_ref").await?;
        }
    }
    for column in LEGACY_BILLING_COLUMNS {
        externalize_inline_body(
            db,
            blobs_dir,
            "model_calls",
            &model_columns,
            column,
            "billing_ref",
        )
        .await?;
    }
    db.execute_raw(statement(
        "UPDATE calls_meta SET schema_version=? WHERE id=1",
        vec![CALLS_SCHEMA_VERSION.into()],
    ))
    .await?;
    tracing::info!(
        from = from_version,
        to = CALLS_SCHEMA_VERSION,
        "calls schema migration completed with data preserved"
    );
    Ok(())
}

/// 把旧 `call_bodies` 中内联保存的正文补写成内容寻址 blob 文件。
///
/// 该布局仍然用摘要作为 `body_ref`，所以只需按引用回写文件；引用不是合法 sha256 摘要时跳过，
/// 不猜测也无法校验的格式。
async fn externalize_call_bodies(db: &DatabaseConnection, blobs_dir: &Path) -> Result<()> {
    let columns = table_columns(db, "call_bodies").await?;
    if !columns.contains("body_ref") {
        return Ok(());
    }
    for inline_column in LEGACY_BODY_COLUMNS {
        if !columns.contains(*inline_column) {
            continue;
        }
        let rows = db
            .query_all_raw(statement(
                &format!(
                    "SELECT body_ref AS body_ref,{inline_column} AS body
                     FROM call_bodies WHERE {inline_column} IS NOT NULL"
                ),
                vec![],
            ))
            .await?;
        for row in &rows {
            let reference: String = row.try_get("", "body_ref")?;
            let body: String = row.try_get("", "body")?;
            if body.is_empty() || blob_file_name(&reference).is_none() {
                continue;
            }
            write_blob(blobs_dir, &reference, &body).await?;
        }
    }
    Ok(())
}

/// 补齐缺失的列并返回迁移后的列集合；列/表名都是编译期常量，不做动态 SQL 拼接注入面。
async fn ensure_columns(
    db: &DatabaseConnection,
    table: &str,
    specs: &[(&str, &str)],
) -> Result<BTreeSet<String>> {
    let mut columns = table_columns(db, table).await?;
    for (column, declaration) in specs {
        if !columns.contains(*column) {
            db.execute_unprepared(&format!(
                "ALTER TABLE {table} ADD COLUMN {column} {declaration}"
            ))
            .await?;
            columns.insert((*column).to_string());
        }
    }
    Ok(columns)
}

async fn table_columns(db: &DatabaseConnection, table: &str) -> Result<BTreeSet<String>> {
    let rows = db
        .query_all_raw(statement(&format!("PRAGMA table_info({table})"), vec![]))
        .await?;
    rows.iter()
        .map(|row| row.try_get::<String>("", "name").map_err(Into::into))
        .collect()
}

/// 把一列旧版内联正文改写为内容寻址 blob，并在目标引用列上回填摘要。
///
/// 只有 `<inline_column>` 已存在、目标引用列为空且正文非空时才改写；重复运行幂等。
async fn externalize_inline_body(
    db: &DatabaseConnection,
    blobs_dir: &Path,
    table: &str,
    columns: &BTreeSet<String>,
    inline_column: &str,
    target_column: &str,
) -> Result<()> {
    if !columns.contains(inline_column) || !columns.contains(target_column) {
        return Ok(());
    }
    let at_expression = if columns.contains("finished_at") && columns.contains("started_at") {
        "COALESCE(finished_at,started_at,admitted_at)"
    } else if columns.contains("started_at") {
        "started_at"
    } else if columns.contains("admitted_at") {
        "admitted_at"
    } else {
        "0"
    };
    let rows = db
        .query_all_raw(statement(
            &format!(
                "SELECT rowid AS row_id,{inline_column} AS body,{at_expression} AS at
                 FROM {table}
                 WHERE {inline_column} IS NOT NULL AND {target_column} IS NULL"
            ),
            vec![],
        ))
        .await?;
    for row in &rows {
        let row_id: i64 = row.try_get("", "row_id")?;
        let body: String = row.try_get("", "body")?;
        if body.is_empty() {
            continue;
        }
        let at: i64 = row.try_get("", "at").unwrap_or(0);
        let reference = body_ref(&body);
        write_blob(blobs_dir, &reference, &body).await?;
        db.execute_raw(statement(
            "INSERT INTO call_bodies(body_ref,byte_length,created_at) VALUES(?,?,?)
             ON CONFLICT(body_ref) DO NOTHING",
            vec![
                reference.clone().into(),
                i64::try_from(body.len())?.into(),
                at.into(),
            ],
        ))
        .await?;
        db.execute_raw(statement(
            &format!("UPDATE {table} SET {target_column}=? WHERE rowid=?"),
            vec![reference.into(), row_id.into()],
        ))
        .await?;
    }
    Ok(())
}

/// writer supervisor：worker panic 后重启；在途事实由等待方重试，不会被静默丢弃。
async fn supervise_writer(shared: Arc<CallsWriter>) {
    loop {
        let worker = tokio::spawn(run_writer(shared.clone()));
        let Err(error) = worker.await else {
            return;
        };
        record_last_error(
            &shared,
            format!("call writer terminated unexpectedly: {error}"),
        );
        if shared.stopping.load(Ordering::Acquire) {
            return;
        }
        tokio::time::sleep(Duration::from_secs(1)).await;
        if shared.stopping.load(Ordering::Acquire) {
            return;
        }
    }
}

/// 唯一的调用库写入循环：取批、应用、重试、推进水位。
///
/// 受理方（attempt effect 与计费观察）会等待自己那次写入的结果，所以这里不做人为的批量等待：
/// 只要队列非空就立刻取一批，批量大小来自并发受理自然堆积的条目。
async fn run_writer(shared: Arc<CallsWriter>) {
    let mut retries = 0usize;
    loop {
        let stopping = shared.stopping.load(Ordering::Acquire);
        let pressure = queue_pressure(&shared);
        if pressure.mutations == 0 {
            if stopping {
                return;
            }
            shared.work_notify.notified().await;
            continue;
        }
        if pressure.bytes >= MAX_QUEUE_BYTES {
            tracing::warn!(
                mutations = pressure.mutations,
                bytes = pressure.bytes,
                oldest_age_ms = pressure
                    .oldest_accepted_at
                    .map_or(0, |oldest| oldest.elapsed().as_millis() as u64),
                "call writer queue is under pressure"
            );
        }
        let mut pending: VecDeque<QueuedCallMutation> = drain_batch(&shared).into();
        if pending.is_empty() {
            continue;
        }
        let batch_bytes = pending.iter().fold(0u64, |total, entry| {
            total.saturating_add(entry.bytes as u64)
        });
        shared.in_flight_bytes.store(batch_bytes, Ordering::Release);
        let mut deferred: Option<(QueuedCallMutation, BatchFailure)> = None;
        while let Some(entry) = pending.pop_front() {
            let ticket = entry.ticket;
            match apply_mutation(&shared, &entry.mutation).await {
                Ok(outcome) => {
                    retries = 0;
                    complete_mutation(entry, outcome);
                    advance_durable(&shared, ticket);
                    clear_last_error(&shared);
                    notify_space(&shared);
                }
                Err(failure) if failure.retryable && shared.stopping.load(Ordering::Acquire) => {
                    // 停机不再退避：未写入事实显式交回等待方，由调用方保留并重试。
                    fail_mutation(
                        &shared,
                        entry,
                        &format!("call writer stopped before writing: {}", failure.message),
                    );
                    advance_durable(&shared, ticket);
                    notify_space(&shared);
                }
                Err(failure) if failure.retryable => {
                    deferred = Some((entry, failure));
                    break;
                }
                Err(failure) => {
                    fail_mutation(&shared, entry, &failure.message);
                    advance_durable(&shared, ticket);
                    notify_space(&shared);
                }
            }
        }
        // 本批已处理完（含退回队首的 deferred 条目），不再有 in-flight 字节。
        shared.in_flight_bytes.store(0, Ordering::Release);
        let Some((entry, failure)) = deferred else {
            continue;
        };
        pending.push_front(entry);
        requeue(&shared, pending);
        notify_space(&shared);
        retries = retries.saturating_add(1);
        if shared.stopping.load(Ordering::Acquire) {
            tokio::task::yield_now().await;
            continue;
        }
        let backoff = if retries <= FAST_RETRIES {
            RETRY_BACKOFF * u32::try_from(retries).unwrap_or(u32::MAX)
        } else {
            let exponent = u32::try_from(retries.saturating_sub(FAST_RETRIES + 1))
                .unwrap_or(u32::MAX)
                .min(5);
            Duration::from_secs(1u64 << exponent).min(MAX_RETRY_BACKOFF)
        };
        tracing::warn!(
            attempt = retries,
            error = failure.message,
            "call writer retrying after a transient failure"
        );
        wait_for_retry(&shared, backoff).await;
    }
}

async fn wait_for_retry(shared: &CallsWriter, backoff: Duration) {
    tokio::select! {
        _ = shared.retry_notify.notified() => {}
        _ = tokio::time::sleep(backoff) => {}
    }
}

/// 从队首按条目/字节预算取一批 entry；保持 FIFO 顺序。
fn drain_batch(shared: &CallsWriter) -> Vec<QueuedCallMutation> {
    let mut queue = shared
        .queue
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    let mut entries = Vec::with_capacity(MAX_BATCH_MUTATIONS.min(queue.len().max(1)));
    let mut bytes = 0usize;
    while entries.len() < MAX_BATCH_MUTATIONS {
        let Some(front) = queue.front() else {
            break;
        };
        let next_bytes = bytes.saturating_add(front.bytes);
        if !entries.is_empty() && next_bytes > MAX_BATCH_BYTES {
            break;
        }
        bytes = next_bytes;
        entries.push(queue.pop_front().expect("front entry checked"));
    }
    entries
}

/// 瞬时失败后把未完成条目按原顺序放回队首等待重试。
fn requeue(shared: &CallsWriter, entries: VecDeque<QueuedCallMutation>) {
    let mut queue = shared
        .queue
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    for entry in entries.into_iter().rev() {
        queue.push_front(entry);
    }
}

/// 一次受理得到结论（提交或显式拒绝）后推进耐久 ticket 水位。
fn advance_durable(shared: &CallsWriter, ticket: u64) {
    let current = *shared.durable_ticket.borrow();
    if ticket > current {
        shared.durable_ticket.send_replace(ticket);
    }
}

fn complete_mutation(entry: QueuedCallMutation, outcome: MutationOutcome) {
    match (entry.completion, outcome) {
        (Some(CallCompletion::Effect(sender)), MutationOutcome::Effect) => {
            let _ = sender.send(Ok(()));
        }
        (Some(CallCompletion::Effect(sender)), MutationOutcome::Billing(_)) => {
            let _ = sender.send(Err(anyhow::anyhow!(
                "call writer returned a mismatched mutation outcome"
            )));
        }
        (Some(CallCompletion::Billing(sender)), MutationOutcome::Billing(write)) => {
            let _ = sender.send(Ok(write));
        }
        (Some(CallCompletion::Billing(sender)), MutationOutcome::Effect) => {
            let _ = sender.send(Err(anyhow::anyhow!(
                "call writer returned a mismatched mutation outcome"
            )));
        }
        (None, _) => {}
    }
}

fn fail_mutation(shared: &CallsWriter, entry: QueuedCallMutation, message: &str) {
    tracing::error!(error = message, "call writer rejected a call fact");
    record_last_error(shared, message.to_owned());
    match entry.completion {
        Some(CallCompletion::Effect(sender)) => {
            let _ = sender.send(Err(anyhow::anyhow!(message.to_owned())));
        }
        Some(CallCompletion::Billing(sender)) => {
            let _ = sender.send(Err(anyhow::anyhow!(message.to_owned())));
        }
        None => {}
    }
}

fn record_last_error(shared: &CallsWriter, message: String) {
    tracing::error!(error = message, "call writer is degraded");
    *shared
        .last_error
        .lock()
        .unwrap_or_else(|error| error.into_inner()) = Some(message);
}

/// A durable write succeeded, so the previously recorded degradation no longer describes the
/// writer. The error is retained until this point instead of clearing on admission or retry.
fn clear_last_error(shared: &CallsWriter) {
    *shared
        .last_error
        .lock()
        .unwrap_or_else(|error| error.into_inner()) = None;
}

fn notify_space(shared: &CallsWriter) {
    shared.space_notify.notify_one();
}

fn queue_is_empty(shared: &CallsWriter) -> bool {
    shared
        .queue
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .is_empty()
}

fn queue_bytes(queue: &VecDeque<QueuedCallMutation>) -> usize {
    queue
        .iter()
        .fold(0usize, |total, entry| total.saturating_add(entry.bytes))
}

fn queue_pressure(shared: &CallsWriter) -> QueuePressure {
    let queue = shared
        .queue
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    let mut pressure = QueuePressure {
        mutations: queue.len(),
        ..QueuePressure::default()
    };
    for entry in queue.iter() {
        pressure.bytes = pressure.bytes.saturating_add(entry.bytes);
        pressure.oldest_accepted_at = Some(
            pressure
                .oldest_accepted_at
                .map_or(entry.accepted_at, |current| current.min(entry.accepted_at)),
        );
    }
    pressure
}

async fn apply_mutation(
    shared: &CallsWriter,
    mutation: &CallMutation,
) -> Result<MutationOutcome, BatchFailure> {
    let result = match mutation {
        CallMutation::Effect(effect) => apply_effect(shared, effect)
            .await
            .map(|()| MutationOutcome::Effect),
        CallMutation::Billing {
            root_thread_id,
            thread_id,
            retention,
            billing,
        } => apply_billing(shared, root_thread_id, thread_id, billing, *retention)
            .await
            .map(MutationOutcome::Billing),
    };
    result.map_err(|error| {
        let message = error.to_string();
        BatchFailure {
            retryable: is_retryable_write(&message),
            message,
        }
    })
}

/// 把一次 effect 的调用事实写入调用库；调用开始与终态更新共享同一调用身份。
async fn apply_effect(shared: &CallsWriter, effect: &ThreadEffectBatch) -> Result<()> {
    let sequence = integer(effect.sequence)?;
    let tx = shared.db.begin().await?;
    let current = tx
        .query_one_raw(statement(
            "SELECT durable_write_seq FROM call_watermarks WHERE thread_id=?",
            vec![effect.thread_id.clone().into()],
        ))
        .await?
        .map(|row| row.try_get::<i64>("", "durable_write_seq"))
        .transpose()?
        .unwrap_or(0);
    if sequence <= current {
        tx.rollback().await?;
        return Ok(());
    }
    if let Some(attempt) = &effect.attempt {
        upsert_attempt(shared, &tx, effect, attempt, sequence).await?;
    }
    for delivery in effect.deliveries.iter() {
        deliver_tool_call(
            shared,
            &tx,
            &effect.thread_id,
            delivery,
            sequence,
            effect.committed_at,
        )
        .await?;
    }
    // Task lifecycle facts (in particular a cancel request) are the source state a repeated cancel
    // query needs after the owner released the task identity. Only the fact columns are recorded
    // here; the terminal delivery keeps ownership of status/body.
    for record in effect.tasks.iter() {
        record_task_lifecycle(&tx, &effect.thread_id, record, sequence).await?;
    }
    tx.execute_raw(statement(
        "INSERT INTO call_watermarks(thread_id,admitted_write_seq,durable_write_seq)
         VALUES(?,?,?)
         ON CONFLICT(thread_id) DO UPDATE SET
            admitted_write_seq=MAX(excluded.admitted_write_seq,call_watermarks.admitted_write_seq),
            durable_write_seq=MAX(excluded.durable_write_seq,call_watermarks.durable_write_seq)",
        vec![
            effect.thread_id.clone().into(),
            sequence.into(),
            sequence.into(),
        ],
    ))
    .await?;
    tx.commit().await?;
    Ok(())
}

/// 幂等写入一次计费/性能事实；同一身份不同正文明确失败。
async fn apply_billing(
    shared: &CallsWriter,
    root_thread_id: &str,
    thread_id: &str,
    billing: &InferenceBillingRecord,
    retention: CallRetention,
) -> Result<BillingWrite> {
    let body = serde_json::to_string(billing)?;
    let body_ref = body_ref(&body);
    let tx = shared.db.begin().await?;
    let existing = tx
        .query_one_raw(statement(
            "SELECT billing_ref FROM model_calls WHERE thread_id=? AND call_id=?",
            vec![thread_id.into(), billing.inference_id.clone().into()],
        ))
        .await?;
    if let Some(row) = existing {
        let stored: Option<String> = row.try_get("", "billing_ref")?;
        match stored {
            Some(stored) if stored == body_ref => {
                tx.rollback().await?;
                return Ok(BillingWrite::Identical);
            }
            Some(_) => {
                bail!(
                    "model call {} conflicts with the durable call record",
                    billing.inference_id
                );
            }
            None => {}
        }
    }
    // 计费观察不携带 attempt 执行序号：新行以 0 起底，冲突时保留既有 revision。
    // 这样同一调用身份的 attempt（commit）写入始终能以其 effect 序号覆盖计费快照，
    // 而不会被计费计数推高到阻碍终态。
    let revision: i64 = 0;
    let usage = billing.accounting.usage.totals();
    let has_unpriced_usage = billing.accounting.has_unpriced_usage();
    let (cost_currency, cost_amount) = match billing.accounting.estimated_costs().into_iter().next()
    {
        Some(cost) => (Some(cost.currency), Some(cost.amount)),
        None => (None, None),
    };
    let (ttft_millis, decode_millis, response_millis) = match billing.timing {
        Some(timing) => (
            opt_i64(timing.ttft_millis),
            opt_i64(timing.decode_millis),
            opt_i64(timing.total_millis),
        ),
        None => (None, None, None),
    };
    let (configured_model, sent_model, reported_model) = match &billing.model_observation {
        Some(observation) => (
            Some(observation.configured_model.clone()),
            Some(observation.sent_model.clone()),
            observation.reported_model.clone(),
        ),
        None => (None, Some(billing.model.clone()), None),
    };
    let recorded_at = billing.recorded_at;
    put_body(shared, &tx, &body_ref, &body, recorded_at).await?;
    tx.execute_raw(statement(
        "INSERT INTO model_calls(
            thread_id,call_id,root_thread_id,turn_id,attempt_id,retry_of,revision,admitted_at,
            started_at,finished_at,status,terminal,retention,purpose,provider_instance_id,
            provider_display_name,configured_model,sent_model,reported_model,reasoning_effort,
            input_tokens,output_tokens,cache_read_tokens,cache_write_tokens,reasoning_tokens,
            total_tokens,ttft_millis,decode_millis,response_millis,cost_currency,cost_amount,
            has_unpriced_usage,billing_ref)
         VALUES(?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?)
         ON CONFLICT(thread_id,call_id) DO UPDATE SET
            root_thread_id=COALESCE(excluded.root_thread_id,model_calls.root_thread_id),
            revision=model_calls.revision,
            finished_at=COALESCE(model_calls.finished_at,excluded.finished_at),
            status=CASE WHEN model_calls.terminal=0 THEN excluded.status ELSE model_calls.status END,
            terminal=1,
            retention=COALESCE(excluded.retention,model_calls.retention),
            purpose=COALESCE(excluded.purpose,model_calls.purpose),
            provider_instance_id=COALESCE(excluded.provider_instance_id,model_calls.provider_instance_id),
            provider_display_name=COALESCE(excluded.provider_display_name,model_calls.provider_display_name),
            configured_model=COALESCE(excluded.configured_model,model_calls.configured_model),
            sent_model=COALESCE(excluded.sent_model,model_calls.sent_model),
            reported_model=COALESCE(excluded.reported_model,model_calls.reported_model),
            reasoning_effort=COALESCE(excluded.reasoning_effort,model_calls.reasoning_effort),
            input_tokens=COALESCE(excluded.input_tokens,model_calls.input_tokens),
            output_tokens=COALESCE(excluded.output_tokens,model_calls.output_tokens),
            cache_read_tokens=COALESCE(excluded.cache_read_tokens,model_calls.cache_read_tokens),
            cache_write_tokens=COALESCE(excluded.cache_write_tokens,model_calls.cache_write_tokens),
            reasoning_tokens=COALESCE(excluded.reasoning_tokens,model_calls.reasoning_tokens),
            total_tokens=COALESCE(excluded.total_tokens,model_calls.total_tokens),
            ttft_millis=COALESCE(excluded.ttft_millis,model_calls.ttft_millis),
            decode_millis=COALESCE(excluded.decode_millis,model_calls.decode_millis),
            response_millis=COALESCE(excluded.response_millis,model_calls.response_millis),
            cost_currency=COALESCE(excluded.cost_currency,model_calls.cost_currency),
            cost_amount=COALESCE(excluded.cost_amount,model_calls.cost_amount),
            has_unpriced_usage=MAX(model_calls.has_unpriced_usage,excluded.has_unpriced_usage),
            billing_ref=excluded.billing_ref",
        vec![
            thread_id.into(),
            billing.inference_id.clone().into(),
            root_thread_id.into(),
            String::new().into(),
            billing.inference_id.clone().into(),
            Option::<String>::None.into(),
            revision.into(),
            recorded_at.into(),
            recorded_at.into(),
            Some(recorded_at).into(),
            CallStatus::Committed.as_str().into(),
            1_i32.into(),
            retention.as_str().into(),
            billing.purpose.clone().into(),
            billing.provider_instance_id.clone().into(),
            billing.provider.clone().into(),
            configured_model.into(),
            sent_model.into(),
            reported_model.into(),
            billing.reasoning_effort.clone().into(),
            opt_i64(usage.prompt_tokens).into(),
            opt_i64(usage.completion_tokens).into(),
            opt_i64(usage.cached_prompt_tokens).into(),
            opt_i64(usage.cache_write_tokens).into(),
            opt_i64(usage.reasoning_tokens).into(),
            opt_i64(usage.total_tokens).into(),
            ttft_millis.into(),
            decode_millis.into(),
            response_millis.into(),
            cost_currency.into(),
            cost_amount.into(),
            (has_unpriced_usage as i32).into(),
            body_ref.into(),
        ],
    ))
    .await?;
    tx.commit().await?;
    Ok(BillingWrite::Inserted)
}

async fn upsert_attempt(
    shared: &CallsWriter,
    tx: &impl ConnectionTrait,
    effect: &ThreadEffectBatch,
    attempt: &AttemptUpdate,
    sequence: i64,
) -> Result<()> {
    let status = attempt_status(&attempt.outcome);
    let terminal = status.is_terminal();
    let attempted_at = effect.committed_at;
    let body = serde_json::to_string(attempt)?;
    let body_ref = body_ref(&body);
    put_body(shared, tx, &body_ref, &body, attempted_at).await?;
    let (
        input_tokens,
        output_tokens,
        cache_read_tokens,
        cache_write_tokens,
        reasoning_tokens,
        total_tokens,
    ) = usage_columns(outcome_usage(&attempt.outcome));
    tx.execute_raw(statement(
        "INSERT INTO model_calls(
            thread_id,call_id,turn_id,attempt_id,retry_of,revision,admitted_at,started_at,
            finished_at,status,terminal,input_tokens,output_tokens,cache_read_tokens,
            cache_write_tokens,reasoning_tokens,total_tokens,body_ref)
         VALUES(?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?)
         ON CONFLICT(thread_id,call_id) DO UPDATE SET
            turn_id=excluded.turn_id,
            attempt_id=excluded.attempt_id,
            retry_of=COALESCE(excluded.retry_of,model_calls.retry_of),
            revision=excluded.revision,
            started_at=MIN(model_calls.started_at,excluded.started_at),
            finished_at=COALESCE(excluded.finished_at,model_calls.finished_at),
            status=excluded.status,
            terminal=excluded.terminal,
            input_tokens=COALESCE(excluded.input_tokens,model_calls.input_tokens),
            output_tokens=COALESCE(excluded.output_tokens,model_calls.output_tokens),
            cache_read_tokens=COALESCE(excluded.cache_read_tokens,model_calls.cache_read_tokens),
            cache_write_tokens=COALESCE(excluded.cache_write_tokens,model_calls.cache_write_tokens),
            reasoning_tokens=COALESCE(excluded.reasoning_tokens,model_calls.reasoning_tokens),
            total_tokens=COALESCE(excluded.total_tokens,model_calls.total_tokens),
            body_ref=COALESCE(excluded.body_ref,model_calls.body_ref)
         WHERE excluded.revision >= model_calls.revision
            AND (excluded.terminal=1 OR model_calls.terminal=0)",
        vec![
            effect.thread_id.clone().into(),
            attempt.attempt_id.clone().into(),
            attempt.turn_id.clone().into(),
            attempt.attempt_id.clone().into(),
            attempt.retry_of.clone().into(),
            sequence.into(),
            attempted_at.into(),
            attempted_at.into(),
            (if terminal { Some(attempted_at) } else { None }).into(),
            status.as_str().into(),
            (if terminal { 1_i32 } else { 0_i32 }).into(),
            input_tokens.into(),
            output_tokens.into(),
            cache_read_tokens.into(),
            cache_write_tokens.into(),
            reasoning_tokens.into(),
            total_tokens.into(),
            body_ref.into(),
        ],
    ))
    .await?;
    if let AttemptOutcome::Committed(output) = &attempt.outcome {
        for call in &output.tool_calls {
            let arguments = serde_json::to_string(&call.arguments)?;
            admit_tool_call(
                shared,
                tx,
                &effect.thread_id,
                &call.call_id,
                &attempt.turn_id,
                &call.tool_id,
                sequence,
                effect.committed_at,
                &arguments,
            )
            .await?;
        }
    }
    Ok(())
}

/// Records the task-lifecycle source state of one effect on its tool call row.
///
/// A cancel request is a core task revision, so it must be durable for a repeated cancel query to
/// answer with the recorded receipt instead of treating the identity as unknown. Only the
/// monotonically-raising facts are written: the revision never moves backwards and the terminal
/// status/body stay owned by the delivery write.
async fn record_task_lifecycle(
    tx: &impl ConnectionTrait,
    thread_id: &str,
    record: &pl_core::thread::task::TaskRecord,
    sequence: i64,
) -> Result<()> {
    let cancel_requested = if record.cancel_requested
        || record.status == pl_core::thread::task::TaskStatus::Cancelled
    {
        1_i32
    } else {
        0_i32
    };
    tx.execute_raw(statement(
        "UPDATE tool_calls SET revision=MAX(revision,?),cancel_requested=?
         WHERE thread_id=? AND call_id=?",
        vec![
            sequence.into(),
            cancel_requested.into(),
            thread_id.to_owned().into(),
            record.call_id.clone().into(),
        ],
    ))
    .await?;
    Ok(())
}

async fn deliver_tool_call(
    shared: &CallsWriter,
    tx: &impl ConnectionTrait,
    thread_id: &str,
    delivery: &ToolDelivery,
    sequence: i64,
    at: i64,
) -> Result<()> {
    let status = tool_status(&delivery.outcome);
    let body = serde_json::to_string(delivery)?;
    let body_ref = body_ref(&body);
    put_body(shared, tx, &body_ref, &body, at).await?;
    let existing = tx
        .query_one_raw(statement(
            "SELECT 1 FROM tool_calls WHERE thread_id=? AND call_id=?",
            vec![thread_id.into(), delivery.call_id.clone().into()],
        ))
        .await?;
    ensure!(
        existing.is_some(),
        "tool delivery {} has no admitted call",
        delivery.call_id
    );
    tx.execute_raw(statement(
        "UPDATE tool_calls SET revision=?,finished_at=?,status=?,terminal=1,body_ref=?
         WHERE thread_id=? AND call_id=? AND terminal=0",
        vec![
            sequence.into(),
            at.into(),
            status.as_str().into(),
            body_ref.into(),
            thread_id.into(),
            delivery.call_id.clone().into(),
        ],
    ))
    .await?;
    Ok(())
}

#[allow(clippy::too_many_arguments)]
async fn admit_tool_call(
    shared: &CallsWriter,
    tx: &impl ConnectionTrait,
    thread_id: &str,
    call_id: &str,
    turn_id: &str,
    tool_id: &str,
    sequence: i64,
    at: i64,
    body: &str,
) -> Result<()> {
    let body_ref = body_ref(body);
    put_body(shared, tx, &body_ref, body, at).await?;
    tx.execute_raw(statement(
        "INSERT INTO tool_calls(
            thread_id,call_id,turn_id,tool_id,revision,admitted_at,started_at,finished_at,
            status,terminal,body_ref)
         VALUES(?,?,?,?,?,?,?,?,?,?,?)
         ON CONFLICT(thread_id,call_id) DO UPDATE SET
            revision=excluded.revision,
            body_ref=COALESCE(excluded.body_ref,tool_calls.body_ref)
         WHERE excluded.revision > tool_calls.revision",
        vec![
            thread_id.into(),
            call_id.into(),
            turn_id.into(),
            tool_id.into(),
            sequence.into(),
            at.into(),
            at.into(),
            Option::<i64>::None.into(),
            CallStatus::Running.as_str().into(),
            0_i32.into(),
            body_ref.into(),
        ],
    ))
    .await?;
    Ok(())
}

fn model_fact(row: &QueryResult) -> Result<ModelCallFact> {
    Ok(ModelCallFact {
        revision: u64::try_from(row.try_get::<i64>("", "revision")?)?,
        thread_id: row.try_get("", "thread_id")?,
        root_thread_id: row.try_get("", "root_thread_id")?,
        call_id: row.try_get("", "call_id")?,
        turn_id: row.try_get("", "turn_id")?,
        retry_of: row.try_get("", "retry_of")?,
        status: row.try_get("", "status")?,
        terminal: row.try_get::<i64>("", "terminal")? != 0,
        retention: row.try_get("", "retention")?,
        purpose: row.try_get("", "purpose")?,
        provider_instance_id: row.try_get("", "provider_instance_id")?,
        provider_display_name: row.try_get("", "provider_display_name")?,
        configured_model: row.try_get("", "configured_model")?,
        sent_model: row.try_get("", "sent_model")?,
        reported_model: row.try_get("", "reported_model")?,
        reasoning_effort: row.try_get("", "reasoning_effort")?,
        input_tokens: opt_u64(row.try_get("", "input_tokens")?),
        output_tokens: opt_u64(row.try_get("", "output_tokens")?),
        cache_read_tokens: opt_u64(row.try_get("", "cache_read_tokens")?),
        cache_write_tokens: opt_u64(row.try_get("", "cache_write_tokens")?),
        reasoning_tokens: opt_u64(row.try_get("", "reasoning_tokens")?),
        total_tokens: opt_u64(row.try_get("", "total_tokens")?),
        ttft_millis: opt_u64(row.try_get("", "ttft_millis")?),
        decode_millis: opt_u64(row.try_get("", "decode_millis")?),
        response_millis: opt_u64(row.try_get("", "response_millis")?),
        cost_currency: row.try_get("", "cost_currency")?,
        cost_amount: row.try_get("", "cost_amount")?,
        has_unpriced_usage: row.try_get::<i64>("", "has_unpriced_usage")? != 0,
        started_at: row.try_get("", "started_at")?,
        finished_at: row.try_get("", "finished_at")?,
    })
}

fn performance_sample_row(row: &QueryResult) -> Result<PerformanceSampleRow> {
    Ok(PerformanceSampleRow {
        completed_at: row.try_get("", "completed_at")?,
        provider_instance_id: row.try_get("", "provider_instance_id")?,
        provider_display_name: row.try_get("", "provider_display_name")?,
        configured_model: row.try_get("", "configured_model")?,
        sent_model: row.try_get("", "sent_model")?,
        reported_model: row.try_get("", "reported_model")?,
        reasoning_effort: row.try_get("", "reasoning_effort")?,
        completion_tokens: required_u64(row, "output_tokens")?,
        ttft_millis: required_u64(row, "ttft_millis")?,
        decode_millis: required_u64(row, "decode_millis")?,
        response_millis: required_u64(row, "response_millis")?,
    })
}

fn performance_summary_row(row: &QueryResult) -> Result<PerformanceSummaryRow> {
    Ok(PerformanceSummaryRow {
        provider_instance_id: row.try_get("", "provider_instance_id")?,
        provider_display_name: row.try_get("", "provider_display_name")?,
        sent_model: row.try_get("", "sent_model")?,
        reasoning_effort: row.try_get("", "reasoning_effort")?,
        sample_count: required_u64(row, "sample_count")?,
        completion_tokens: required_u64(row, "completion_tokens")?,
        total_ttft_millis: required_u64(row, "total_ttft_millis")?,
        total_decode_millis: required_u64(row, "total_decode_millis")?,
        total_response_millis: required_u64(row, "total_response_millis")?,
    })
}

/// 写入内容寻址 calls blob 文件，并在库中登记引用与字节数。
async fn put_body(
    shared: &CallsWriter,
    tx: &impl ConnectionTrait,
    body_ref: &str,
    content: &str,
    at: i64,
) -> Result<()> {
    write_blob(&shared.blobs_dir, body_ref, content).await?;
    tx.execute_raw(statement(
        "INSERT INTO call_bodies(body_ref,byte_length,created_at) VALUES(?,?,?)
         ON CONFLICT(body_ref) DO NOTHING",
        vec![
            body_ref.into(),
            i64::try_from(content.len())?.into(),
            at.into(),
        ],
    ))
    .await?;
    Ok(())
}

/// 内容寻址写入：文件名就是摘要，重复写入直接复用已有文件。
async fn write_blob(dir: &Path, body_ref: &str, content: &str) -> Result<()> {
    write_blob_bytes(dir, body_ref, content.as_bytes()).await
}

/// 内容寻址写入的字节形式；migration 边界复制退役库正文时复用同一条落盘路径。
async fn write_blob_bytes(dir: &Path, body_ref: &str, content: &[u8]) -> Result<()> {
    let name = blob_file_name(body_ref).context("invalid call body reference")?;
    let path = dir.join(name);
    if tokio::fs::try_exists(&path).await? {
        return Ok(());
    }
    let staging = dir.join(format!(".{name}.staging"));
    tokio::fs::write(&staging, content).await?;
    match tokio::fs::rename(&staging, &path).await {
        Ok(()) => Ok(()),
        Err(error) => {
            let _ = tokio::fs::remove_file(&staging).await;
            if tokio::fs::try_exists(&path).await? {
                Ok(())
            } else {
                Err(error.into())
            }
        }
    }
}

/// 校验内容寻址引用并返回可安全拼接的文件名。
fn blob_file_name(body_ref: &str) -> Option<&str> {
    let hex = body_ref.strip_prefix("sha256:")?;
    if hex.len() != 64
        || !hex
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return None;
    }
    Some(hex)
}

fn attempt_status(outcome: &AttemptOutcome) -> CallStatus {
    match outcome {
        AttemptOutcome::Running => CallStatus::Running,
        AttemptOutcome::Interrupted => CallStatus::Interrupted,
        AttemptOutcome::Committed(_) => CallStatus::Committed,
        AttemptOutcome::Cancelled { .. } => CallStatus::Cancelled,
        AttemptOutcome::Failed(_) => CallStatus::Failed,
        AttemptOutcome::Rejected { .. } => CallStatus::Rejected,
    }
}

fn tool_status(outcome: &ToolOutcome) -> CallStatus {
    match outcome {
        ToolOutcome::Succeeded => CallStatus::Completed,
        ToolOutcome::Cancelled => CallStatus::Cancelled,
        ToolOutcome::Interrupted => CallStatus::Interrupted,
        ToolOutcome::Failed(_) => CallStatus::Failed,
    }
}

fn outcome_usage(outcome: &AttemptOutcome) -> Option<&ModelUsage> {
    match outcome {
        AttemptOutcome::Running | AttemptOutcome::Interrupted => None,
        AttemptOutcome::Committed(output) | AttemptOutcome::Rejected { output, .. } => {
            Some(&output.usage)
        }
        AttemptOutcome::Failed(error) => Some(&error.usage),
        AttemptOutcome::Cancelled { result } => Some(match result {
            Ok(output) => &output.usage,
            Err(error) => &error.usage,
        }),
    }
}

#[allow(clippy::type_complexity)]
fn usage_columns(
    usage: Option<&ModelUsage>,
) -> (
    Option<i64>,
    Option<i64>,
    Option<i64>,
    Option<i64>,
    Option<i64>,
    Option<i64>,
) {
    let Some(usage) = usage else {
        return (None, None, None, None, None, None);
    };
    let total = usage
        .input_tokens
        .zip(usage.output_tokens)
        .and_then(|(input, output)| input.checked_add(output));
    // `ModelUsage` marks an unmeasured token class as `None`: preserve that unknown distinction instead
    // of collapsing it to zero.
    (
        usage.input_tokens.and_then(opt_i64),
        usage.output_tokens.and_then(opt_i64),
        usage.cache_read_tokens.and_then(opt_i64),
        usage.cache_write_tokens.and_then(opt_i64),
        usage.reasoning_tokens.and_then(opt_i64),
        total.and_then(opt_i64),
    )
}

fn opt_i64(value: u64) -> Option<i64> {
    i64::try_from(value).ok()
}

fn opt_u64(value: Option<i64>) -> Option<u64> {
    value.and_then(|value| u64::try_from(value).ok())
}

fn required_u64(row: &QueryResult, column: &str) -> Result<u64> {
    let value: Option<i64> = row.try_get("", column)?;
    Ok(value
        .and_then(|value| u64::try_from(value).ok())
        .unwrap_or(0))
}

fn body_ref(content: &str) -> String {
    pl_core::context::content_hash(content.as_bytes())
}

fn statement(sql: &str, values: Vec<Value>) -> Statement {
    Statement::from_sql_and_values(DatabaseBackend::Sqlite, sql, values)
}

fn integer(value: u64) -> Result<i64> {
    value
        .try_into()
        .map_err(|_| anyhow::anyhow!("call write sequence exceeds SQLite range"))
}

/// 只有忙/锁/IO 类错误允许自动重试；结构或约束错误必须显式回给调用方。
fn is_retryable_write(message: &str) -> bool {
    let message = message.to_ascii_lowercase();
    message.contains("database is locked")
        || message.contains("database table is locked")
        || message.contains("database is busy")
        || message.contains("disk i/o error")
        || message.contains("sqlite_busy")
        || message.contains("sqlite_locked")
        || message.contains("sqlite_ioerr")
}

/// 一次性 migration 边界对退役 `calls.sqlite` 的审计。
///
/// 只在持独占 Studio runtime 锁的一次性转换中产生，并写进 durable migration report：源格式版本、
/// 源字节指纹、源的行/水位/正文计数与正文校验数量。`verified` 仅当身份、终态、列、水位与内容寻址
/// 正文全部校验通过后为真；退役库只有在 `verified` 为真时才允许归档。
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub(crate) struct CallsImportReport {
    /// 退役库是否实际存在并被检查；缺失时为 `false`，其余字段为零值。
    pub(crate) source_present: bool,
    /// 源库识别到的 `calls_meta.schema_version`。
    pub(crate) source_schema_version: i64,
    pub(crate) source_database_id: Option<String>,
    /// 源库主文件 + `-wal` + `blobs` 正文的只读聚合指纹；phase-1 记录，phase-3/phase-4 在
    /// "实时源位置 + 字节保留归档位置"的并集上复算并绑定到同一源字节。
    pub(crate) source_fingerprint: String,
    pub(crate) source_model_calls: u64,
    pub(crate) source_tool_calls: u64,
    pub(crate) source_watermarks: u64,
    pub(crate) source_bodies: u64,
    /// 源库引用的正文引用中，已在保留快照与 staged 目标库逐项校验通过的数量。
    pub(crate) verified_bodies: u64,
    /// 校验通过时 staged 目标库中的总行数；发布前对"实际发布的行"的最后观测。
    pub(crate) destination_model_calls: u64,
    pub(crate) destination_tool_calls: u64,
    pub(crate) verified: bool,
}

/// 退役调用库的只读身份指纹：主文件与 `-wal` 的内容，以及 `blobs` 根下每个内容寻址 blob 的相对
/// 路径与完整字节。phase-1 记录、phase-3 导入前与 phase-4 发布前复算，把审计绑定到同一源字节。
///
/// 流式：主文件、`-wal` 与每个 blob 正文都按固定缓冲读取并写入摘要，内存占用与源库/blob 规模无关
/// 而是常数级；blob 正文一并纳入，等长内容篡改无法骗过身份比对。全程只读文件与目录，绝不打开
/// SQLite（WAL 的只读打开可能创建 `-shm` 而改动源目录），也绝不写任何源文件。
pub(crate) async fn legacy_call_store_fingerprint(source: &Path) -> Result<String> {
    legacy_call_store_fingerprint_over(source, None).await
}

/// 目标调用库（staging 或已发布 canonical `calls/calls.sqlite`）的**逻辑内容**身份。
///
/// 与退役源不同，目标库在迁移期间会被本进程多次打开、关闭：SQLite 在连接关闭时会把 `-wal`
/// 中的页 checkpoint 合并进主文件并删除 `-wal`。于是主文件与 `-wal` 的**物理字节**会在没有任何
/// 事实变化时改变（纯粹的 WAL/checkpoint 重排），而发布边界的守卫只关心**事实**是否被改动或
/// 丢失，因此这里对当前 schema 下可见的逻辑内容求指纹，而不是对物理字节求指纹：
///
/// * 以确定性顺序（按主键/身份列排序）摘要 `calls_meta`、`call_watermarks`、`model_calls`、
///   `tool_calls`、`call_bodies` 的每一行，行内各列先经 SQLite `quote()` 规范化为稳定文本；
/// * 摘要内容寻址 `blobs` 根下每个 blob 的相对路径与完整正文。
///
/// 任一事实行、水位、终态、正文登记或被引用 blob 的增删改都会改变指纹；只把同样的物理页在
/// WAL 与主文件之间搬运的重排不会。读取经 SQLite 完成，主文件与 `-wal` 中的事实一并以当前视图
/// 呈现，因此尚未 checkpoint 的写入也会被计入，绝不会因为只读主文件而漏判。phase-4 校验通过后
/// 记录，续跑时复算比对：一致即证明目标**事实**未被改动（无需重读旧会话 journal），任一事实
/// 变化都 fail closed。
pub(crate) async fn calls_store_fingerprint(database: &Path) -> Result<String> {
    ensure!(
        tokio::fs::try_exists(database).await?,
        "call store to fingerprint is missing: {}; existing data preserved",
        database.display()
    );
    let db = connect_calls_database(database).await?;
    let result = async {
        let mut hasher = Sha256::new();
        for (label, sql) in calls_content_fingerprint_queries() {
            hasher.update(label.as_bytes());
            hasher.update([0x1e]);
            let rows = db.query_all_raw(statement(&sql, vec![])).await?;
            for row in rows {
                let canonical: String = row.try_get("", "row")?;
                hasher.update(canonical.as_bytes());
                hasher.update([0x1f]);
            }
            // 行集合终止符：空表与"缺一个终止分隔符"必须得到不同的摘要。
            hasher.update([0x1d]);
        }
        digest_blob_identities(&mut hasher, &legacy_call_store_blobs_dir(database)).await?;
        Ok(pl_core::context::content_hash(&hasher.finalize()))
    }
    .await;
    close_connection(db, result).await
}

/// 逻辑内容指纹的确定性查询集合：每张表的全部事实列按稳定顺序投影成单列 `row` 文本。
fn calls_content_fingerprint_queries() -> [(&'static str, String); 5] {
    [
        (
            "calls_meta",
            content_row_query("calls_meta", "id,schema_version,database_id", "id"),
        ),
        (
            "call_watermarks",
            content_row_query(
                "call_watermarks",
                "thread_id,admitted_write_seq,durable_write_seq",
                "thread_id",
            ),
        ),
        (
            "model_calls",
            content_row_query("model_calls", MODEL_CALL_COLUMNS, "thread_id,call_id"),
        ),
        (
            "tool_calls",
            // `cancel_requested` 不在合并列清单里（它是无条件补的列），但它是取消回执的真实事实，
            // 必须一并绑进逻辑指纹。
            content_row_query(
                "tool_calls",
                "thread_id,call_id,turn_id,tool_id,revision,admitted_at,started_at,finished_at,\
                 status,terminal,body_ref,cancel_requested",
                "thread_id,call_id",
            ),
        ),
        (
            "call_bodies",
            content_row_query("call_bodies", "body_ref,byte_length,created_at", "body_ref"),
        ),
    ]
}

/// 把一列列名投影成稳定文本行：各列先取 `length(quote(col))`（列值的规范文本长度）再取
/// `quote(col)`（SQL 字面量文本），用不会出现在长度前缀中的 `char(30)`/`char(31)` 分隔。长度前缀
/// 让"某个值为空串/含分隔符"都无法与相邻列发生歧义，因此任一列字节变化都会改变行文本。
fn content_row_query(table: &str, columns: &str, order_by: &str) -> String {
    let projection = columns
        .split(',')
        .map(str::trim)
        .filter(|column| !column.is_empty())
        .map(|column| format!("length(quote({column}))||char(30)||quote({column})||char(31)"))
        .collect::<Vec<_>>()
        .join("||");
    format!("SELECT {projection} AS row FROM {table} ORDER BY {order_by}")
}

/// 退役调用库在"实时源位置 + 字节保留归档位置"上的聚合身份。
///
/// 发布阶段逐个 `rename` 主文件、`-wal`、`-shm` 与 `blobs`，因此崩溃或文件系统错误可能让源位置与
/// 归档位置各自持有同一制品的不同子集。该指纹从两个位置的并集中解析每个成员：某个成员只在其中
/// 一处分存在时按该处字节计入，于是"分裂但完整"的源与"完整在源"或"完整在归档"算出同一指纹，
/// 迁移得以在没有人工介入时继续推进。同一成员名在两处同时存在则判定为歧义并 fail closed，绝不
/// 静默丢弃其中一份。
pub(crate) struct LegacyCallStoreIdentity {
    /// 主文件是否在实时源位置或归档位置之一存在。
    pub(crate) present: bool,
    /// 聚合字节指纹；`present` 为 `false` 时为空串。
    pub(crate) fingerprint: String,
}

/// 计算退役调用库的聚合身份：先解析主文件是否存在，再对实时源与归档的并集串流求指纹。
pub(crate) async fn legacy_call_store_identity(
    source: &Path,
    archived: Option<&Path>,
) -> Result<LegacyCallStoreIdentity> {
    if resolve_member(source, archived).await?.is_none() {
        return Ok(LegacyCallStoreIdentity {
            present: false,
            fingerprint: String::new(),
        });
    }
    let fingerprint = legacy_call_store_fingerprint_over(source, archived).await?;
    Ok(LegacyCallStoreIdentity {
        present: true,
        fingerprint,
    })
}

/// 逐成员计算退役调用库的聚合指纹；`archived` 为 `None` 时退化为只读实时源位置。
async fn legacy_call_store_fingerprint_over(
    source: &Path,
    archived: Option<&Path>,
) -> Result<String> {
    let mut hasher = Sha256::new();
    let _ = digest_member_file(&mut hasher, source, archived, true).await?;
    let source_wal = sidecar_path(source, "-wal");
    let archived_wal = archived.map(|path| sidecar_path(path, "-wal"));
    let _ = digest_member_file(&mut hasher, &source_wal, archived_wal.as_deref(), false).await?;
    let source_blobs = legacy_call_store_blobs_dir(source);
    let archived_blobs = archived.map(legacy_call_store_blobs_dir);
    digest_member_blobs(&mut hasher, &source_blobs, archived_blobs.as_deref()).await?;
    let digest = hasher.finalize();
    Ok(pl_core::context::content_hash(&digest))
}

/// 在实时源与归档之间解析一个常规文件成员，并把它流式写入摘要；返回唯一存在的路径。
///
/// 两处同时存在同名成员是歧义，fail closed；`required` 为真而两处都缺失同样 fail closed，否则写入
/// "缺失"标记，使缺失成员与存在成员产生确定性的不同摘要。
async fn digest_member_file(
    hasher: &mut Sha256,
    source: &Path,
    archived: Option<&Path>,
    required: bool,
) -> Result<Option<PathBuf>> {
    let source_exists = tokio::fs::try_exists(source).await?;
    let archived_exists = match archived {
        Some(path) => tokio::fs::try_exists(path).await?,
        None => false,
    };
    ensure!(
        !(source_exists && archived_exists),
        "retired call store member exists both live and archived ({} and {}); refusing to guess \
         which copy is authoritative; existing data preserved",
        source.display(),
        archived
            .map(|path| path.display().to_string())
            .unwrap_or_default()
    );
    if source_exists {
        let _ = digest_file_into(hasher, source, false).await?;
        return Ok(Some(source.to_path_buf()));
    }
    if let Some(path) = archived.filter(|_| archived_exists) {
        let _ = digest_file_into(hasher, path, false).await?;
        return Ok(Some(path.to_path_buf()));
    }
    ensure!(
        !required,
        "retired call store member is missing ({}); existing data preserved",
        source.display()
    );
    hasher.update([0u8]);
    Ok(None)
}

/// 在实时源与归档之间解析 `blobs` 根，并把整棵目录树（含正文）写入摘要；两处同名根 fail closed。
async fn digest_member_blobs(
    hasher: &mut Sha256,
    source: &Path,
    archived: Option<&Path>,
) -> Result<()> {
    let source_exists = tokio::fs::try_exists(source).await?;
    let archived_exists = match archived {
        Some(path) => tokio::fs::try_exists(path).await?,
        None => false,
    };
    ensure!(
        !(source_exists && archived_exists),
        "retired call store blob root exists both live and archived ({} and {}); refusing to guess \
         which copy is authoritative; existing data preserved",
        source.display(),
        archived
            .map(|path| path.display().to_string())
            .unwrap_or_default()
    );
    if source_exists {
        return digest_blob_identities(hasher, source).await;
    }
    if let Some(path) = archived.filter(|_| archived_exists) {
        return digest_blob_identities(hasher, path).await;
    }
    // 缺失 `blobs` 根与存在但为空的根产生不同摘要，保证成员集合完全确定。
    hasher.update([0u8]);
    Ok(())
}

/// 在实时源与归档之间解析一个成员名对应的唯一路径；找不到或两处都有歧义时返回 `None`/错误。
async fn resolve_member(source: &Path, archived: Option<&Path>) -> Result<Option<PathBuf>> {
    let source_exists = tokio::fs::try_exists(source).await?;
    let archived_exists = match archived {
        Some(path) => tokio::fs::try_exists(path).await?,
        None => false,
    };
    ensure!(
        !(source_exists && archived_exists),
        "retired call store member exists both live and archived ({} and {}); refusing to guess \
         which copy is authoritative; existing data preserved",
        source.display(),
        archived
            .map(|path| path.display().to_string())
            .unwrap_or_default()
    );
    Ok(if source_exists {
        Some(source.to_path_buf())
    } else {
        archived.filter(|_| archived_exists).map(Path::to_path_buf)
    })
}

/// 把一个文件流式写入摘要：先写存在/缺失标记与长度，再按固定缓冲写入内容；返回文件是否存在。
async fn digest_file_into(hasher: &mut Sha256, path: &Path, missing_ok: bool) -> Result<bool> {
    use tokio::io::AsyncReadExt;

    const CHUNK: usize = 64 * 1024;
    let mut file = match tokio::fs::File::open(path).await {
        Ok(file) => file,
        Err(error) if missing_ok && error.kind() == std::io::ErrorKind::NotFound => {
            hasher.update([0u8]);
            return Ok(false);
        }
        Err(error) => return Err(error.into()),
    };
    let metadata = file.metadata().await?;
    hasher.update([1u8]);
    hasher.update(metadata.len().to_le_bytes());
    let mut buffer = vec![0u8; CHUNK];
    loop {
        let read = file.read(&mut buffer).await?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(true)
}

/// 把 `blobs` 目录下每个常规文件的相对路径与完整字节按名字排序后写入摘要；符号链接/重解析点 fail
/// closed。正文按固定缓冲串流读取，因此等长内容篡改也会改变摘要，而内存占用保持常数级。
async fn digest_blob_identities(hasher: &mut Sha256, blobs: &Path) -> Result<()> {
    let present = tokio::fs::try_exists(blobs).await?;
    hasher.update([u8::from(present)]);
    let mut entries: Vec<(String, PathBuf)> = Vec::new();
    if present {
        let mut pending = vec![blobs.to_path_buf()];
        while let Some(directory) = pending.pop() {
            let mut listing = tokio::fs::read_dir(&directory).await?;
            while let Some(entry) = listing.next_entry().await? {
                let path = entry.path();
                let file_type = entry.file_type().await?;
                if file_type.is_dir() {
                    pending.push(path);
                    continue;
                }
                ensure!(
                    file_type.is_file(),
                    "retired call store blob is not a regular file: {}; existing data preserved",
                    path.display()
                );
                let relative = path
                    .strip_prefix(blobs)
                    .context("retired call store blob escaped its root")?;
                let name = relative
                    .to_str()
                    .context("retired call store blob name is not UTF-8")?
                    .to_owned();
                entries.push((name, path));
            }
        }
    }
    entries.sort();
    for (name, path) in entries {
        hasher.update(
            u64::try_from(name.len())
                .context("blob name is too long")?
                .to_le_bytes(),
        );
        hasher.update(name.as_bytes());
        // 正文一并绑定：名字与长度相同但内容被等长改写的 blob 必须改变摘要，phase-4 之后的篡改
        // 才能被发布边界拒绝。
        let _ = digest_file_into(hasher, &path, false).await?;
    }
    Ok(())
}

/// phase-3：在迁移工作副本上归一化旧 calls 格式并把事实合并进 staged 目标库。
///
/// `snapshot` 必须是 phase-1 备份出的字节副本；原始退役库从不被 SQLite 打开或改写。
pub(crate) async fn merge_legacy_call_store(
    snapshot: &Path,
    destination: &Path,
    work_dir: &Path,
    source_fingerprint: &str,
) -> Result<CallsImportReport> {
    run_legacy_call_store(
        snapshot,
        destination,
        work_dir,
        source_fingerprint,
        LegacyCallMode::Import,
    )
    .await
}

/// phase-4：只从同一保留快照重新推导审计，校验 staged 目标库完整覆盖源事实，不写目标库。
pub(crate) async fn verify_legacy_call_store(
    snapshot: &Path,
    destination: &Path,
    work_dir: &Path,
    source_fingerprint: &str,
) -> Result<CallsImportReport> {
    run_legacy_call_store(
        snapshot,
        destination,
        work_dir,
        source_fingerprint,
        LegacyCallMode::Verify,
    )
    .await
}

/// 一次退役调用库操作是"导入"还是"仅校验"。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LegacyCallMode {
    /// phase-3：缺失的源事实被写入 staged 目标库。
    Import,
    /// phase-4：只读校验 staged 目标库，绝不写目标库。
    Verify,
}

/// 相位机共享主体：从保留快照派生工作副本、归一化、导入或校验，全程不改原始源字节。
///
/// 只在持独占 Studio runtime 锁、且 staging 调用库 writer 已停止时调用：此时目标库只有本函数一个
/// writer。`snapshot` 是早前备份出的字节副本；格式升级只发生在派生工作副本上。未知/损坏/超前格式、
/// 正文缺失或哈希不符、目标库未覆盖源事实都会 fail closed 并保留全部原字节。
async fn run_legacy_call_store(
    snapshot: &Path,
    destination: &Path,
    work_dir: &Path,
    source_fingerprint: &str,
    mode: LegacyCallMode,
) -> Result<CallsImportReport> {
    ensure!(
        snapshot != destination,
        "refusing to import a retired call store onto itself; existing data preserved"
    );
    ensure!(
        tokio::fs::try_exists(snapshot).await?,
        "retired call store snapshot is missing: {}; existing data preserved",
        snapshot.display()
    );
    // Bind the bytes we are about to derive from to the recorded source identity: a stale or partially
    // written backup, or one whose blobs/sidecar differ from the recorded source, is refused before any
    // normalization or merge. The digest is streamed, so this is a bounded-memory re-read.
    ensure!(
        !source_fingerprint.is_empty(),
        "retired call store source fingerprint is absent; existing data preserved"
    );
    let snapshot_fingerprint = legacy_call_store_fingerprint(snapshot).await?;
    ensure!(
        snapshot_fingerprint == source_fingerprint,
        "retired call store backup does not match the recorded source fingerprint; existing data \
         preserved"
    );
    let destination_blobs = legacy_call_store_blobs_dir(destination);
    remove_dir_if_present(work_dir).await?;
    let result = async {
        let work_database = snapshot_legacy_call_store(snapshot, work_dir).await?;
        let work_blobs = work_dir.join(CALLS_BLOBS_DIR_NAME);
        // 归一化只作用于工作副本：补齐当前列/表并把内联正文外置为内容寻址 blob（加法、幂等）。
        let work = connect_calls_database(&work_database).await?;
        let normalized = normalize_legacy_call_store(&work, &work_blobs).await;
        let source_schema_version = close_connection(work, normalized).await?;

        tokio::fs::create_dir_all(&destination_blobs).await?;
        let target = connect_calls_database(destination).await?;
        let inner = async {
            ensure_calls_schema(&target, &destination_blobs).await?;
            attach_legacy_call_store(&target, &work_database).await?;
            let audit = apply_legacy_call_store(
                &target,
                source_schema_version,
                source_fingerprint,
                &work_blobs,
                &destination_blobs,
                mode,
            )
            .await;
            let detach = detach_legacy_call_store(&target).await;
            combine_import_and_detach(audit, detach)
        }
        .await;
        close_connection(target, inner).await
    }
    .await;
    let cleanup = remove_dir_if_present(work_dir).await;
    match (result, cleanup) {
        (Ok(report), Ok(())) => Ok(report),
        (Err(error), _) => Err(error),
        (Ok(_), Err(error)) => {
            Err(error.context("failed to clean the retired call store workspace"))
        }
    }
}

/// 在迁移工作副本上校验 `calls` 格式并就地升级；返回识别到的源 `schema_version`。
async fn normalize_legacy_call_store(db: &DatabaseConnection, blobs_dir: &Path) -> Result<i64> {
    let columns = table_columns(db, "calls_meta").await?;
    ensure!(
        columns.contains("schema_version"),
        "retired call store snapshot is not a recognizable calls database (missing \
         calls_meta.schema_version); existing data preserved"
    );
    let row = db
        .query_one_raw(statement(
            "SELECT schema_version FROM calls_meta WHERE id=1",
            vec![],
        ))
        .await?
        .context("retired call store snapshot has no calls_meta row; existing data preserved")?;
    let version: i64 = row.try_get("", "schema_version")?;
    ensure!(
        (1..=CALLS_SCHEMA_VERSION).contains(&version),
        "retired call store schema {version} is not a supported recognizable version (supported \
         1..={CALLS_SCHEMA_VERSION}); existing data preserved"
    );
    ensure_calls_schema(db, blobs_dir).await?;
    Ok(version)
}

/// 字节复制一份调用库（主文件、`-wal` 边车与 `blobs`）到 `destination` 目录，只读源。
///
/// 幂等且原子：目标主文件已存在时复用既有字节副本（phase-1 备份续跑不被改写；工作副本目录每轮先
/// 删除再重建，因此总是全新复制）；否则先复制到 `<destination>.partial` 再整体改名，崩溃不会留下可
/// 被误用的半成品。`-shm` 是瞬时索引文件，SQLite 打开副本时自建，故不入快照；原始退役库的 `-shm`
/// 保持不动，只在归档时随原字节整体退休。返回复制后的库文件路径。
///
/// 复用既有副本时本函数**不**证明它与当前源一致；调用方必须用
/// [`legacy_call_store_fingerprint`] 核对返回值与所记录的源指纹后才可使用。
pub(crate) async fn snapshot_legacy_call_store(
    source: &Path,
    destination: &Path,
) -> Result<PathBuf> {
    let name = source
        .file_name()
        .context("retired call store has no file name")?;
    let database = destination.join(name);
    if tokio::fs::try_exists(&database).await? {
        return Ok(database);
    }
    let partial = sidecar_path(destination, ".partial");
    remove_dir_if_present(&partial).await?;
    tokio::fs::create_dir_all(&partial).await?;
    copy_regular_file(source, &partial.join(name)).await?;
    let wal = sidecar_path(source, "-wal");
    if tokio::fs::try_exists(&wal).await? {
        let mut sidecar_name = name.to_os_string();
        sidecar_name.push("-wal");
        copy_regular_file(&wal, &partial.join(sidecar_name)).await?;
    }
    let blobs = legacy_call_store_blobs_dir(source);
    if tokio::fs::try_exists(&blobs).await? {
        copy_directory(&blobs, &partial.join(CALLS_BLOBS_DIR_NAME)).await?;
    }
    // The destination may hold a stale partial from an earlier crash; its main file is absent, so the
    // directory is incomplete and safe to replace.
    remove_dir_if_present(destination).await?;
    if let Some(parent) = destination.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }
    tokio::fs::rename(&partial, destination).await?;
    Ok(database)
}

async fn attach_legacy_call_store(db: &DatabaseConnection, source: &Path) -> Result<()> {
    let source = source
        .to_str()
        .context("retired call store path is not UTF-8")?
        .to_owned();
    db.execute_raw(statement(
        "ATTACH DATABASE ? AS legacy",
        vec![Value::String(Some(source))],
    ))
    .await?;
    Ok(())
}

async fn detach_legacy_call_store(db: &DatabaseConnection) -> Result<()> {
    db.execute_unprepared("DETACH DATABASE legacy").await?;
    Ok(())
}

fn combine_import_and_detach(
    audit: Result<CallsImportReport>,
    detach: Result<()>,
) -> Result<CallsImportReport> {
    match (audit, detach) {
        (Ok(report), Ok(())) => Ok(report),
        (Err(error), Ok(())) => Err(error),
        (Ok(_), Err(error)) => Err(error.context("failed to detach the retired call store")),
        (Err(error), Err(detach_error)) => Err(error.context(format!(
            "detaching the retired call store also failed: {detach_error}"
        ))),
    }
}

/// 在已挂载 `legacy` 的目标库连接上校验 +（Import 时）合并源事实，并返回审计。
///
/// 正文校验先于引用写入：`legacy` 引用的每个内容寻址正文都必须能从保留快照读出且摘要匹配。
async fn apply_legacy_call_store(
    db: &DatabaseConnection,
    source_schema_version: i64,
    source_fingerprint: &str,
    work_blobs: &Path,
    destination_blobs: &Path,
    mode: LegacyCallMode,
) -> Result<CallsImportReport> {
    let source_database_id = match db
        .query_one_raw(statement(
            "SELECT database_id FROM legacy.calls_meta WHERE id=1",
            vec![],
        ))
        .await?
    {
        Some(row) => row.try_get::<Option<String>>("", "database_id")?,
        None => None,
    };
    let source_model_calls = scalar_count(db, "SELECT COUNT(*) FROM legacy.model_calls").await?;
    let source_tool_calls = scalar_count(db, "SELECT COUNT(*) FROM legacy.tool_calls").await?;
    let source_watermarks = scalar_count(db, "SELECT COUNT(*) FROM legacy.call_watermarks").await?;
    let source_bodies = scalar_count(db, "SELECT COUNT(*) FROM legacy.call_bodies").await?;

    let verified_bodies =
        reconcile_legacy_call_bodies(db, work_blobs, destination_blobs, mode).await?;

    if mode == LegacyCallMode::Import {
        merge_attached_rows(db, MODEL_CALL_COLUMNS, "model_calls", MODEL_CALL_MERGE).await?;
        merge_attached_rows(db, TOOL_CALL_COLUMNS, "tool_calls", TOOL_CALL_MERGE).await?;
        merge_call_watermarks(db).await?;
        merge_call_bodies(db).await?;
    }
    verify_call_import(db, destination_blobs).await?;
    let destination_model_calls = scalar_count(db, "SELECT COUNT(*) FROM main.model_calls").await?;
    let destination_tool_calls = scalar_count(db, "SELECT COUNT(*) FROM main.tool_calls").await?;

    Ok(CallsImportReport {
        source_present: true,
        source_schema_version,
        source_database_id,
        source_fingerprint: source_fingerprint.to_owned(),
        source_model_calls: to_u64(source_model_calls)?,
        source_tool_calls: to_u64(source_tool_calls)?,
        source_watermarks: to_u64(source_watermarks)?,
        source_bodies: to_u64(source_bodies)?,
        verified_bodies: to_u64(verified_bodies)?,
        destination_model_calls: to_u64(destination_model_calls)?,
        destination_tool_calls: to_u64(destination_tool_calls)?,
        verified: true,
    })
}

/// 把 `legacy.<table>` 的行按身份合并进 `main.<table>`。新身份直接插入，已存在身份只补空列并提升
/// `revision`/`terminal`；`merge_set` 必须只使用 `COALESCE(<table>.<col>,excluded.<col>)`、
/// `MAX`/`MIN` 与由 `terminal`/`revision` 决定的 `status`，绝不覆盖目标库已有的更新事实。
async fn merge_attached_rows(
    db: &DatabaseConnection,
    columns: &str,
    table: &str,
    merge_set: &str,
) -> Result<()> {
    let sql = format!(
        "INSERT INTO main.{table}({columns}) \
         SELECT {columns} FROM legacy.{table} WHERE 1 \
         ON CONFLICT(thread_id,call_id) DO UPDATE SET {merge_set}"
    );
    db.execute_unprepared(&sql).await?;
    Ok(())
}

async fn merge_call_watermarks(db: &DatabaseConnection) -> Result<()> {
    db.execute_unprepared(
        "INSERT INTO main.call_watermarks(thread_id,admitted_write_seq,durable_write_seq) \
         SELECT thread_id,admitted_write_seq,durable_write_seq FROM legacy.call_watermarks WHERE 1 \
         ON CONFLICT(thread_id) DO UPDATE SET \
            admitted_write_seq=MAX(call_watermarks.admitted_write_seq,excluded.admitted_write_seq), \
            durable_write_seq=MAX(call_watermarks.durable_write_seq,excluded.durable_write_seq)",
    )
    .await?;
    Ok(())
}

async fn merge_call_bodies(db: &DatabaseConnection) -> Result<()> {
    db.execute_unprepared(
        "INSERT INTO main.call_bodies(body_ref,byte_length,created_at) \
         SELECT body_ref,byte_length,created_at FROM legacy.call_bodies WHERE 1 \
         ON CONFLICT(body_ref) DO NOTHING",
    )
    .await?;
    Ok(())
}

/// 校验保留快照中被引用的全部正文：逐个读取并按内容寻址摘要验证。
///
/// `Import` 时把缺失的正文写进目标 blobs 根，`Verify` 时只读校验（目标库缺一个即在后续覆盖检查中
/// fail closed）。返回校验通过的不同正文引用数。
async fn reconcile_legacy_call_bodies(
    db: &DatabaseConnection,
    work_blobs: &Path,
    destination_blobs: &Path,
    mode: LegacyCallMode,
) -> Result<i64> {
    let rows = db
        .query_all_raw(statement(
            "SELECT body_ref AS body_ref FROM legacy.call_bodies WHERE body_ref IS NOT NULL \
             UNION SELECT body_ref FROM legacy.model_calls WHERE body_ref IS NOT NULL \
             UNION SELECT billing_ref FROM legacy.model_calls WHERE billing_ref IS NOT NULL \
             UNION SELECT body_ref FROM legacy.tool_calls WHERE body_ref IS NOT NULL",
            vec![],
        ))
        .await?;
    let mut verified = 0i64;
    for row in rows {
        let reference: String = row.try_get("", "body_ref")?;
        let name = blob_file_name(&reference).with_context(|| {
            format!(
                "retired call body reference {reference} is not content-addressed; existing data \
                 preserved"
            )
        })?;
        let source = work_blobs.join(name);
        let bytes = tokio::fs::read(&source).await.with_context(|| {
            format!(
                "retired call body blob is missing: {}; existing data preserved",
                source.display()
            )
        })?;
        ensure!(
            pl_core::context::content_hash(&bytes) == reference,
            "retired call body {reference} content hash mismatch; existing data preserved"
        );
        if mode == LegacyCallMode::Import {
            let destination = destination_blobs.join(name);
            if !tokio::fs::try_exists(&destination).await? {
                write_blob_bytes(destination_blobs, &reference, &bytes).await?;
            }
        }
        verified = verified.saturating_add(1);
    }
    Ok(verified)
}

/// 逐项校验已合并的调用事实：没有源行丢失，没有非空源列丢失，终态/水位只进不退，且每个被引用的
/// 正文都有内容寻址文件与库内登记。任一失败都保留原字节并回错误。
async fn verify_call_import(db: &DatabaseConnection, destination_blobs: &Path) -> Result<()> {
    let missing_model = scalar_count(
        db,
        "SELECT COUNT(*) FROM legacy.model_calls s \
         LEFT JOIN main.model_calls d ON d.thread_id=s.thread_id AND d.call_id=s.call_id \
         WHERE d.thread_id IS NULL",
    )
    .await?;
    ensure!(
        missing_model == 0,
        "call import lost {missing_model} retired model call row(s); existing data preserved"
    );
    let missing_tool = scalar_count(
        db,
        "SELECT COUNT(*) FROM legacy.tool_calls s \
         LEFT JOIN main.tool_calls d ON d.thread_id=s.thread_id AND d.call_id=s.call_id \
         WHERE d.thread_id IS NULL",
    )
    .await?;
    ensure!(
        missing_tool == 0,
        "call import lost {missing_tool} retired tool call row(s); existing data preserved"
    );
    let model_disagreement = scalar_count(db, MODEL_CALL_VERIFY).await?;
    ensure!(
        model_disagreement == 0,
        "call import left {model_disagreement} retired model call fact(s) unreconciled; existing \
         data preserved"
    );
    let tool_disagreement = scalar_count(db, TOOL_CALL_VERIFY).await?;
    ensure!(
        tool_disagreement == 0,
        "call import left {tool_disagreement} retired tool call fact(s) unreconciled; existing data \
         preserved"
    );
    let watermark_disagreement = scalar_count(
        db,
        "SELECT COUNT(*) FROM legacy.call_watermarks s \
         LEFT JOIN main.call_watermarks d ON d.thread_id=s.thread_id \
         WHERE d.thread_id IS NULL OR d.durable_write_seq < s.durable_write_seq \
            OR d.admitted_write_seq < s.admitted_write_seq",
    )
    .await?;
    ensure!(
        watermark_disagreement == 0,
        "call import regressed {watermark_disagreement} thread watermark(s); existing data preserved"
    );
    let unregistered = scalar_count(
        db,
        "SELECT COUNT(*) FROM (\
            SELECT body_ref AS reference FROM main.model_calls WHERE body_ref IS NOT NULL \
            UNION SELECT billing_ref FROM main.model_calls WHERE billing_ref IS NOT NULL \
            UNION SELECT body_ref FROM main.tool_calls WHERE body_ref IS NOT NULL\
         ) refs \
         LEFT JOIN main.call_bodies b ON b.body_ref=refs.reference \
         WHERE b.body_ref IS NULL",
    )
    .await?;
    ensure!(
        unregistered == 0,
        "call import left {unregistered} call body reference(s) without a stored body; existing data \
         preserved"
    );
    for row in db
        .query_all_raw(statement(
            "SELECT reference FROM (\
                SELECT body_ref AS reference FROM main.model_calls WHERE body_ref IS NOT NULL \
                UNION SELECT billing_ref FROM main.model_calls WHERE billing_ref IS NOT NULL \
                UNION SELECT body_ref FROM main.tool_calls WHERE body_ref IS NOT NULL\
             )",
            vec![],
        ))
        .await?
    {
        let reference: String = row.try_get("", "reference")?;
        let Some(name) = blob_file_name(&reference) else {
            bail!(
                "stored call body reference {reference} is not content-addressed; existing data \
                 preserved"
            );
        };
        let path = destination_blobs.join(name);
        let bytes = tokio::fs::read(&path)
            .await
            .with_context(|| format!("stored call body blob is missing: {}", path.display()))?;
        ensure!(
            pl_core::context::content_hash(&bytes) == reference,
            "stored call body {reference} content hash mismatch; existing data preserved"
        );
    }
    Ok(())
}

async fn scalar_count(db: &DatabaseConnection, sql: &str) -> Result<i64> {
    let row = db
        .query_one_raw(statement(sql, vec![]))
        .await?
        .context("call import inspection returned no row")?;
    Ok(row.try_get_by_index::<i64>(0)?)
}

fn to_u64(value: i64) -> Result<u64> {
    u64::try_from(value).context("call import count is negative")
}

/// 退役调用库的 blobs 根（`calls.sqlite` 同目录的 `blobs`）；migration 备份/归档与 helper 共用。
pub(crate) fn legacy_call_store_blobs_dir(database: &Path) -> PathBuf {
    database
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .join(CALLS_BLOBS_DIR_NAME)
}

fn sidecar_path(path: &Path, suffix: &str) -> PathBuf {
    let mut name = path.as_os_str().to_os_string();
    name.push(suffix);
    PathBuf::from(name)
}

/// 字节复制单个常规文件；符号链接/重解析点或非文件一律拒绝，避免把源目录结构带进工作副本。
async fn copy_regular_file(source: &Path, destination: &Path) -> Result<()> {
    let metadata = tokio::fs::symlink_metadata(source).await?;
    ensure!(
        metadata.is_file() && !pl_tool::workspace::path_safety::is_link_or_reparse(&metadata),
        "retired call store member is not a regular file: {}; existing data preserved",
        source.display()
    );
    if let Some(parent) = destination.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }
    let bytes = tokio::fs::read(source).await?;
    tokio::fs::write(destination, &bytes).await?;
    Ok(())
}

/// 递归字节复制目录；只有常规文件被复制，任何其它类型 fail closed。
async fn copy_directory(source: &Path, destination: &Path) -> Result<()> {
    tokio::fs::create_dir_all(destination).await?;
    let mut pending = vec![source.to_path_buf()];
    while let Some(directory) = pending.pop() {
        let mut entries = tokio::fs::read_dir(&directory).await?;
        while let Some(entry) = entries.next_entry().await? {
            let path = entry.path();
            let file_type = entry.file_type().await?;
            let relative = path
                .strip_prefix(source)
                .context("retired call store blob escaped its root")?;
            let target = destination.join(relative);
            if file_type.is_dir() {
                tokio::fs::create_dir_all(&target).await?;
                pending.push(path);
            } else if file_type.is_file() {
                copy_regular_file(&path, &target).await?;
            } else {
                bail!(
                    "retired call store blob is not a regular file: {}; existing data preserved",
                    path.display()
                );
            }
        }
    }
    Ok(())
}

async fn remove_dir_if_present(path: &Path) -> Result<()> {
    match tokio::fs::remove_dir_all(path).await {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error.into()),
    }
}

/// 打开一个 migration 边界专用的调用库连接；工作副本与目标库都由本模块独占使用。
async fn connect_calls_database(path: &Path) -> Result<DatabaseConnection> {
    let mut options = ConnectOptions::new(crate::studio::paths::sqlite_url(path));
    options
        .max_connections(1)
        .min_connections(1)
        .sqlx_logging(false);
    Ok(Database::connect(options).await?)
}

async fn close_connection<T>(db: DatabaseConnection, result: Result<T>) -> Result<T> {
    match (result, db.close().await) {
        (Ok(value), Ok(())) => Ok(value),
        (Err(error), Ok(())) => Err(error),
        (Ok(_), Err(error)) => Err(error).context("failed to close a call store connection"),
        (Err(error), Err(close)) => {
            Err(error).context(format!("call store cleanup also failed: {close}"))
        }
    }
}

/// `model_calls` 的完整列清单；导入时源、目标都使用同一顺序，避免对列位置或 `SELECT *` 的隐式假设。
const MODEL_CALL_COLUMNS: &str = concat!(
    "thread_id,call_id,root_thread_id,turn_id,attempt_id,retry_of,revision,admitted_at,",
    "started_at,finished_at,status,terminal,retention,purpose,provider_instance_id,",
    "provider_display_name,configured_model,sent_model,reported_model,reasoning_effort,",
    "input_tokens,output_tokens,cache_read_tokens,cache_write_tokens,reasoning_tokens,",
    "total_tokens,ttft_millis,decode_millis,response_millis,cost_currency,cost_amount,",
    "has_unpriced_usage,body_ref,billing_ref",
);

/// 导入 `model_calls` 冲突时的合并规则：只补空列、只提升 `revision`/`terminal`，`status` 按终态与
/// revision 的支配关系选择，绝不覆盖目标库更完整或更新的事实。
const MODEL_CALL_MERGE: &str = concat!(
    "root_thread_id=COALESCE(model_calls.root_thread_id,excluded.root_thread_id),",
    "turn_id=COALESCE(model_calls.turn_id,excluded.turn_id),",
    "attempt_id=COALESCE(model_calls.attempt_id,excluded.attempt_id),",
    "retry_of=COALESCE(model_calls.retry_of,excluded.retry_of),",
    "revision=MAX(model_calls.revision,excluded.revision),",
    "admitted_at=MIN(model_calls.admitted_at,excluded.admitted_at),",
    "started_at=MIN(model_calls.started_at,excluded.started_at),",
    "finished_at=COALESCE(model_calls.finished_at,excluded.finished_at),",
    "status=CASE WHEN excluded.terminal>model_calls.terminal OR (excluded.terminal=model_calls.terminal AND excluded.revision>=model_calls.revision) THEN COALESCE(excluded.status,model_calls.status) ELSE COALESCE(model_calls.status,excluded.status) END,",
    "terminal=MAX(model_calls.terminal,excluded.terminal),",
    "retention=COALESCE(model_calls.retention,excluded.retention),",
    "purpose=COALESCE(model_calls.purpose,excluded.purpose),",
    "provider_instance_id=COALESCE(model_calls.provider_instance_id,excluded.provider_instance_id),",
    "provider_display_name=COALESCE(model_calls.provider_display_name,excluded.provider_display_name),",
    "configured_model=COALESCE(model_calls.configured_model,excluded.configured_model),",
    "sent_model=COALESCE(model_calls.sent_model,excluded.sent_model),",
    "reported_model=COALESCE(model_calls.reported_model,excluded.reported_model),",
    "reasoning_effort=COALESCE(model_calls.reasoning_effort,excluded.reasoning_effort),",
    "input_tokens=COALESCE(model_calls.input_tokens,excluded.input_tokens),",
    "output_tokens=COALESCE(model_calls.output_tokens,excluded.output_tokens),",
    "cache_read_tokens=COALESCE(model_calls.cache_read_tokens,excluded.cache_read_tokens),",
    "cache_write_tokens=COALESCE(model_calls.cache_write_tokens,excluded.cache_write_tokens),",
    "reasoning_tokens=COALESCE(model_calls.reasoning_tokens,excluded.reasoning_tokens),",
    "total_tokens=COALESCE(model_calls.total_tokens,excluded.total_tokens),",
    "ttft_millis=COALESCE(model_calls.ttft_millis,excluded.ttft_millis),",
    "decode_millis=COALESCE(model_calls.decode_millis,excluded.decode_millis),",
    "response_millis=COALESCE(model_calls.response_millis,excluded.response_millis),",
    "cost_currency=COALESCE(model_calls.cost_currency,excluded.cost_currency),",
    "cost_amount=COALESCE(model_calls.cost_amount,excluded.cost_amount),",
    "has_unpriced_usage=MAX(model_calls.has_unpriced_usage,excluded.has_unpriced_usage),",
    "body_ref=COALESCE(model_calls.body_ref,excluded.body_ref),",
    "billing_ref=COALESCE(model_calls.billing_ref,excluded.billing_ref)",
);

const TOOL_CALL_COLUMNS: &str = concat!(
    "thread_id,call_id,turn_id,tool_id,revision,admitted_at,started_at,finished_at,status,",
    "terminal,body_ref",
);

const TOOL_CALL_MERGE: &str = concat!(
    "turn_id=COALESCE(tool_calls.turn_id,excluded.turn_id),",
    "tool_id=COALESCE(tool_calls.tool_id,excluded.tool_id),",
    "revision=MAX(tool_calls.revision,excluded.revision),",
    "admitted_at=MIN(tool_calls.admitted_at,excluded.admitted_at),",
    "started_at=MIN(tool_calls.started_at,excluded.started_at),",
    "finished_at=COALESCE(tool_calls.finished_at,excluded.finished_at),",
    "status=CASE WHEN excluded.terminal>tool_calls.terminal OR (excluded.terminal=tool_calls.terminal AND excluded.revision>=tool_calls.revision) THEN COALESCE(excluded.status,tool_calls.status) ELSE COALESCE(tool_calls.status,excluded.status) END,",
    "terminal=MAX(tool_calls.terminal,excluded.terminal),",
    "body_ref=COALESCE(tool_calls.body_ref,excluded.body_ref)",
);

/// 校验 `model_calls` 的合并后置条件：没有非空源列丢失，`revision`/`terminal` 只进不退。
const MODEL_CALL_VERIFY: &str = "SELECT COUNT(*) FROM legacy.model_calls s \
     JOIN main.model_calls d ON d.thread_id=s.thread_id AND d.call_id=s.call_id \
     WHERE d.revision<s.revision OR d.terminal<s.terminal \
        OR (s.root_thread_id IS NOT NULL AND d.root_thread_id IS NULL) \
        OR (s.turn_id IS NOT NULL AND d.turn_id IS NULL) \
        OR (s.attempt_id IS NOT NULL AND d.attempt_id IS NULL) \
        OR (s.retry_of IS NOT NULL AND d.retry_of IS NULL) \
        OR (s.finished_at IS NOT NULL AND d.finished_at IS NULL) \
        OR (s.status IS NOT NULL AND d.status IS NULL) \
        OR (s.retention IS NOT NULL AND d.retention IS NULL) \
        OR (s.purpose IS NOT NULL AND d.purpose IS NULL) \
        OR (s.provider_instance_id IS NOT NULL AND d.provider_instance_id IS NULL) \
        OR (s.provider_display_name IS NOT NULL AND d.provider_display_name IS NULL) \
        OR (s.configured_model IS NOT NULL AND d.configured_model IS NULL) \
        OR (s.sent_model IS NOT NULL AND d.sent_model IS NULL) \
        OR (s.reported_model IS NOT NULL AND d.reported_model IS NULL) \
        OR (s.reasoning_effort IS NOT NULL AND d.reasoning_effort IS NULL) \
        OR (s.input_tokens IS NOT NULL AND d.input_tokens IS NULL) \
        OR (s.output_tokens IS NOT NULL AND d.output_tokens IS NULL) \
        OR (s.cache_read_tokens IS NOT NULL AND d.cache_read_tokens IS NULL) \
        OR (s.cache_write_tokens IS NOT NULL AND d.cache_write_tokens IS NULL) \
        OR (s.reasoning_tokens IS NOT NULL AND d.reasoning_tokens IS NULL) \
        OR (s.total_tokens IS NOT NULL AND d.total_tokens IS NULL) \
        OR (s.ttft_millis IS NOT NULL AND d.ttft_millis IS NULL) \
        OR (s.decode_millis IS NOT NULL AND d.decode_millis IS NULL) \
        OR (s.response_millis IS NOT NULL AND d.response_millis IS NULL) \
        OR (s.cost_currency IS NOT NULL AND d.cost_currency IS NULL) \
        OR (s.cost_amount IS NOT NULL AND d.cost_amount IS NULL) \
        OR (s.has_unpriced_usage>d.has_unpriced_usage) \
        OR (s.body_ref IS NOT NULL AND d.body_ref IS NULL) \
        OR (s.billing_ref IS NOT NULL AND d.billing_ref IS NULL)";

const TOOL_CALL_VERIFY: &str = "SELECT COUNT(*) FROM legacy.tool_calls s \
     JOIN main.tool_calls d ON d.thread_id=s.thread_id AND d.call_id=s.call_id \
     WHERE d.revision<s.revision OR d.terminal<s.terminal \
        OR (s.turn_id IS NOT NULL AND d.turn_id IS NULL) \
        OR (s.tool_id IS NOT NULL AND d.tool_id IS NULL) \
        OR (s.finished_at IS NOT NULL AND d.finished_at IS NULL) \
        OR (s.status IS NOT NULL AND d.status IS NULL) \
        OR (s.body_ref IS NOT NULL AND d.body_ref IS NULL)";

/// Test-only: materialize a supported retired per-product call store for migration acceptance tests.
///
/// Uses the *real* current calls schema ([`ensure_calls_schema`], a supported recognizable version in
/// the `1..=CALLS_SCHEMA_VERSION` range) and writes exactly the facts migration must carry over: one
/// terminal model call bound to a real Thread/Turn with nontrivial usage/metadata, its
/// content-addressed body blob and `call_bodies` registration, and a durable watermark. It never runs
/// in production (`#[cfg(test)]`) and never changes any production semantics. Returns the `body_ref`
/// so the caller can assert the body bytes/hash survive into the canonical store.
#[cfg(test)]
pub(crate) async fn write_legacy_call_store_fixture(
    database: &Path,
    thread_id: &str,
    turn_id: &str,
    call_id: &str,
    body: &[u8],
    watermark: i64,
) -> Result<String> {
    if let Some(parent) = database.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }
    let blobs = legacy_call_store_blobs_dir(database);
    tokio::fs::create_dir_all(&blobs).await?;
    let db = connect_calls_database(database).await?;
    let result = async {
        ensure_calls_schema(&db, &blobs).await?;
        let body_ref = pl_core::context::content_hash(body);
        write_blob_bytes(&blobs, &body_ref, body).await?;
        let byte_length = i64::try_from(body.len())?;
        db.execute_raw(statement(
            "INSERT INTO call_bodies(body_ref, byte_length, created_at) VALUES(?,?,?)",
            vec![
                Value::String(Some(body_ref.clone())),
                Value::BigInt(Some(byte_length)),
                Value::BigInt(Some(120)),
            ],
        ))
        .await?;
        db.execute_raw(statement(
            "INSERT INTO model_calls(thread_id, call_id, root_thread_id, turn_id, attempt_id, \
                revision, admitted_at, started_at, finished_at, status, terminal, retention, \
                provider_instance_id, configured_model, sent_model, reported_model, input_tokens, \
                output_tokens, total_tokens, decode_millis, response_millis, has_unpriced_usage, \
                body_ref) VALUES(?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?)",
            vec![
                Value::String(Some(thread_id.to_owned())),
                Value::String(Some(call_id.to_owned())),
                Value::String(Some(thread_id.to_owned())),
                Value::String(Some(turn_id.to_owned())),
                Value::String(Some(call_id.to_owned())),
                Value::BigInt(Some(2)),
                Value::BigInt(Some(100)),
                Value::BigInt(Some(120)),
                Value::BigInt(Some(200)),
                Value::String(Some("completed".to_owned())),
                Value::BigInt(Some(1)),
                Value::String(Some("turn".to_owned())),
                Value::String(Some("provider-fixture".to_owned())),
                Value::String(Some("model-fixture".to_owned())),
                Value::String(Some("model-fixture".to_owned())),
                Value::String(Some("model-fixture".to_owned())),
                Value::BigInt(Some(10)),
                Value::BigInt(Some(20)),
                Value::BigInt(Some(30)),
                Value::BigInt(Some(50)),
                Value::BigInt(Some(80)),
                Value::BigInt(Some(0)),
                Value::String(Some(body_ref.clone())),
            ],
        ))
        .await?;
        db.execute_raw(statement(
            "INSERT INTO call_watermarks(thread_id, admitted_write_seq, durable_write_seq) \
             VALUES(?,?,?)",
            vec![
                Value::String(Some(thread_id.to_owned())),
                Value::BigInt(Some(watermark)),
                Value::BigInt(Some(watermark)),
            ],
        ))
        .await?;
        Ok::<String, anyhow::Error>(body_ref)
    }
    .await;
    close_connection(db, result).await
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use sea_orm::{ConnectionTrait, Value};

    use super::{
        calls_store_fingerprint, connect_calls_database, legacy_call_store_blobs_dir,
        legacy_call_store_fingerprint, legacy_call_store_identity, sidecar_path, statement,
        write_legacy_call_store_fixture,
    };

    #[tokio::test]
    async fn skipped_billing_recovery_reads_only_calls_without_a_durable_billing_body() {
        let root = tempfile::tempdir().unwrap();
        let calls = super::CallsStore::open(&root.path().join("calls.sqlite"))
            .await
            .unwrap();
        for (revision, call_id, billing_ref) in [
            (5_i64, "billed", Some("existing-body")),
            (6_i64, "unbilled", None),
        ] {
            calls
                .writer
                .db
                .execute_raw(statement(
                    "INSERT INTO model_calls(thread_id,call_id,turn_id,attempt_id,revision,admitted_at,started_at,status,terminal,billing_ref) VALUES(?,?,?,?,?,?,?,?,?,?)",
                    vec![
                        "thread".into(),
                        call_id.into(),
                        "turn".into(),
                        call_id.into(),
                        revision.into(),
                        1_i64.into(),
                        1_i64.into(),
                        "committed".into(),
                        1_i64.into(),
                        billing_ref.map(str::to_owned).into(),
                    ],
                ))
                .await
                .unwrap();
        }
        let pending = calls
            .model_call_facts_between("thread", 0, 6, 10)
            .await
            .unwrap();
        assert_eq!(
            pending
                .iter()
                .map(|fact| fact.call_id.as_str())
                .collect::<Vec<_>>(),
            vec!["unbilled"]
        );
        calls.shutdown().await.unwrap();
    }

    fn write_file(path: &Path, bytes: &[u8]) {
        std::fs::create_dir_all(path.parent().expect("path has a parent")).unwrap();
        std::fs::write(path, bytes).unwrap();
    }

    /// A retired store that a crash split across the live source and the byte-preserved archive must
    /// resolve to the same identity as the whole store, while a member present in both locations is
    /// ambiguous and fails closed.
    #[tokio::test]
    async fn fingerprint_resolves_split_source_and_archive() {
        let root = tempfile::tempdir().unwrap();
        let source = root.path().join("studio/calls.sqlite");
        let archived = root.path().join("migrations/session-archive/calls.sqlite");
        write_file(&source, b"main-database-bytes");
        write_file(&sidecar_path(&source, "-wal"), b"wal-bytes");
        write_file(
            &legacy_call_store_blobs_dir(&source).join("blob-aaaa"),
            b"body-bytes",
        );

        let whole = legacy_call_store_fingerprint(&source).await.unwrap();
        let identity = legacy_call_store_identity(&source, Some(&archived))
            .await
            .unwrap();
        assert!(identity.present);
        assert_eq!(identity.fingerprint, whole);

        // A crash between the individual publication renames: only the blobs reached the archive.
        std::fs::create_dir_all(archived.parent().unwrap()).unwrap();
        std::fs::rename(
            legacy_call_store_blobs_dir(&source),
            legacy_call_store_blobs_dir(&archived),
        )
        .unwrap();
        let split = legacy_call_store_identity(&source, Some(&archived))
            .await
            .unwrap();
        assert!(split.present);
        assert_eq!(split.fingerprint, whole);

        // The remaining members follow: fully archived bytes still resolve to the same identity.
        std::fs::rename(&source, &archived).unwrap();
        std::fs::rename(
            sidecar_path(&source, "-wal"),
            sidecar_path(&archived, "-wal"),
        )
        .unwrap();
        let archived_identity = legacy_call_store_identity(&source, Some(&archived))
            .await
            .unwrap();
        assert!(archived_identity.present);
        assert_eq!(archived_identity.fingerprint, whole);

        // A member that exists both live and archived is ambiguous and must not be guessed.
        write_file(&source, b"main-database-bytes");
        assert!(
            legacy_call_store_identity(&source, Some(&archived))
                .await
                .is_err()
        );
    }

    /// An equal-length content mutation of a blob must change the retired-source byte identity.
    #[tokio::test]
    async fn fingerprint_binds_blob_content() {
        let root = tempfile::tempdir().unwrap();
        let source = root.path().join("studio/calls.sqlite");
        write_file(&source, b"main-database-bytes");
        let blob = legacy_call_store_blobs_dir(&source).join("blob-aaaa");
        write_file(&blob, b"AAAA");
        let before = legacy_call_store_fingerprint(&source).await.unwrap();
        std::fs::write(&blob, b"BBBB").unwrap();
        let after = legacy_call_store_fingerprint(&source).await.unwrap();
        assert_ne!(before, after);
    }

    /// The destination identity is logical content, not file bytes: a WAL checkpoint that only rewrites
    /// the physical layout of the same facts keeps it stable, while a changed fact or blob fails closed.
    #[tokio::test]
    async fn destination_fingerprint_is_stable_across_wal_checkpoint_and_binds_content() {
        let root = tempfile::tempdir().unwrap();
        let database = root.path().join("calls/calls.sqlite");
        write_legacy_call_store_fixture(
            &database,
            "thread-fixture",
            "turn-fixture",
            "call-fixture",
            b"body-bytes",
            3,
        )
        .await
        .unwrap();
        let recorded = calls_store_fingerprint(&database).await.unwrap();

        // Rewrite the same facts through WAL and checkpoint them into the main file: only the physical
        // layout moves, so the logical identity is unchanged.
        let db = connect_calls_database(&database).await.unwrap();
        db.query_all_raw(statement("PRAGMA journal_mode=WAL", vec![]))
            .await
            .unwrap();
        db.execute_raw(statement(
            "UPDATE call_watermarks SET durable_write_seq=durable_write_seq WHERE thread_id=?",
            vec![Value::String(Some("thread-fixture".to_owned()))],
        ))
        .await
        .unwrap();
        db.query_all_raw(statement("PRAGMA wal_checkpoint(TRUNCATE)", vec![]))
            .await
            .unwrap();
        db.close().await.unwrap();
        assert_eq!(calls_store_fingerprint(&database).await.unwrap(), recorded);

        // A tampered fact row changes the logical identity.
        let db = connect_calls_database(&database).await.unwrap();
        db.execute_raw(statement(
            "UPDATE model_calls SET body_ref=? WHERE thread_id=? AND call_id=?",
            vec![
                Value::String(Some("sha256:tampered".to_owned())),
                Value::String(Some("thread-fixture".to_owned())),
                Value::String(Some("call-fixture".to_owned())),
            ],
        ))
        .await
        .unwrap();
        db.close().await.unwrap();
        assert_ne!(calls_store_fingerprint(&database).await.unwrap(), recorded);
    }
}
