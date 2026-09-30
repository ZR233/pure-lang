//! 全局调用统计投影（`calls.sqlite`）与版本化 JSONL 滚动调用日志。
//!
//! 调用正文写入 `<calls>/logs/` 下的版本化 JSONL 段文件：单段最大 16 MiB、跨 UTC 自然日轮转、
//! 单条记录 1 MiB 上限（超过显式 `truncated`）、整体保留 7 天且总量不超过 256 MiB。启动、每 15
//! 分钟与每次轮转都会清理过期/超量段；清理失败记录为可重试降级并在下一次追加前阻塞增长，因此日志
//! 不会无限膨胀。段尾半条记录（进程在写入中途退出）在打开时回退到最后一个换行。
//!
//! `calls.sqlite` 只保留有界数据：随段回收的日志索引（`call_log_index`）、按 root/thread/purpose
//! 的累计费用摘要（`call_usage_summary`，由可靠 writer 通过
//! [`CallsStore::replace_session_usage`] 以 revision 绝对投影幂等供给，不依赖将被删除的正文），
//! 以及有界的性能样本（`performance_samples`，最多 [`PERFORMANCE_SAMPLE_LIMIT`] 条）。
//!
//! 单库只有一个逻辑 writer：轻量调用事件进入同一条有界队列，后台批量落盘（JSONL 追加 + SQLite
//! 索引/统计）。队列满或写入失败只记录统计缺口，不阻塞 Thread 的权威历史提交。
//!
//! 打开时会校验 `calls_meta.schema_version`：schema <= 4 的旧库先保全累计摘要与性能样本、再把保留
//! 期正文转换进日志，全部成功后才前移版本并删除旧结构；schema 5 修复遗留样本外键，并从保留
//! 日志重建性能投影。未来版本显式失败并保留原字节，不触碰用户 home 下的旧数据根。

use std::collections::{BTreeSet, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use anyhow::{Result, bail, ensure};
use pl_core::model::ModelUsage;
use pl_core::thread::journal::AttemptUpdate;
use pl_core::thread::{AttemptOutcome, ThreadEffectBatch};
use pl_protocol::{InferenceBillingRecord, RuntimeCostAmount};
use sea_orm::sqlx::sqlite::{SqliteJournalMode, SqliteSynchronous};
use sea_orm::{
    ConnectOptions, ConnectionTrait, Database, DatabaseBackend, DatabaseConnection,
    DatabaseTransaction, QueryResult, Statement, TransactionTrait, Value,
};
use tokio::sync::{Notify, watch};

use crate::hash::merge_costs;

mod event;
mod log;
mod performance;
mod schema;
mod summary;

pub(crate) use performance::PERFORMANCE_SAMPLE_LIMIT;
pub(crate) use summary::{PurposeUsageProjection, SessionUsageProjection};

use event::CallLogRecord;
use log::{CallLog, LOG_CLEANUP_INTERVAL};

pub(crate) const CALLS_SCHEMA_VERSION: i64 = 6;
/// `calls` 目录下日志段所在子目录名。
const CALL_LOG_DIR_NAME: &str = "logs";
/// 一天对应的秒数；日志轮转/保留期以 UTC 自然日为准。
pub(super) const SECONDS_PER_DAY: i64 = 86_400;
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

/// 单个 root 会话的费用聚合投影（由调用库摘要聚合得到）。
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

/// 受理一次调用写入的轻量工作单元。
enum CallMutation {
    /// 一次 effect：可能携带一条轻量 attempt 记录；无论是否携带都要推进该 Thread 的 durable 水位。
    Effect {
        thread_id: String,
        sequence: i64,
        records: Vec<CallLogRecord>,
    },
    Billing(Box<CallLogRecord>),
    Summary(SessionUsageProjection),
}

impl CallMutation {
    /// 估算条目保留字节，用于队列与批次的压力预算；只序列化轻量记录。
    fn estimated_bytes(&self) -> usize {
        match self {
            Self::Effect { records, .. } => {
                128 + records.iter().map(event::estimate_bytes).sum::<usize>()
            }
            Self::Billing(record) => event::estimate_bytes(record),
            Self::Summary(projection) => {
                serde_json::to_vec(projection).map_or(256, |bytes| bytes.len())
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

/// 单一逻辑 writer 的共享状态：唯一 SQLite 连接、唯一滚动日志、唯一队列与水位。
struct CallsWriter {
    db: DatabaseConnection,
    log: tokio::sync::Mutex<CallLog>,
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

/// 全局调用库句柄；clone 共享同一条队列、日志与后台 writer。
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
        let root_dir = path
            .parent()
            .unwrap_or_else(|| Path::new("."))
            .to_path_buf();
        let mut log = CallLog::open(root_dir.join(CALL_LOG_DIR_NAME)).await?;
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
        schema::initialize(&db, &mut log, &root_dir).await?;
        let mut startup_error = None;
        if let Err(error) = log.rebuild_index(&db).await {
            startup_error = Some(error.to_string());
            tracing::warn!(%error, "调用日志索引重建失败，诊断降级");
        }
        // 启动清理尽力而为：失败记录降级，周期任务会重试。
        if let Err(error) = maintenance(&db, &mut log).await {
            startup_error = Some(error.to_string());
            tracing::warn!(error = %error, "调用库启动清理失败，稍后重试");
        }
        let (durable_ticket, _) = watch::channel(0u64);
        let writer = Arc::new(CallsWriter {
            db,
            log: tokio::sync::Mutex::new(log),
            queue: Mutex::new(VecDeque::new()),
            work_notify: Notify::new(),
            retry_notify: Notify::new(),
            admitted_ticket: AtomicU64::new(0),
            durable_ticket,
            in_flight_bytes: AtomicU64::new(0),
            stopping: AtomicBool::new(false),
            statistics_gap: AtomicBool::new(startup_error.is_some()),
            last_error: Mutex::new(startup_error),
            #[cfg(test)]
            panic_next_mutation: AtomicBool::new(false),
        });
        tokio::spawn(supervise_writer(writer.clone()));
        Ok(Self { writer })
    }

    /// 用可靠 writer 的 revision 绝对投影替换某个 root/thread 的累计费用摘要。
    ///
    /// 主代理从 Thread 折叠出的权威累计值（含 purpose 拆分）调用本接口；更小的 revision 被忽略，
    /// 因此重复投递同一投影是幂等的，且摘要不依赖将被删除的日志正文。
    pub(crate) fn replace_session_usage(&self, projection: SessionUsageProjection) -> Result<()> {
        ensure!(
            !self.writer.stopping.load(Ordering::Acquire),
            "call writer is stopping"
        );
        let mutation = CallMutation::Summary(projection);
        let bytes = mutation.estimated_bytes();
        {
            let mut queue = self
                .writer
                .queue
                .lock()
                .unwrap_or_else(|error| error.into_inner());
            if let CallMutation::Summary(newest) = &mutation {
                queue.retain(|entry| !matches!(&entry.mutation, CallMutation::Summary(previous) if previous.thread_id == newest.thread_id && previous.revision <= newest.revision));
            }
            ensure!(
                queue.len() < MAX_QUEUE_MUTATIONS
                    && queue_bytes(&queue).saturating_add(bytes) <= MAX_QUEUE_BYTES,
                "call summary queue is full"
            );
            queue.push_back(QueuedCallMutation {
                ticket: self.next_ticket(),
                accepted_at: tokio::time::Instant::now(),
                bytes,
                mutation,
            });
        }
        self.writer.work_notify.notify_one();
        Ok(())
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
    ///
    /// 这里只从 effect 提取轻量调用记录，绝不 clone/encode 整个 [`ThreadEffectBatch`]，
    /// 也不把 context / tools 带进队列。
    pub(crate) fn try_admit_effect(&self, effect: &ThreadEffectBatch) -> bool {
        if self.writer.stopping.load(Ordering::Acquire) {
            record_last_error(
                &self.writer,
                "call statistics writer is stopping".to_owned(),
            );
            return false;
        }
        let sequence = i64::try_from(effect.sequence).unwrap_or(i64::MAX);
        let mut records: Vec<_> = effect
            .attempt
            .as_ref()
            .map(|attempt| {
                event::attempt_record(&effect.thread_id, sequence, effect.committed_at, attempt)
            })
            .into_iter()
            .collect();
        records.extend(effect.deliveries.iter().map(|delivery| {
            event::tool_record(
                &effect.thread_id,
                effect
                    .tasks
                    .iter()
                    .find(|task| task.call_id == delivery.call_id)
                    .map(|task| task.turn_id.clone()),
                sequence,
                effect.committed_at,
                delivery,
            )
        }));
        let mutation = CallMutation::Effect {
            thread_id: effect.thread_id.clone(),
            sequence,
            records,
        };
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
        status: CallStatus,
        retention: CallRetention,
    ) -> Result<u64> {
        if self.writer.stopping.load(Ordering::Acquire) {
            self.writer.statistics_gap.store(true, Ordering::Release);
            bail!("call writer is stopping");
        }
        let record = event::billing_record(root_thread_id, thread_id, retention, billing, status);
        let mutation = CallMutation::Billing(Box::new(record));
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

    /// 按 root 会话聚合费用；摘要由可靠 writer 供给，不依赖已被清理的日志正文。
    pub(crate) async fn legacy_auxiliary_usage(
        &self,
        thread_id: &str,
    ) -> Result<pl_core::thread::UsageSummary> {
        match self
            .writer
            .db
            .query_one_raw(statement(
                "SELECT payload FROM legacy_auxiliary_usage WHERE thread_id=?",
                vec![thread_id.into()],
            ))
            .await?
        {
            Some(row) => Ok(serde_json::from_str(
                &row.try_get::<String>("", "payload")?,
            )?),
            None => Ok(Default::default()),
        }
    }

    /// Migration-only baseline, retained independently of disposable diagnostic logs.
    pub(crate) async fn legacy_session_costs(
        &self,
        thread_id: &str,
        fence: u64,
    ) -> Result<Vec<PurposeUsageProjection>> {
        let watermark = self.durable_effect_sequence(thread_id).await?;
        ensure!(
            watermark <= fence,
            "legacy call watermark is ahead of checkpoint"
        );
        let rows = self.writer.db.query_all_raw(statement("SELECT purpose,cost_currency,amount FROM call_usage_summary WHERE thread_id=? AND revision=0 AND cost_currency<>''", vec![thread_id.into()])).await?;
        let mut purposes =
            std::collections::BTreeMap::<Option<String>, Vec<RuntimeCostAmount>>::new();
        for row in rows {
            let purpose: String = row.try_get("", "purpose")?;
            purposes
                .entry((!purpose.is_empty()).then_some(purpose))
                .or_default()
                .push(RuntimeCostAmount {
                    currency: row.try_get("", "cost_currency")?,
                    amount: row.try_get("", "amount")?,
                });
        }
        Ok(purposes
            .into_iter()
            .map(|(purpose, estimated_costs)| PurposeUsageProjection {
                purpose,
                estimated_costs,
            })
            .collect())
    }

    pub(crate) async fn session_cost_rollups(&self) -> Result<Vec<SessionCostRollup>> {
        summary::read_rollups(&self.writer.db).await
    }

    /// 调用明细只读取仍在保留期的日志；性能摘要的 3000 条上限不决定明细生命周期。
    pub(crate) async fn recent_performance_samples(
        &self,
        limit: u32,
    ) -> Result<Vec<PerformanceSampleRow>> {
        let log = self.writer.log.lock().await;
        let rows = self
            .writer
            .db
            .query_all_raw(statement(
                "SELECT segment,offset,length,content_hash,recorded_at FROM call_log_index
             WHERE kind IN ('billing','migrated') AND recorded_at>?
             ORDER BY recorded_at DESC,thread_id DESC,call_id DESC LIMIT ?",
                vec![
                    (crate::studio::unix_seconds() - log::LOG_RETENTION_SECONDS).into(),
                    i64::from(limit).into(),
                ],
            ))
            .await?;
        let mut samples = Vec::with_capacity(rows.len());
        for row in rows {
            let location = log::AppendedRecord {
                segment: row.try_get("", "segment")?,
                offset: u64::try_from(row.try_get::<i64>("", "offset")?)?,
                length: u64::try_from(row.try_get::<i64>("", "length")?)?,
            };
            let record = log
                .read(
                    &location,
                    &row.try_get::<String>("", "content_hash")?,
                    row.try_get("", "recorded_at")?,
                )
                .await?;
            if record.status != CallStatus::Committed.as_str() {
                continue;
            }
            samples.push(PerformanceSampleRow {
                completed_at: record.recorded_at,
                provider_instance_id: record.provider_instance_id.unwrap_or_default(),
                provider_display_name: record.provider_display_name.unwrap_or_default(),
                configured_model: record.configured_model,
                sent_model: record.sent_model.unwrap_or_default(),
                reported_model: record.reported_model,
                reasoning_effort: record.reasoning_effort,
                completion_tokens: record
                    .usage
                    .and_then(|usage| usage.output_tokens)
                    .unwrap_or(0),
                ttft_millis: record.timing.map(|timing| timing.ttft_millis),
                decode_millis: record.timing.map(|timing| timing.decode_millis),
                response_millis: record.timing.map(|timing| timing.response_millis),
            });
        }
        Ok(samples)
    }

    /// 按 provider instance/发送模型/effort 聚合性能汇总（数据库聚合投影）。
    pub(crate) async fn performance_summary_rows(&self) -> Result<Vec<PerformanceSummaryRow>> {
        let rows = self
            .writer
            .db
            .query_all_raw(statement(
                "SELECT provider_instance_id,
                        COALESCE(MAX(provider_display_name), '') AS provider_display_name,
                        sent_model,
                        reasoning_effort,
                        COUNT(*) AS sample_count,
                        COALESCE(SUM(output_tokens), 0) AS completion_tokens,
                        COALESCE(SUM(ttft_millis), 0) AS total_ttft_millis,
                        COALESCE(SUM(decode_millis), 0) AS total_decode_millis,
                        COALESCE(SUM(response_millis), 0) AS total_response_millis
                 FROM performance_samples
                 WHERE provider_instance_id IS NOT NULL AND sent_model IS NOT NULL
                   AND decode_millis > 0 AND output_tokens IS NOT NULL
                 GROUP BY provider_instance_id, sent_model, reasoning_effort
                 ORDER BY provider_instance_id, sent_model,
                          reasoning_effort IS NOT NULL, reasoning_effort",
                vec![],
            ))
            .await?;
        rows.iter().map(performance_summary_row).collect()
    }
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

/// 唯一的调用库写入循环：取批、应用、重试、推进水位，并周期性清理日志。
async fn run_writer(shared: Arc<CallsWriter>) {
    let mut retries = 0usize;
    // 周期清理以绝对截止时间为准，即使队列一直非空（写入/重试未停）也会到点清理，
    // 避免文档承诺的“每 15 分钟清理”被持续压力饿死。
    let mut next_cleanup = tokio::time::Instant::now() + LOG_CLEANUP_INTERVAL;
    loop {
        if tokio::time::Instant::now() >= next_cleanup {
            next_cleanup = tokio::time::Instant::now() + LOG_CLEANUP_INTERVAL;
            let mut log = shared.log.lock().await;
            if let Err(error) = maintenance(&shared.db, &mut log).await {
                record_last_error(&shared, format!("call log cleanup failed: {error}"));
            }
        }
        let stopping = shared.stopping.load(Ordering::Acquire);
        let pressure = queue_pressure(&shared);
        if pressure.mutations == 0 {
            if stopping {
                return;
            }
            tokio::select! {
                _ = shared.work_notify.notified() => continue,
                // 到点后唤醒执行顶部截止时间检查，而不是在空闲分支里重复清理逻辑。
                _ = tokio::time::sleep_until(next_cleanup) => continue,
            }
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
        let deferred = match apply_batch(&shared, &pending).await {
            Ok((completed, failures, deferred)) => {
                if completed > 0 {
                    retries = 0;
                    clear_last_error(&shared);
                    for failure in failures {
                        fail_mutation(&shared, &failure);
                    }
                    let ticket = pending[completed - 1].ticket;
                    pending.drain(..completed);
                    // No receipt may outrun the commit of the outer transaction.
                    advance_durable(&shared, ticket);
                }
                deferred
            }
            Err(error) => Some(classify_failure(error)),
        };
        // 本批已处理完（含退回队首的 deferred 条目），不再有 in-flight 字节。
        shared.in_flight_bytes.store(0, Ordering::Release);
        let Some(failure) = deferred else {
            continue;
        };
        if !failure.retryable || shared.stopping.load(Ordering::Acquire) {
            fail_mutation(&shared, &failure.message);
            if let Some(last) = pending.back() {
                advance_durable(&shared, last.ticket);
            }
            continue;
        }
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

/// 清理日志并回收索引；周期任务与启动路径共用。
async fn maintenance(db: &DatabaseConnection, log: &mut CallLog) -> Result<()> {
    let outcome = log.cleanup(crate::studio::unix_seconds()).await;
    if !outcome.removed.is_empty() {
        let tx = db.begin().await?;
        prune_index(&tx, &outcome.removed).await?;
        tx.commit().await?;
    }
    if let Some(failure) = outcome.failure {
        bail!("call log cleanup incomplete: {failure}");
    }
    let root_dir = log.root_dir();
    schema::retry_pending_cleanup(db, &root_dir).await?;
    Ok(())
}

/// One disk commit per bounded batch. Each mutation uses a savepoint, so a bad statistics fact
/// cannot poison the remaining valid facts; a transient failure leaves that suffix queued.
async fn apply_batch(
    shared: &CallsWriter,
    pending: &VecDeque<QueuedCallMutation>,
) -> Result<(usize, Vec<String>, Option<BatchFailure>)> {
    let mut log = shared.log.lock().await;
    let tx = shared.db.begin().await?;
    let mut completed = 0;
    let mut failures = Vec::new();
    let mut deferred = None;
    for entry in pending {
        #[cfg(test)]
        if shared.panic_next_mutation.swap(false, Ordering::AcqRel) {
            panic!("injected call statistics consumer exit");
        }
        match apply_mutation(&mut log, &tx, &entry.mutation).await {
            Ok(()) => {}
            Err(failure) if failure.retryable => {
                deferred = Some(failure);
                break;
            }
            Err(failure) => failures.push(failure.message),
        }
        completed += 1;
    }
    performance::trim(&tx).await?;
    tx.commit().await?;
    Ok((completed, failures, deferred))
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

async fn apply_mutation(
    log: &mut CallLog,
    db: &DatabaseTransaction,
    mutation: &CallMutation,
) -> Result<(), BatchFailure> {
    let result = match mutation {
        CallMutation::Effect {
            thread_id,
            sequence,
            records,
        } => apply_effect(log, db, thread_id, *sequence, records).await,
        CallMutation::Billing(record) => apply_billing(log, db, record).await,
        CallMutation::Summary(projection) => summary::replace(db, projection).await,
    };
    result.map_err(classify_failure)
}

fn classify_failure(error: anyhow::Error) -> BatchFailure {
    let message = error.to_string();
    BatchFailure {
        retryable: is_retryable_write(&message),
        message,
    }
}

/// 把一次 effect 的调用事实写入调用库；调用开始与终态更新共享同一调用身份。
async fn apply_effect(
    log: &mut CallLog,
    db: &DatabaseTransaction,
    thread_id: &str,
    sequence: i64,
    records: &[CallLogRecord],
) -> Result<()> {
    let tx = db.begin().await?;
    let current = tx
        .query_one_raw(statement(
            "SELECT durable_write_seq FROM call_watermarks WHERE thread_id=?",
            vec![thread_id.into()],
        ))
        .await?
        .map(|row| row.try_get::<i64>("", "durable_write_seq"))
        .transpose()?
        .unwrap_or(0);
    if sequence <= current {
        tx.rollback().await?;
        return Ok(());
    }
    for record in records {
        apply_attempt(&tx, log, record).await?;
    }
    tx.execute_raw(statement(
        "INSERT INTO call_watermarks(thread_id,admitted_write_seq,durable_write_seq)
         VALUES(?,?,?)
         ON CONFLICT(thread_id) DO UPDATE SET
            admitted_write_seq=MAX(excluded.admitted_write_seq,call_watermarks.admitted_write_seq),
            durable_write_seq=MAX(excluded.durable_write_seq,call_watermarks.durable_write_seq)",
        vec![thread_id.into(), sequence.into(), sequence.into()],
    ))
    .await?;
    tx.commit().await?;
    Ok(())
}

/// attempt 记录按 revision 单调：更旧的修订不覆盖已索引的更新。
async fn apply_attempt(
    db: &DatabaseTransaction,
    log: &mut CallLog,
    record: &CallLogRecord,
) -> Result<()> {
    let existing = db
        .query_one_raw(statement(
            "SELECT revision FROM call_log_index WHERE thread_id=? AND call_id=? AND kind=?",
            vec![
                record.thread_id.clone().into(),
                record.call_id.clone().into(),
                record.kind.clone().into(),
            ],
        ))
        .await?;
    if let Some(row) = existing {
        let stored: i64 = row.try_get("", "revision")?;
        if record.revision <= stored {
            return Ok(());
        }
    }
    let now = crate::studio::unix_seconds();
    append_record(db, log, record, now).await
}

/// 幂等写入一次计费/性能事实；同一身份不同正文明确失败。
async fn apply_billing(
    log: &mut CallLog,
    db: &DatabaseTransaction,
    record: &CallLogRecord,
) -> Result<()> {
    let (line, _) = event::encode_record(record)?;
    let content_hash = pl_core::context::content_hash(line.as_bytes());
    let existing = db
        .query_one_raw(statement(
            "SELECT content_hash FROM call_log_index WHERE thread_id=? AND call_id=? AND kind=?",
            vec![
                record.thread_id.clone().into(),
                record.call_id.clone().into(),
                record.kind.clone().into(),
            ],
        ))
        .await?;
    if let Some(row) = existing {
        let stored: Option<String> = row.try_get("", "content_hash")?;
        match stored {
            Some(stored) if stored == content_hash => return Ok(()),
            Some(_) => {
                bail!(
                    "model call {} conflicts with the durable call record",
                    record.call_id
                );
            }
            None => {}
        }
    }
    let now = crate::studio::unix_seconds();
    append_record(db, log, record, now).await?;
    if record.status == CallStatus::Committed.as_str() {
        let sample = performance::sample_from_record(record);
        performance::record(db, &sample).await?;
    }
    Ok(())
}

/// 追加一条记录到 JSONL 日志并在 SQLite 索引中登记位置；顺带回收被清理段的索引行。
async fn append_record(
    db: &impl ConnectionTrait,
    log: &mut CallLog,
    record: &CallLogRecord,
    now: i64,
) -> Result<()> {
    if now.saturating_sub(record.recorded_at) >= log::LOG_RETENTION_SECONDS {
        return Ok(());
    }
    let (line, truncated) = event::encode_record(record)?;
    let outcome = log
        .append(event::record_day(record.recorded_at), now, &line)
        .await?;
    prune_index(db, &outcome.removed).await?;
    index_record(db, record, &line, &outcome.record, truncated).await
}

async fn index_record(
    db: &impl ConnectionTrait,
    record: &CallLogRecord,
    line: &str,
    location: &log::AppendedRecord,
    truncated: bool,
) -> Result<()> {
    let content_hash = pl_core::context::content_hash(line.as_bytes());
    db.execute_raw(statement(
        "INSERT INTO call_log_index(
            thread_id,call_id,kind,revision,content_hash,segment,offset,length,truncated,recorded_at)
         VALUES(?,?,?,?,?,?,?,?,?,?)
         ON CONFLICT(thread_id,call_id,kind) DO UPDATE SET
            revision=MAX(excluded.revision,call_log_index.revision),
            content_hash=excluded.content_hash,
            segment=excluded.segment,
            offset=excluded.offset,
            length=excluded.length,
            truncated=excluded.truncated,
            recorded_at=excluded.recorded_at WHERE excluded.revision >= call_log_index.revision",
        vec![
            record.thread_id.clone().into(),
            record.call_id.clone().into(),
            record.kind.clone().into(),
            record.revision.into(),
            content_hash.into(),
            location.segment.clone().into(),
            integer(location.offset)?.into(),
            integer(location.length)?.into(),
            (truncated as i32).into(),
            record.recorded_at.into(),
        ],
    ))
    .await?;
    Ok(())
}

/// 删除被清理段的索引行。
async fn prune_index(db: &impl ConnectionTrait, segments: &[String]) -> Result<()> {
    for segment in segments {
        db.execute_raw(statement(
            "DELETE FROM call_log_index WHERE segment=?",
            vec![segment.clone().into()],
        ))
        .await?;
    }
    Ok(())
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

fn opt_i64(value: u64) -> Option<i64> {
    i64::try_from(value).ok()
}

fn required_u64(row: &QueryResult, column: &str) -> Result<u64> {
    let value: Option<i64> = row.try_get("", column)?;
    Ok(value
        .and_then(|value| u64::try_from(value).ok())
        .unwrap_or(0))
}

fn statement(sql: &str, values: Vec<Value>) -> Statement {
    Statement::from_sql_and_values(DatabaseBackend::Sqlite, sql, values)
}

fn integer(value: u64) -> Result<i64> {
    value
        .try_into()
        .map_err(|_| anyhow::anyhow!("call write sequence exceeds SQLite range"))
}

/// 只有数据库忙/锁/IO 类错误允许自动重试；结构或约束错误必须显式回给调用方。
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

#[cfg(test)]
mod storage_fault_tests {
    use super::*;

    #[tokio::test]
    async fn unwritable_diagnostics_do_not_starve_absolute_accounting() -> Result<()> {
        let temp = tempfile::tempdir()?;
        tokio::fs::write(temp.path().join("logs"), b"blocked directory").await?;
        let store = CallsStore::open(&temp.path().join("calls.sqlite")).await?;
        let record = CallLogRecord {
            version: event::CALL_LOG_RECORD_VERSION,
            kind: "billing".into(),
            thread_id: "thread".into(),
            call_id: "call".into(),
            status: "committed".into(),
            recorded_at: crate::studio::unix_seconds(),
            ..Default::default()
        };
        let mutations = [
            CallMutation::Billing(Box::new(record)),
            CallMutation::Summary(SessionUsageProjection {
                root_thread_id: "thread".into(),
                thread_id: "thread".into(),
                revision: 7,
                has_unpriced_usage: false,
                purpose_costs: vec![PurposeUsageProjection {
                    purpose: None,
                    estimated_costs: vec![RuntimeCostAmount {
                        currency: "USD".into(),
                        amount: 3.0,
                    }],
                }],
            }),
        ];
        let pending = mutations
            .into_iter()
            .enumerate()
            .map(|(index, mutation)| QueuedCallMutation {
                ticket: index as u64 + 1,
                accepted_at: tokio::time::Instant::now(),
                bytes: mutation.estimated_bytes(),
                mutation,
            })
            .collect();
        let (completed, failures, deferred) = apply_batch(&store.writer, &pending).await?;
        assert_eq!(completed, 2);
        assert_eq!(failures.len(), 1);
        assert!(deferred.is_none());
        assert!(store.statistics_gap());
        assert_eq!(
            store.session_cost_rollups().await?[0].estimated_costs[0].amount,
            3.0
        );
        store.stop_best_effort();
        Ok(())
    }

    #[tokio::test]
    async fn batch_savepoint_failure_preserves_other_calls_and_commits_before_receipts()
    -> Result<()> {
        let temp = tempfile::tempdir()?;
        let store = CallsStore::open(&temp.path().join("calls.sqlite")).await?;
        store
            .writer
            .db
            .execute_unprepared(
                "CREATE TABLE failure_probe(thread_id TEXT NOT NULL);
                 CREATE TRIGGER reject_one BEFORE INSERT ON call_watermarks
                 WHEN NEW.thread_id='rejected' BEGIN
                   INSERT INTO failure_probe VALUES(NEW.thread_id);
                   SELECT RAISE(FAIL,'injected invalid call');
                 END;",
            )
            .await?;
        let pending: VecDeque<QueuedCallMutation> = ["first", "rejected", "last"]
            .into_iter()
            .enumerate()
            .map(|(index, thread)| {
                let mutation = CallMutation::Effect {
                    thread_id: thread.to_owned(),
                    sequence: 1,
                    records: Vec::new(),
                };
                QueuedCallMutation {
                    ticket: index as u64 + 1,
                    accepted_at: tokio::time::Instant::now(),
                    bytes: mutation.estimated_bytes(),
                    mutation,
                }
            })
            .collect();
        let (completed, failures, deferred) = apply_batch(&store.writer, &pending).await?;
        assert_eq!(completed, 3);
        assert_eq!(failures.len(), 1);
        assert!(failures[0].contains("injected invalid call"));
        assert!(deferred.is_none());
        // An independent connection observes both committed facts before a receipt is advanced.
        let reopened = CallsStore::open(&temp.path().join("calls.sqlite")).await?;
        let rows = reopened
            .writer
            .db
            .query_all_raw(statement(
                "SELECT thread_id FROM call_watermarks ORDER BY thread_id",
                vec![],
            ))
            .await?;
        let ids = rows
            .iter()
            .map(|row| row.try_get::<String>("", "thread_id"))
            .collect::<std::result::Result<Vec<_>, _>>()?;
        assert_eq!(ids, ["first", "last"]);
        // RAISE(FAIL) preserves earlier trigger writes unless the mutation's savepoint rolls back.
        let failed_writes = reopened
            .writer
            .db
            .query_all_raw(statement("SELECT thread_id FROM failure_probe", vec![]))
            .await?;
        assert!(failed_writes.is_empty());
        assert_eq!(*store.writer.durable_ticket.borrow(), 0);
        store.stop_best_effort();
        reopened.stop_best_effort();
        Ok(())
    }
}
