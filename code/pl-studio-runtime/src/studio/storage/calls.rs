//! 全局调用统计投影（`calls.sqlite`）；Thread 历史与工具任务由会话库负责。
//!
//! 每次实际 attempt 的调用身份、状态、时延、token 与价格摘要按尽力而为写入；
//! 正文以内容寻址 calls blob 文件保存，数据库只保留引用。
//!
//! 单库只有一个逻辑 writer：effect 与计费观察进入同一条有界队列，后台批量落库。
//! 统计队列满或写入失败只记录统计缺口，不阻塞 Thread 的权威历史提交。
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
use pl_core::thread::{AttemptOutcome, ThreadEffectBatch};
use pl_protocol::{InferenceBillingRecord, RuntimeCostAmount};
use sea_orm::sqlx::sqlite::{SqliteJournalMode, SqliteSynchronous};
use sea_orm::{
    ConnectOptions, ConnectionTrait, Database, DatabaseBackend, DatabaseConnection, QueryResult,
    Statement, TransactionTrait, Value,
};
use tokio::sync::{Notify, watch};

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

/// 调用结果类别；持久化为稳定字符串。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CallStatus {
    Running,
    Committed,
    Rejected,
    Cancelled,
    Failed,
    Interrupted,
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
        }
    }

    pub(crate) const fn is_terminal(self) -> bool {
        !matches!(self, Self::Running)
    }
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

/// 一条成功调用；未测得的时延不能伪装成零时延。
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
    pub(crate) ttft_millis: Option<u64>,
    pub(crate) decode_millis: Option<u64>,
    pub(crate) response_millis: Option<u64>,
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

struct QueuedCallMutation {
    ticket: u64,
    accepted_at: tokio::time::Instant,
    bytes: usize,
    mutation: CallMutation,
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
    admitted_ticket: AtomicU64,
    durable_ticket: watch::Sender<u64>,
    /// Encoded bytes of the batch currently being written; a read-only pressure observation.
    in_flight_bytes: AtomicU64,
    stopping: AtomicBool,
    last_error: Mutex<Option<String>>,
    statistics_gap: AtomicBool,
    #[cfg(test)]
    panic_next_mutation: AtomicBool,
}

/// 全局调用库句柄；clone 共享同一条队列、水位与后台 writer。
#[derive(Clone)]
pub(crate) struct CallsStore {
    writer: Arc<CallsWriter>,
}

impl CallsStore {
    pub(crate) fn metrics(&self) -> super::coordinator::CallsQueueMetrics {
        super::coordinator::CallsQueueMetrics {
            admitted_sequence: self.admitted_ticket(),
            durable_sequence: self.durable_ticket(),
            pending_operations: u64::try_from(self.pending_count()).unwrap_or(u64::MAX),
            pending_bytes: u64::try_from(self.pending_bytes()).unwrap_or(u64::MAX),
            in_flight_bytes: u64::try_from(self.in_flight_bytes()).unwrap_or(u64::MAX),
            oldest_pending_age_millis: self
                .oldest_pending_age()
                .map(|age| u64::try_from(age.as_millis()).unwrap_or(u64::MAX)),
            last_error: self.last_error(),
            pressure_paused: self.pressure_paused(),
            statistics_gap: self.statistics_gap(),
        }
    }

    pub(crate) fn statistics_gap(&self) -> bool {
        self.writer.statistics_gap.load(Ordering::Acquire)
    }

    pub(crate) fn mark_statistics_gap(&self) {
        self.writer.statistics_gap.store(true, Ordering::Release);
    }

    #[cfg(test)]
    pub(crate) fn panic_on_next_mutation(&self) {
        self.writer
            .panic_next_mutation
            .store(true, Ordering::Release);
    }
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
            .max_connections(2)
            .min_connections(2)
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
            admitted_ticket: AtomicU64::new(0),
            durable_ticket,
            in_flight_bytes: AtomicU64::new(0),
            stopping: AtomicBool::new(false),
            last_error: Mutex::new(None),
            statistics_gap: AtomicBool::new(false),
            #[cfg(test)]
            panic_next_mutation: AtomicBool::new(false),
        });
        tokio::spawn(supervise_writer(writer.clone()));
        Ok(Self { writer })
    }

    /// 当前已受理的最高统计 ticket，供持久化进度观测。
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

    pub(crate) fn subscribe_durable_ticket(&self) -> watch::Receiver<u64> {
        self.writer.durable_ticket.subscribe()
    }

    /// Reads the committed effect watermark for one Thread, independently of global queue tickets.
    pub(crate) async fn durable_effect_sequence(&self, thread_id: &str) -> Result<u64> {
        let row = self
            .writer
            .db
            .query_one_raw(statement(
                "SELECT durable_write_seq FROM call_watermarks WHERE thread_id=?",
                vec![thread_id.into()],
            ))
            .await?;
        row.map(|row| {
            let value: i64 = row.try_get("", "durable_write_seq")?;
            u64::try_from(value).map_err(Into::into)
        })
        .transpose()
        .map(|value| value.unwrap_or(0))
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

    /// Best-effort call statistics. Full task identities and deliveries belong to history.sqlite.
    /// An unavailable/full statistics queue never blocks the executing Thread.
    pub(crate) fn try_admit_effect(&self, effect: &ThreadEffectBatch) -> bool {
        if self.writer.stopping.load(Ordering::Acquire) {
            record_last_error(
                &self.writer,
                "call statistics writer is stopping".to_owned(),
            );
            return false;
        }
        let mutation = CallMutation::Effect(Box::new(effect.clone()));
        let bytes = mutation.estimated_bytes();
        let admitted = {
            let mut queue = self
                .writer
                .queue
                .lock()
                .unwrap_or_else(|error| error.into_inner());
            if queue.len() >= MAX_QUEUE_MUTATIONS
                || queue_bytes(&queue).saturating_add(bytes) > MAX_QUEUE_BYTES
            {
                false
            } else {
                let ticket = self.next_ticket();
                queue.push_back(QueuedCallMutation {
                    ticket,
                    accepted_at: tokio::time::Instant::now(),
                    bytes,
                    mutation,
                });
                true
            }
        };
        if admitted {
            self.writer.work_notify.notify_one();
        } else {
            record_last_error(
                &self.writer,
                "call statistics queue is full; statistics gap".to_owned(),
            );
        }
        admitted
    }

    /// 同步受理一次计费观察并把其 ticket 交回调用方。
    ///
    /// 受理只入队、不等待 durability。队列满或写入器停机时返回错误，
    /// 调用方记录统计缺口而不阻塞 Thread 执行。
    pub(crate) fn admit_billing(
        &self,
        root_thread_id: &str,
        thread_id: &str,
        billing: &InferenceBillingRecord,
        retention: CallRetention,
    ) -> Result<u64> {
        if self.writer.stopping.load(Ordering::Acquire) {
            self.writer.statistics_gap.store(true, Ordering::Release);
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
            if queue.len() >= MAX_QUEUE_MUTATIONS
                || queue_bytes(&queue).saturating_add(bytes) > MAX_QUEUE_BYTES
            {
                self.writer.statistics_gap.store(true, Ordering::Release);
                bail!("call writer queue is full");
            }
            let ticket = self.next_ticket();
            queue.push_back(QueuedCallMutation {
                ticket,
                accepted_at: tokio::time::Instant::now(),
                bytes,
                mutation,
            });
            ticket
        };
        self.writer.work_notify.notify_one();
        Ok(ticket)
    }

    /// Stop accepting statistics without waiting for the optional projection to drain.
    pub(crate) fn stop_best_effort(&self) {
        if self.pending_count() > 0 || self.in_flight_bytes() > 0 {
            self.mark_statistics_gap();
        }
        self.writer.stopping.store(true, Ordering::Release);
        self.writer.work_notify.notify_one();
        self.writer.retry_notify.notify_one();
    }

    fn next_ticket(&self) -> u64 {
        self.writer
            .admitted_ticket
            .fetch_add(1, Ordering::AcqRel)
            .wrapping_add(1)
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

    /// 读取最近的成功调用（新到旧），包含尚无有效性能计时的记录。
    pub(crate) async fn recent_performance_samples(
        &self,
        limit: u32,
    ) -> Result<Vec<PerformanceSampleRow>> {
        let rows = self
            .writer
            .db
            .query_all_raw(statement(
                "SELECT COALESCE(finished_at, started_at) AS completed_at,
                        COALESCE(provider_instance_id, '') AS provider_instance_id,
                        COALESCE(provider_display_name, '') AS provider_display_name,
                        configured_model, COALESCE(sent_model, '') AS sent_model,
                        reported_model, reasoning_effort, output_tokens,
                        ttft_millis, decode_millis, response_millis
                 FROM model_calls
                 WHERE terminal=1 AND status='committed'
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
                 WHERE terminal=1 AND status='committed' AND provider_instance_id IS NOT NULL
                   AND sent_model IS NOT NULL AND decode_millis > 0
                   AND output_tokens IS NOT NULL
                 GROUP BY provider_instance_id, sent_model, reasoning_effort
                 ORDER BY provider_instance_id, sent_model,
                          reasoning_effort IS NOT NULL, reasoning_effort",
                vec![],
            ))
            .await?;
        rows.iter().map(performance_summary_row).collect()
    }
}

async fn initialize(db: &DatabaseConnection, blobs_dir: &Path) -> Result<()> {
    ensure_calls_schema(db, blobs_dir).await
}

/// Ensures the current calls schema exists, upgrading any older revision in place.
///
/// Additive upgrades keep existing v2 facts; a future revision fails closed.
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
/// 队列非空就取一批，批量大小来自并发受理自然堆积的条目。
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
            #[cfg(test)]
            if shared.panic_next_mutation.swap(false, Ordering::AcqRel) {
                panic!("injected call statistics consumer exit");
            }
            let ticket = entry.ticket;
            match apply_mutation(&shared, &entry.mutation).await {
                Ok(()) => {
                    retries = 0;
                    advance_durable(&shared, ticket);
                    clear_last_error(&shared);
                }
                Err(failure) if failure.retryable && shared.stopping.load(Ordering::Acquire) => {
                    // 停机不再退避：未写入事实显式交回等待方，由调用方保留并重试。
                    fail_mutation(
                        &shared,
                        &format!("call writer stopped before writing: {}", failure.message),
                    );
                    advance_durable(&shared, ticket);
                }
                Err(failure) if failure.retryable => {
                    deferred = Some((entry, failure));
                    break;
                }
                Err(failure) => {
                    fail_mutation(&shared, &failure.message);
                    advance_durable(&shared, ticket);
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

fn fail_mutation(shared: &CallsWriter, message: &str) {
    tracing::error!(error = message, "call writer rejected a call fact");
    record_last_error(shared, message.to_owned());
}

fn record_last_error(shared: &CallsWriter, message: String) {
    shared.statistics_gap.store(true, Ordering::Release);
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

async fn apply_mutation(shared: &CallsWriter, mutation: &CallMutation) -> Result<(), BatchFailure> {
    let result = match mutation {
        CallMutation::Effect(effect) => apply_effect(shared, effect).await,
        CallMutation::Billing {
            root_thread_id,
            thread_id,
            retention,
            billing,
        } => apply_billing(shared, root_thread_id, thread_id, billing, *retention).await,
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
) -> Result<()> {
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
                return Ok(());
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
    Ok(())
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
    Ok(())
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
        ttft_millis: row
            .try_get::<Option<i64>>("", "ttft_millis")?
            .map(u64::try_from)
            .transpose()?,
        decode_millis: row
            .try_get::<Option<i64>>("", "decode_millis")?
            .map(u64::try_from)
            .transpose()?,
        response_millis: row
            .try_get::<Option<i64>>("", "response_millis")?
            .map(u64::try_from)
            .transpose()?,
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
