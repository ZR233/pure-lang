//! 版本化 JSONL 滚动调用日志。
//!
//! 单段最大 16 MiB，跨 UTC 自然日轮转；整体保留 7 天且总量不超过 256 MiB。启动、每 15 分钟与
//! 每次轮转都会清理过期/超量段；清理失败记入可重试降级状态，并在下一次追加前阻塞增长，因此日志
//! 不会无限膨胀。段尾半条记录（进程在写入中途退出）在打开时回退到最后一个换行，读取端把不完整尾
//! 行视为缺失。
//!
//! 日志是单个逻辑 writer 的私有资源；它只被 `CallsWriter` 唯一后台任务持有，不跨任务共享。

use super::*;
use tokio::io::{AsyncReadExt, AsyncSeekExt, AsyncWriteExt};

/// 周期性清理间隔。
pub(super) const LOG_CLEANUP_INTERVAL: Duration = Duration::from_secs(15 * 60);
/// 单段字节上限。
const SEGMENT_MAX_BYTES: u64 = 16 * 1024 * 1024;
/// 日志总量上限。
const LOG_MAX_BYTES: u64 = 256 * 1024 * 1024;
/// 保留期；超过即清理。
pub(super) const LOG_RETENTION_SECONDS: i64 = 7 * SECONDS_PER_DAY;
const SEGMENT_PREFIX: &str = "calls-";
const SEGMENT_SUFFIX: &str = ".jsonl";

/// 一次追加在日志中的位置。
#[derive(Debug, Clone)]
pub(super) struct AppendedRecord {
    pub(super) segment: String,
    pub(super) offset: u64,
    pub(super) length: u64,
}

/// 追加结果：位置与被顺带清理掉的段名。
#[derive(Debug, Clone)]
pub(super) struct AppendOutcome {
    pub(super) record: AppendedRecord,
    pub(super) removed: Vec<String>,
}

/// 清理结果；`failure` 非空表示这次清理没有完全成功，调用方应记录可重试降级。
#[derive(Debug, Default)]
pub(super) struct CleanupOutcome {
    pub(super) removed: Vec<String>,
    pub(super) over_capacity: bool,
    pub(super) failure: Option<anyhow::Error>,
}

struct Segment {
    day: i64,
    name: String,
    file: tokio::fs::File,
    bytes: u64,
}

struct SegmentInfo {
    day: i64,
    seq: u64,
    name: String,
    len: u64,
}

/// 单个逻辑 writer 持有的滚动日志。
pub(super) struct CallLog {
    dir: PathBuf,
    current: Option<Segment>,
    total_bytes: u64,
    next_sequence: u64,
    degraded: Option<String>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum OpenPolicy {
    Degraded,
    Strict,
}

#[derive(Debug, thiserror::Error)]
pub(super) enum LogReadError {
    #[error("call diagnostic expired: {0}")]
    Expired(String),
    #[error("call diagnostic is missing: {0}")]
    Missing(String),
    #[error("call diagnostic is damaged: {0}")]
    Damaged(String),
}

impl CallLog {
    /// Read one retained record while holding the writer's log lease, excluding concurrent cleanup.
    pub(super) async fn read(
        &self,
        location: &AppendedRecord,
        hash: &str,
        recorded_at: i64,
    ) -> Result<CallLogRecord> {
        if crate::studio::unix_seconds().saturating_sub(recorded_at) >= LOG_RETENTION_SECONDS {
            return Err(LogReadError::Expired(location.segment.clone()).into());
        }
        ensure!(
            location.length <= event::CALL_LOG_RECORD_MAX_BYTES as u64
                && !location.segment.contains(['/', '\\']),
            "invalid diagnostic location"
        );
        let mut file = match tokio::fs::File::open(self.dir.join(&location.segment)).await {
            Ok(file) => file,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Err(LogReadError::Missing(location.segment.clone()).into());
            }
            Err(error) => return Err(error.into()),
        };
        file.seek(std::io::SeekFrom::Start(location.offset)).await?;
        let mut bytes = vec![0; usize::try_from(location.length)?];
        file.read_exact(&mut bytes).await?;
        let mut ending = [0];
        file.read_exact(&mut ending).await?;
        if ending[0] != b'\n' || pl_core::context::content_hash(&bytes) != hash {
            return Err(LogReadError::Damaged(location.segment.clone()).into());
        }
        let record: CallLogRecord = serde_json::from_slice(&bytes)?;
        ensure!(
            record.version == event::CALL_LOG_RECORD_VERSION,
            "unsupported diagnostic version"
        );
        Ok(record)
    }

    /// 打开（必要时创建）日志目录，统计现有段并修复最新段的半条尾记录。
    pub(super) async fn open(dir: PathBuf) -> Result<Self> {
        Self::open_with_policy(dir, OpenPolicy::Degraded).await
    }

    pub(super) async fn prepare(dir: PathBuf) -> Result<Self> {
        Self::open_with_policy(dir, OpenPolicy::Strict).await
    }

    async fn open_with_policy(dir: PathBuf, policy: OpenPolicy) -> Result<Self> {
        let setup = async {
            tokio::fs::create_dir_all(&dir).await?;
            list_segments(&dir).await
        }
        .await;
        let mut segments = match setup {
            Ok(segments) => segments,
            Err(error) => {
                if policy == OpenPolicy::Strict {
                    return Err(error);
                }
                tracing::warn!(%error, "调用日志不可用，会话仍可运行");
                return Ok(Self {
                    dir,
                    current: None,
                    total_bytes: 0,
                    next_sequence: 0,
                    degraded: Some(error.to_string()),
                });
            }
        };
        segments.sort_by_key(|segment| (segment.day, segment.seq));
        let total_bytes = segments.iter().map(|segment| segment.len).sum();
        let next_sequence = segments
            .iter()
            .map(|segment| segment.seq + 1)
            .max()
            .unwrap_or(0);
        let mut log = Self {
            dir,
            current: None,
            total_bytes,
            next_sequence,
            degraded: None,
        };
        if let Some(newest) = segments.last() {
            let path = log.dir.join(&newest.name);
            let repaired = match repair_tail(&path).await {
                Ok(length) => length,
                Err(error) => {
                    if policy == OpenPolicy::Strict {
                        return Err(error);
                    }
                    log.degraded = Some(error.to_string());
                    newest.len
                }
            };
            if repaired < newest.len {
                log.total_bytes = log.total_bytes.saturating_sub(newest.len - repaired);
                tracing::warn!(
                    segment = newest.name,
                    discarded_bytes = newest.len - repaired,
                    "调用日志丢弃了不完整的尾部记录"
                );
            }
        }
        Ok(log)
    }

    pub(super) fn close(&mut self) {
        self.current.take();
    }

    /// 日志目录所属的 calls 根目录。
    pub(super) fn root_dir(&self) -> PathBuf {
        self.dir
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_else(|| PathBuf::from("."))
    }

    /// 追加一行记录；必要时轮转与清理。超过总容量且清理无法腾出空间时拒绝本条，保留后续清理责任。
    pub(super) async fn append(&mut self, day: i64, now: i64, line: &str) -> Result<AppendOutcome> {
        let mut bytes = Vec::with_capacity(line.len() + 1);
        bytes.extend_from_slice(line.as_bytes());
        bytes.push(b'\n');
        let len = bytes.len() as u64;
        ensure!(
            len <= SEGMENT_MAX_BYTES,
            "call record exceeds segment capacity"
        );

        let mut removed = Vec::new();
        if self.total_bytes.saturating_add(len) > LOG_MAX_BYTES {
            let outcome = self.cleanup_budget(now, len).await;
            removed.extend(outcome.removed);
            if outcome.over_capacity || self.total_bytes.saturating_add(len) > LOG_MAX_BYTES {
                let error = outcome
                    .failure
                    .unwrap_or_else(|| std::io::Error::other("call log is over capacity").into());
                self.degraded = Some(error.to_string());
                return Err(error).context("call log is over capacity");
            }
        }

        let rotate = match &self.current {
            None => true,
            Some(segment) => {
                segment.day != day || segment.bytes.saturating_add(len) > SEGMENT_MAX_BYTES
            }
        };
        if rotate {
            self.current = None;
            self.open_segment(day).await?;
            let outcome = self.cleanup_budget(now, len).await;
            removed.extend(outcome.removed);
        }

        let segment = self.current.as_mut().expect("segment opened above");
        let offset = segment.bytes;
        if let Err(error) = segment.file.write_all(&bytes).await {
            // 回退到本次写入之前，避免留下半条记录。
            let repair = segment.file.set_len(offset).await;
            self.degraded = Some(error.to_string());
            if let Err(repair) = repair {
                self.current = None;
                return Err(repair)
                    .context(error)
                    .context("call log append and tail repair failed");
            }
            return Err(error.into());
        }
        segment.bytes = offset + len;
        self.total_bytes = self.total_bytes.saturating_add(len);
        segment.file.flush().await?;
        segment.file.sync_data().await?;
        if let Some(previous) = self.degraded.take() {
            tracing::info!(error = previous, "调用日志已从先前的失败中恢复");
        }
        Ok(AppendOutcome {
            record: AppendedRecord {
                segment: segment.name.clone(),
                offset,
                length: line.len() as u64,
            },
            removed,
        })
    }

    /// 清理过期与超量段，返回被删除的段名。永远不删除当前写入段。
    pub(super) async fn cleanup(&mut self, now: i64) -> CleanupOutcome {
        self.cleanup_budget(now, 0).await
    }

    async fn cleanup_budget(&mut self, now: i64, reserve: u64) -> CleanupOutcome {
        if self
            .current
            .as_ref()
            .is_some_and(|segment| is_expired(segment.day, now))
        {
            self.current = None;
        }
        let mut segments = match list_segments(&self.dir).await {
            Ok(segments) => segments,
            Err(error) => {
                let failure = error.to_string();
                self.degraded = Some(failure.clone());
                return CleanupOutcome {
                    failure: Some(error),
                    ..CleanupOutcome::default()
                };
            }
        };
        segments.sort_by_key(|segment| (segment.day, segment.seq));
        self.total_bytes = segments.iter().map(|segment| segment.len).sum();
        let current = self.current.as_ref().map(|segment| segment.name.clone());

        let mut doomed: Vec<(String, u64)> = Vec::new();
        for segment in &segments {
            if Some(&segment.name) == current.as_ref() {
                continue;
            }
            if is_expired(segment.day, now) {
                doomed.push((segment.name.clone(), segment.len));
            }
        }
        let remaining_total: u64 = segments
            .iter()
            .filter(|segment| !doomed.iter().any(|(name, _)| name == &segment.name))
            .map(|segment| segment.len)
            .sum();
        let mut projected = remaining_total;
        for segment in &segments {
            if projected.saturating_add(reserve) <= LOG_MAX_BYTES {
                break;
            }
            if Some(&segment.name) == current.as_ref()
                || doomed.iter().any(|(name, _)| name == &segment.name)
            {
                continue;
            }
            doomed.push((segment.name.clone(), segment.len));
            projected = projected.saturating_sub(segment.len);
        }

        let mut outcome = CleanupOutcome::default();
        for (name, len) in doomed {
            let path = self.dir.join(&name);
            match tokio::fs::remove_file(&path).await {
                Ok(()) => {}
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => {
                    let failure = error.to_string();
                    self.degraded = Some(failure.clone());
                    outcome.failure = Some(error.into());
                    break;
                }
            }
            self.total_bytes = self.total_bytes.saturating_sub(len);
            outcome.removed.push(name);
        }
        let projected = self.total_bytes;
        outcome.over_capacity = projected.saturating_add(reserve) > LOG_MAX_BYTES;
        outcome
    }

    /// Rebuilds the disposable locator index from complete retained records after a crash.
    pub(super) async fn rebuild_index(&self, db: &DatabaseConnection) -> Result<()> {
        let mut segments = list_segments(&self.dir).await?;
        segments.sort_by_key(|segment| (segment.day, segment.seq));
        let tx = db.begin().await?;
        tx.execute_unprepared("DELETE FROM call_log_index").await?;
        let mut recovered_samples = 0usize;
        for segment in segments {
            ensure!(
                segment.len <= SEGMENT_MAX_BYTES,
                "oversized diagnostic segment {}",
                segment.name
            );
            let bytes = tokio::fs::read(self.dir.join(&segment.name)).await?;
            let mut offset = 0usize;
            for line in bytes.split_inclusive(|byte| *byte == b'\n') {
                if line.last() != Some(&b'\n') {
                    break;
                }
                let text = std::str::from_utf8(&line[..line.len() - 1])?;
                let record = event::decode_record(text)?;
                super::index_record(
                    &tx,
                    &record,
                    text,
                    &AppendedRecord {
                        segment: segment.name.clone(),
                        offset: offset as u64,
                        length: text.len() as u64,
                    },
                    record.truncated,
                )
                .await?;
                // The JSONL append can survive a rolled-back SQLite batch. Rebuild
                // both disposable projections, including samples lost by schema 5's
                // legacy foreign key, without reading or adding reliable cost totals.
                if record.status == CallStatus::Committed.as_str()
                    && matches!(
                        record.kind.as_str(),
                        event::CALL_LOG_KIND_BILLING | event::CALL_LOG_KIND_MIGRATED
                    )
                {
                    super::performance::record(
                        &tx,
                        &super::performance::sample_from_record(&record),
                    )
                    .await?;
                    recovered_samples += 1;
                    if recovered_samples.is_multiple_of(256) {
                        super::performance::trim(&tx).await?;
                    }
                }
                offset += line.len();
            }
        }
        super::performance::trim(&tx).await?;
        tx.commit().await?;
        Ok(())
    }

    async fn open_segment(&mut self, day: i64) -> Result<()> {
        tokio::fs::create_dir_all(&self.dir).await?;
        let seq = self.next_sequence;
        self.next_sequence = self.next_sequence.saturating_add(1);
        let name = format!("{SEGMENT_PREFIX}{day:010}-{seq:06}{SEGMENT_SUFFIX}");
        let path = self.dir.join(&name);
        let file = tokio::fs::OpenOptions::new()
            .create_new(true)
            .append(true)
            .open(&path)
            .await?;
        let bytes = file.metadata().await?.len();
        self.total_bytes = self.total_bytes.saturating_add(bytes);
        self.current = Some(Segment {
            day,
            name,
            file,
            bytes,
        });
        Ok(())
    }
}

fn is_expired(day: i64, now: i64) -> bool {
    // The cutoff day can contain records newer than the exact retention deadline.
    day < event::record_day(now.saturating_sub(LOG_RETENTION_SECONDS))
}

async fn list_segments(dir: &Path) -> Result<Vec<SegmentInfo>> {
    let mut segments = Vec::new();
    let mut entries = tokio::fs::read_dir(dir).await?;
    while let Some(entry) = entries.next_entry().await? {
        let name = entry.file_name().to_string_lossy().into_owned();
        let Some((day, seq)) = parse_segment_name(&name) else {
            continue;
        };
        let len = entry.metadata().await?.len();
        segments.push(SegmentInfo {
            day,
            seq,
            name,
            len,
        });
    }
    Ok(segments)
}

fn parse_segment_name(name: &str) -> Option<(i64, u64)> {
    let body = name
        .strip_prefix(SEGMENT_PREFIX)?
        .strip_suffix(SEGMENT_SUFFIX)?;
    let (day, seq) = body.split_once('-')?;
    let day = day.parse::<i64>().ok()?;
    let seq = seq.parse::<u64>().ok()?;
    Some((day, seq))
}

/// 把段文件回退到最后一个换行之后，丢弃不完整的尾部记录；返回修复后的字节数。
async fn repair_tail(path: &Path) -> Result<u64> {
    if tokio::fs::metadata(path).await?.len() > SEGMENT_MAX_BYTES {
        return Err(crate::studio::startup::data_error(anyhow::anyhow!(
            "oversized call log segment"
        )));
    }
    let bytes = tokio::fs::read(path).await?;
    if bytes.is_empty() || bytes.last() == Some(&b'\n') {
        return Ok(bytes.len() as u64);
    }
    let keep = bytes
        .iter()
        .rposition(|byte| *byte == b'\n')
        .map_or(0, |position| position + 1);
    let file = tokio::fs::OpenOptions::new().write(true).open(path).await?;
    file.set_len(keep as u64).await?;
    Ok(keep as u64)
}

#[cfg(test)]
mod storage_fault_tests {
    use super::*;
    use tokio::io::{AsyncSeekExt, AsyncWriteExt};

    #[tokio::test]
    async fn rolling_log_bounds_disk_repairs_tail_and_retries_failed_cleanup() -> Result<()> {
        let temp = tempfile::tempdir()?;
        let now = crate::studio::unix_seconds().div_euclid(SECONDS_PER_DAY) * SECONDS_PER_DAY;
        let day = event::record_day(now);
        let mut log = CallLog::open(temp.path().to_owned()).await?;
        let record = CallLogRecord {
            version: event::CALL_LOG_RECORD_VERSION,
            recorded_at: now,
            kind: "attempt".into(),
            thread_id: "thread".into(),
            call_id: "call".into(),
            status: "committed".into(),
            truncated: true,
            ..Default::default()
        };
        let (line, _) = event::encode_record(&record)?;
        // A retained record can belong to the UTC day containing the seven-day cutoff.
        let retained_at = now - LOG_RETENTION_SECONDS + SECONDS_PER_DAY - 1;
        let retained = CallLogRecord {
            recorded_at: retained_at,
            call_id: "retained-boundary".into(),
            ..record.clone()
        };
        let (retained_line, _) = event::encode_record(&retained)?;
        let boundary_now = now + SECONDS_PER_DAY / 2;
        let boundary = log
            .append(event::record_day(retained_at), boundary_now, &retained_line)
            .await?;
        assert!(log.cleanup(boundary_now).await.removed.is_empty());
        let retained_path = temp.path().join(&boundary.record.segment);
        assert_eq!(
            tokio::fs::read_to_string(&retained_path).await?,
            format!("{retained_line}\n")
        );
        let first = log.append(day, now, &line).await?;
        assert!(
            log.read(
                &first.record,
                &pl_core::context::content_hash(line.as_bytes()),
                now
            )
            .await?
            .truncated
        );
        let path = temp.path().join(&first.record.segment);
        log.current = None;
        let mut file = tokio::fs::OpenOptions::new()
            .append(true)
            .open(&path)
            .await?;
        file.write_all(b"{partial").await?;
        drop(file);
        let mut log = CallLog::open(temp.path().to_owned()).await?;
        assert_eq!(
            tokio::fs::metadata(&path).await?.len(),
            first.record.length + 1
        );
        assert!(tokio::fs::try_exists(&retained_path).await?);
        let second = log.append(day + 1, now + SECONDS_PER_DAY, "{}").await?;
        assert_ne!(first.record.segment, second.record.segment);
        let expired = log.cleanup(now + LOG_RETENTION_SECONDS).await;
        assert!(!expired.removed.contains(&first.record.segment));
        let expired = log
            .cleanup(now + LOG_RETENTION_SECONDS + SECONDS_PER_DAY)
            .await;
        assert!(expired.removed.contains(&first.record.segment));
        let missing = log
            .read(
                &first.record,
                &pl_core::context::content_hash(line.as_bytes()),
                now,
            )
            .await
            .unwrap_err();
        assert!(matches!(
            missing.downcast_ref::<LogReadError>(),
            Some(LogReadError::Missing(_))
        ));
        let expired = log
            .read(&first.record, "", now - LOG_RETENTION_SECONDS)
            .await
            .unwrap_err();
        assert!(matches!(
            expired.downcast_ref::<LogReadError>(),
            Some(LogReadError::Expired(_))
        ));
        assert!(tokio::fs::try_exists(temp.path().join(&second.record.segment)).await?);

        // A filesystem refusal preserves its segment and is retried after repair.
        let blocked = format!("calls-{:010}-999999.jsonl", day - 8);
        tokio::fs::create_dir(temp.path().join(&blocked)).await?;
        assert!(log.cleanup(now).await.failure.is_some());
        tokio::fs::remove_dir(temp.path().join(&blocked)).await?;
        tokio::fs::write(temp.path().join(&blocked), b"{}\n").await?;
        assert!(log.cleanup(now).await.removed.contains(&blocked));

        // Sparse complete segments reach the actual 256 MiB boundary without a large allocation.
        log.current = None;
        for segment in list_segments(temp.path()).await? {
            tokio::fs::remove_file(temp.path().join(segment.name)).await?;
        }
        for seq in 0..16 {
            let path = temp.path().join(format!("calls-{day:010}-{seq:06}.jsonl"));
            let mut file = tokio::fs::File::create(path).await?;
            file.set_len(SEGMENT_MAX_BYTES).await?;
            file.seek(std::io::SeekFrom::End(-1)).await?;
            file.write_all(b"\n").await?;
        }
        let mut log = CallLog::open(temp.path().to_owned()).await?;
        let appended = log.append(day, now, "{}").await?;
        assert_eq!(
            appended.removed.len(),
            1,
            "capacity reserves room before appending"
        );
        assert!(log.total_bytes <= LOG_MAX_BYTES);
        Ok(())
    }
}
