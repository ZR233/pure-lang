//! Bounded membership of the performance projection; billing facts remain authoritative.

use super::*;

pub(crate) const PERFORMANCE_SAMPLE_LIMIT: usize = 3_000;

pub(super) async fn migrate(db: &DatabaseConnection) -> Result<()> {
    let tx = db.begin().await?;
    tx.execute_raw(statement(
        "INSERT OR REPLACE INTO performance_samples(thread_id,call_id,completed_at)
         SELECT thread_id,call_id,COALESCE(finished_at,started_at) FROM model_calls
         WHERE terminal=1 AND status='committed'
         ORDER BY COALESCE(finished_at,started_at) DESC,thread_id DESC,call_id DESC LIMIT ?",
        vec![(PERFORMANCE_SAMPLE_LIMIT as i64).into()],
    ))
    .await?;
    trim(&tx).await?;
    tx.execute_raw(statement(
        "UPDATE calls_meta SET schema_version=? WHERE id=1",
        vec![CALLS_SCHEMA_VERSION.into()],
    ))
    .await?;
    tx.commit().await?;
    Ok(())
}

pub(super) async fn record(db: &impl ConnectionTrait, thread: &str, call: &str) -> Result<()> {
    db.execute_raw(statement(
        "DELETE FROM performance_samples WHERE thread_id=? AND call_id=?
         AND NOT EXISTS (SELECT 1 FROM model_calls WHERE thread_id=? AND call_id=?
                         AND terminal=1 AND status='committed')",
        vec![thread.into(), call.into(), thread.into(), call.into()],
    ))
    .await?;
    db.execute_raw(statement(
        "INSERT INTO performance_samples(thread_id,call_id,completed_at)
         SELECT thread_id,call_id,COALESCE(finished_at,started_at) FROM model_calls
         WHERE thread_id=? AND call_id=? AND terminal=1 AND status='committed'
         ON CONFLICT(thread_id,call_id) DO UPDATE SET completed_at=excluded.completed_at",
        vec![thread.into(), call.into()],
    ))
    .await?;
    Ok(())
}

pub(super) async fn trim(db: &impl ConnectionTrait) -> Result<()> {
    db.execute_raw(statement(
        "DELETE FROM performance_samples WHERE (thread_id,call_id) IN (
           SELECT thread_id,call_id FROM performance_samples
           ORDER BY completed_at DESC,thread_id DESC,call_id DESC LIMIT -1 OFFSET ?)",
        vec![(PERFORMANCE_SAMPLE_LIMIT as i64).into()],
    ))
    .await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn migration_bounds_performance_without_losing_billing_or_reintroducing_old_samples()
    -> Result<()> {
        let temp = tempfile::tempdir()?;
        let path = temp.path().join("calls.sqlite");
        let legacy = CallsStore::open(&path).await?;
        legacy
            .writer
            .db
            .execute_unprepared(
                "WITH RECURSIVE n(i) AS (SELECT 1 UNION ALL SELECT i+1 FROM n WHERE i<3005)
             INSERT INTO model_calls(thread_id,call_id,turn_id,attempt_id,revision,admitted_at,
               started_at,finished_at,status,terminal,root_thread_id,provider_instance_id,
               sent_model,output_tokens,decode_millis,cost_currency,cost_amount)
             SELECT 'thread',printf('call-%04d',i),'turn',printf('call-%04d',i),i,i,i,i,
               'committed',1,'root','provider','model',10,100,'USD',1.0 FROM n;
             UPDATE calls_meta SET schema_version=3;",
            )
            .await?;
        let upgraded = CallsStore::open(&path).await?;
        let samples = upgraded.recent_performance_samples(5000).await?;
        assert_eq!(samples.len(), 3000);
        assert_eq!(samples.first().unwrap().completed_at, 3005);
        assert_eq!(samples.last().unwrap().completed_at, 6);
        let summary = upgraded.performance_summary_rows().await?;
        assert_eq!(summary[0].sample_count, 3000);
        let costs = upgraded.session_cost_rollups().await?;
        assert_eq!(costs[0].estimated_costs[0].amount, 3005.0);

        // A delayed observation of an old call cannot push out a genuinely newer sample.
        let tx = upgraded.writer.db.begin().await?;
        record(&tx, "thread", "call-0001").await?;
        trim(&tx).await?;
        tx.commit().await?;
        let reopened = CallsStore::open(&path).await?;
        let samples = reopened.recent_performance_samples(5000).await?;
        assert_eq!(samples.len(), 3000);
        assert_eq!(samples.last().unwrap().completed_at, 6);
        legacy.stop_best_effort();
        upgraded.stop_best_effort();
        reopened.stop_best_effort();
        Ok(())
    }
}
