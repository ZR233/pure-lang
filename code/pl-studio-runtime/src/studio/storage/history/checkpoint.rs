//! The latest recovery state shares the history transaction; context records are incremental.

use super::{HistoryStore, applied_write_seq, begin_write, integer, statement};
use anyhow::{Context, Result, ensure};
use pl_core::{
    context::{ContextRecord, content_hash},
    thread::{ThreadCheckpoint, journal::ContextChange},
};
use sea_orm::{ConnectionTrait, TransactionTrait};

impl HistoryStore {
    /// Reads one consistent checkpoint/context/history generation without creating a database.
    pub(crate) async fn checkpoint(&self) -> Result<Option<ThreadCheckpoint>> {
        let Some(connection) = self.reader().await? else {
            return Ok(None);
        };
        if connection.schema_version < 5 && self.state.writer.get().is_none() {
            return Ok(None);
        }
        let tx = connection.db.begin().await?;
        let result = read(&tx, &self.state.thread_id).await?;
        tx.commit().await?;
        Ok(result)
    }

    /// The only import boundary for a validated, fully materialized v2 TOML checkpoint.
    pub(crate) async fn import_checkpoint(
        &self,
        checkpoint: &ThreadCheckpoint,
        legacy_costs: &[super::super::calls::PurposeUsageProjection],
    ) -> Result<()> {
        ensure!(
            checkpoint.thread_id == self.state.thread_id,
            "checkpoint belongs to another Thread"
        );
        ensure!(
            self.watermark().await? == checkpoint.history_fence,
            "legacy history watermark does not match checkpoint fence"
        );
        let tx = begin_write(&self.writer().await?.db).await?;
        ensure!(
            applied_write_seq(&tx).await? == integer(checkpoint.history_fence)?,
            "history changed while importing checkpoint"
        );
        if read(&tx, &self.state.thread_id).await?.is_none() {
            let mut checkpoint = checkpoint.clone();
            for row in tx
                .query_all_raw(statement(
                    "SELECT payload FROM history_items WHERE kind='inference' ORDER BY ordinal",
                    vec![],
                ))
                .await?
            {
                let item: pl_protocol::ThreadItem =
                    serde_json::from_str(&row.try_get::<String>("", "payload")?)?;
                if let pl_protocol::ThreadItemState::Inference(inference) = item.state() {
                    checkpoint
                        .state
                        .attempt_ids
                        .insert(inference.inference_id().to_owned(), item.turn_id.clone());
                }
            }
            for row in tx
                .query_all_raw(statement(
                    "SELECT task_payload FROM history_tool_tasks",
                    vec![],
                ))
                .await?
            {
                let task: pl_core::thread::task::TaskRecord =
                    serde_json::from_str(&row.try_get::<String>("", "task_payload")?)?;
                checkpoint
                    .state
                    .live_calls
                    .insert(task.call_id, task.turn_id);
            }
            // Older checkpoints excluded title/review generation; other usage already belongs to
            // the checkpoint. Reconcile by purpose/currency, never add two overlapping totals.
            let mut remaining: std::collections::BTreeMap<String, f64> = checkpoint
                .state
                .usage_summary
                .estimated_costs
                .iter()
                .map(|cost| (cost.currency.clone(), cost.amount))
                .collect();
            for purpose in legacy_costs {
                for cost in &purpose.estimated_costs {
                    ensure!(
                        cost.amount.is_finite() && cost.amount >= 0.0,
                        "invalid legacy cost"
                    );
                    if matches!(purpose.purpose.as_deref(), Some("title" | "review")) {
                        if let Some(total) = checkpoint
                            .state
                            .usage_summary
                            .estimated_costs
                            .iter_mut()
                            .find(|total| total.currency == cost.currency)
                        {
                            total.amount += cost.amount;
                        } else {
                            checkpoint.state.usage_summary.estimated_costs.push(
                                pl_core::thread::UsageCost {
                                    currency: cost.currency.clone(),
                                    amount: cost.amount,
                                },
                            );
                        }
                    } else {
                        let total = remaining.entry(cost.currency.clone()).or_default();
                        ensure!(
                            *total + 1e-9 >= cost.amount,
                            "legacy cost exceeds reliable checkpoint total"
                        );
                        *total = (*total - cost.amount).max(0.0);
                    }
                    add_cost(&tx, purpose.purpose.as_deref(), &cost.currency, cost.amount).await?;
                }
            }
            for (currency, amount) in remaining {
                if amount > 0.0 {
                    add_cost(&tx, None, &currency, amount).await?;
                }
            }
            write(&tx, &checkpoint, None).await?;
            tx.execute_unprepared("UPDATE session_checkpoint SET legacy_cleanup=1 WHERE id=1")
                .await?;
        }
        tx.commit().await?;
        let restored = self
            .checkpoint()
            .await?
            .context("imported checkpoint is absent")?;
        ensure!(
            restored.state_revision == checkpoint.state_revision
                && restored.state.context == checkpoint.state.context,
            "imported checkpoint verification failed"
        );
        Ok(())
    }

    pub(crate) async fn legacy_cleanup_pending(&self) -> Result<bool> {
        let Some(connection) = self.reader().await? else {
            return Ok(false);
        };
        if connection.schema_version < 5 && self.state.writer.get().is_none() {
            return Ok(false);
        }
        let row = connection
            .db
            .query_one_raw(statement(
                "SELECT legacy_cleanup FROM session_checkpoint WHERE id=1",
                vec![],
            ))
            .await?;
        Ok(row
            .map(|row| row.try_get::<i64>("", "legacy_cleanup"))
            .transpose()?
            .unwrap_or(0)
            != 0)
    }

    pub(crate) async fn finish_legacy_cleanup(&self) -> Result<()> {
        self.writer()
            .await?
            .db
            .execute_unprepared("UPDATE session_checkpoint SET legacy_cleanup=0 WHERE id=1")
            .await?;
        Ok(())
    }
}

pub(super) async fn verify_committed(
    tx: &impl ConnectionTrait,
    checkpoint: &ThreadCheckpoint,
    watermark: i64,
) -> Result<()> {
    let row = tx
        .query_one_raw(statement(
            "SELECT revision FROM session_checkpoint WHERE id=1",
            vec![],
        ))
        .await?
        .context("durable history has no checkpoint")?;
    ensure!(
        row.try_get::<i64>("", "revision")? == watermark
            && watermark >= integer(checkpoint.state_revision)?,
        "durable checkpoint does not cover retried history"
    );
    Ok(())
}

pub(super) async fn write(
    tx: &impl ConnectionTrait,
    checkpoint: &ThreadCheckpoint,
    change: Option<&ContextChange>,
) -> Result<()> {
    ensure!(
        checkpoint.is_materialized(),
        "checkpoint still references external legacy bodies"
    );
    ensure!(
        checkpoint.state_revision == checkpoint.state.commit_sequence
            && checkpoint.history_fence <= checkpoint.state_revision,
        "checkpoint revision is inconsistent"
    );
    let prior = tx
        .query_one_raw(statement(
            "SELECT revision,context_revision,context_count FROM session_checkpoint WHERE id=1",
            vec![],
        ))
        .await?;
    let records = &checkpoint.state.context.records;
    let previous_count = prior
        .as_ref()
        .map(|row| row.try_get::<i64>("", "context_count"))
        .transpose()?;
    let append = match (prior.as_ref(), change) {
        (
            Some(row),
            Some(ContextChange::Append {
                revision,
                records: added,
            }),
        ) => {
            ensure!(
                *revision == checkpoint.state.context.revision,
                "context append revision mismatch"
            );
            let count = usize::try_from(row.try_get::<i64>("", "context_count")?)?;
            ensure!(
                count.checked_add(added.len()) == Some(records.len())
                    && records[count..] == **added,
                "context append does not match checkpoint"
            );
            Some((count, added.as_ref()))
        }
        (Some(row), None) => {
            ensure!(
                row.try_get::<i64>("", "context_revision")?
                    == integer(checkpoint.state.context.revision)?
                    && previous_count == Some(integer(records.len())?),
                "context changed without a mutation"
            );
            Some((records.len(), &[][..]))
        }
        _ => None,
    };
    let (start, changed) = match append {
        Some(value) => value,
        None => {
            tx.execute_unprepared("DELETE FROM current_context").await?;
            (0, records.as_ref())
        }
    };
    for (offset, record) in changed.iter().enumerate() {
        let payload = serde_json::to_string(record)?;
        let hash = content_hash(payload.as_bytes());
        tx.execute_raw(statement(
            "INSERT INTO current_context(ordinal,record_id,payload,payload_hash) VALUES(?,?,?,?)",
            vec![
                integer(start + offset)?.into(),
                record.id.clone().into(),
                payload.into(),
                hash.into(),
            ],
        ))
        .await?;
    }
    let mut metadata = checkpoint.pruned();
    metadata.state.context.records = Default::default();
    let payload = serde_json::to_string(&metadata)?;
    let hash = content_hash(payload.as_bytes());
    tx.execute_raw(statement(
        "INSERT INTO session_checkpoint(id,revision,context_revision,context_count,payload,payload_hash)
         VALUES(1,?,?,?,?,?) ON CONFLICT(id) DO UPDATE SET revision=excluded.revision,
         context_revision=excluded.context_revision,context_count=excluded.context_count,
         payload=excluded.payload,payload_hash=excluded.payload_hash",
        vec![integer(checkpoint.state_revision)?.into(), integer(checkpoint.state.context.revision)?.into(),
            integer(records.len())?.into(), payload.into(), hash.into()],
    )).await?;
    Ok(())
}

async fn read(tx: &impl ConnectionTrait, thread_id: &str) -> Result<Option<ThreadCheckpoint>> {
    let Some(row) = tx.query_one_raw(statement(
        "SELECT revision,context_revision,context_count,payload,payload_hash FROM session_checkpoint WHERE id=1", vec![],
    )).await? else { return Ok(None) };
    let payload: String = row.try_get("", "payload")?;
    ensure!(
        content_hash(payload.as_bytes()) == row.try_get::<String>("", "payload_hash")?,
        "checkpoint content hash mismatch"
    );
    let mut checkpoint: ThreadCheckpoint = serde_json::from_str(&payload)?;
    ensure!(
        checkpoint.thread_id == thread_id
            && ThreadCheckpoint::supports_schema(checkpoint.schema_version)
            && checkpoint.is_materialized()
            && checkpoint.state.context.records.is_empty()
            && checkpoint.state_revision == checkpoint.state.commit_sequence
            && integer(checkpoint.state_revision)? == row.try_get::<i64>("", "revision")?
            && integer(checkpoint.state.context.revision)?
                == row.try_get::<i64>("", "context_revision")?
            && integer(checkpoint.history_fence)? == applied_write_seq(tx).await?,
        "checkpoint identity or history fence mismatch"
    );
    let count = usize::try_from(row.try_get::<i64>("", "context_count")?)?;
    let rows = tx
        .query_all_raw(statement(
            "SELECT ordinal,record_id,payload,payload_hash FROM current_context ORDER BY ordinal",
            vec![],
        ))
        .await?;
    ensure!(rows.len() == count, "current context record count mismatch");
    let mut records = Vec::with_capacity(count);
    for (ordinal, row) in rows.into_iter().enumerate() {
        let payload: String = row.try_get("", "payload")?;
        ensure!(
            integer(ordinal)? == row.try_get::<i64>("", "ordinal")?
                && content_hash(payload.as_bytes()) == row.try_get::<String>("", "payload_hash")?,
            "current context ordering or hash mismatch"
        );
        let record: ContextRecord = serde_json::from_str(&payload)?;
        ensure!(
            record.id == row.try_get::<String>("", "record_id")?,
            "current context identity mismatch"
        );
        records.push(record);
    }
    checkpoint.state.context.records = records.into();
    Ok(Some(checkpoint))
}

/// Accounting is reliable state, independent from the lossy log writer.
pub(super) async fn fold_costs(
    tx: &impl ConnectionTrait,
    effect: &pl_core::thread::ThreadEffectBatch,
) -> Result<()> {
    for fact in crate::studio::thread_projection::billing_facts(effect)? {
        for cost in fact.record.accounting.estimated_costs() {
            add_cost(
                tx,
                fact.record.purpose.as_deref(),
                &cost.currency,
                cost.amount,
            )
            .await?;
        }
    }
    Ok(())
}

async fn add_cost(
    tx: &impl ConnectionTrait,
    purpose: Option<&str>,
    currency: &str,
    amount: f64,
) -> Result<()> {
    let purpose = serde_json::to_string(&purpose)?;
    let previous = tx
        .query_one_raw(statement(
            "SELECT amount FROM session_costs WHERE purpose=? AND currency=?",
            vec![purpose.clone().into(), currency.into()],
        ))
        .await?
        .map(|row| row.try_get::<f64>("", "amount"))
        .transpose()?
        .unwrap_or(0.0);
    let total = previous + amount;
    ensure!(
        amount.is_finite() && total.is_finite(),
        "session accounting overflow"
    );
    tx.execute_raw(statement(
        "INSERT INTO session_costs(purpose,currency,amount) VALUES(?,?,?)
         ON CONFLICT(purpose,currency) DO UPDATE SET amount=excluded.amount",
        vec![purpose.into(), currency.into(), total.into()],
    ))
    .await?;
    Ok(())
}

/// A compact absolute projection; no context or historical body is read.
pub(crate) struct SessionAccounting {
    pub revision: u64,
    pub has_unpriced_usage: bool,
    pub purpose_costs: Vec<(Option<String>, Vec<pl_protocol::RuntimeCostAmount>)>,
}

impl HistoryStore {
    pub(crate) async fn accounting(&self) -> Result<Option<SessionAccounting>> {
        let Some(connection) = self.reader().await? else {
            return Ok(None);
        };
        if connection.schema_version < 5 && self.state.writer.get().is_none() {
            return Ok(None);
        }
        let tx = connection.db.begin().await?;
        let Some(row) = tx
            .query_one_raw(statement(
                "SELECT revision,payload,payload_hash FROM session_checkpoint WHERE id=1",
                vec![],
            ))
            .await?
        else {
            tx.commit().await?;
            return Ok(None);
        };
        let payload: String = row.try_get("", "payload")?;
        ensure!(
            content_hash(payload.as_bytes()) == row.try_get::<String>("", "payload_hash")?,
            "accounting checkpoint hash mismatch"
        );
        let checkpoint: ThreadCheckpoint = serde_json::from_str(&payload)?;
        let mut purposes =
            std::collections::BTreeMap::<Option<String>, Vec<pl_protocol::RuntimeCostAmount>>::new(
            );
        for row in tx
            .query_all_raw(statement(
                "SELECT purpose,currency,amount FROM session_costs ORDER BY purpose,currency",
                vec![],
            ))
            .await?
        {
            let purpose = serde_json::from_str(&row.try_get::<String>("", "purpose")?)?;
            purposes
                .entry(purpose)
                .or_default()
                .push(pl_protocol::RuntimeCostAmount {
                    currency: row.try_get("", "currency")?,
                    amount: row.try_get("", "amount")?,
                });
        }
        tx.commit().await?;
        Ok(Some(SessionAccounting {
            revision: checkpoint.state_revision,
            has_unpriced_usage: checkpoint.state.usage_summary.has_unpriced_usage,
            purpose_costs: purposes.into_iter().collect(),
        }))
    }
}
