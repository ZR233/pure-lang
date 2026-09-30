//! root/thread/purpose 维度的累计费用摘要。
//!
//! 摘要是 revision 绝对投影：可靠 writer 通过 [`super::CallsStore::replace_session_usage`] 提交某个
//! root/thread 的完整累计值，只有当 revision 更大时才整体替换既有行。摘要与正文/日志解耦，因此
//! 日志过期、截断或清理都不会丢失累计费用；幂等更新也不依赖任何将被删除的正文。

use super::*;

/// 单个 root/thread 的绝对累计投影。
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SessionUsageProjection {
    pub(crate) root_thread_id: String,
    pub(crate) thread_id: String,
    /// 单调投影 revision；更小的投影被忽略，相同的投影幂等替换。
    pub(crate) revision: u64,
    pub(crate) has_unpriced_usage: bool,
    pub(crate) purpose_costs: Vec<PurposeUsageProjection>,
}

/// 单个 purpose 的绝对累计投影。
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PurposeUsageProjection {
    pub(crate) purpose: Option<String>,
    pub(crate) estimated_costs: Vec<RuntimeCostAmount>,
}

/// 无费用仅标记未定价用量的行的占位币种；真实币种不会为空。
const UNPRICED_CURRENCY: &str = "";

/// 用 revision 绝对投影替换某个 root/thread 的累计摘要。
pub(super) async fn replace(
    db: &DatabaseTransaction,
    projection: &SessionUsageProjection,
) -> Result<()> {
    let revision = integer(projection.revision)?;
    let tx = db.begin().await?;
    let current = tx
        .query_one_raw(statement(
            "SELECT MAX(revision) AS revision FROM call_usage_summary
             WHERE root_thread_id=? AND thread_id=?",
            vec![
                projection.root_thread_id.clone().into(),
                projection.thread_id.clone().into(),
            ],
        ))
        .await?
        .map(|row| row.try_get::<Option<i64>>("", "revision"))
        .transpose()?
        .flatten()
        .unwrap_or(0);
    if revision <= current && current != 0 {
        tx.rollback().await?;
        return Ok(());
    }
    tx.execute_raw(statement(
        "DELETE FROM call_usage_summary WHERE root_thread_id=? AND thread_id=?",
        vec![
            projection.root_thread_id.clone().into(),
            projection.thread_id.clone().into(),
        ],
    ))
    .await?;
    if projection.purpose_costs.is_empty() {
        insert_row(&tx, projection, "", UNPRICED_CURRENCY, 0.0, revision).await?;
    }
    for purpose in &projection.purpose_costs {
        let purpose_key = purpose.purpose.clone().unwrap_or_default();
        if purpose.estimated_costs.is_empty() {
            insert_row(
                &tx,
                projection,
                &purpose_key,
                UNPRICED_CURRENCY,
                0.0,
                revision,
            )
            .await?;
            continue;
        }
        for cost in &purpose.estimated_costs {
            insert_row(
                &tx,
                projection,
                &purpose_key,
                &cost.currency,
                cost.amount,
                revision,
            )
            .await?;
        }
    }
    tx.commit().await?;
    Ok(())
}

async fn insert_row(
    db: &impl ConnectionTrait,
    projection: &SessionUsageProjection,
    purpose: &str,
    currency: &str,
    amount: f64,
    revision: i64,
) -> Result<()> {
    db.execute_raw(statement(
        "INSERT INTO call_usage_summary(
            root_thread_id,thread_id,purpose,cost_currency,amount,has_unpriced_usage,revision)
         VALUES(?,?,?,?,?,?,?)
         ON CONFLICT(root_thread_id,thread_id,purpose,cost_currency) DO UPDATE SET
            amount=excluded.amount,
            has_unpriced_usage=excluded.has_unpriced_usage,
            revision=excluded.revision",
        vec![
            projection.root_thread_id.clone().into(),
            projection.thread_id.clone().into(),
            purpose.to_owned().into(),
            currency.to_owned().into(),
            amount.into(),
            (projection.has_unpriced_usage as i32).into(),
            revision.into(),
        ],
    ))
    .await?;
    Ok(())
}

/// 迁移时先把旧 `model_calls` 的累计费用保全为摘要（revision 0，供可靠 writer 后续覆盖）。
pub(super) async fn migrate_from_legacy(db: &DatabaseConnection) -> Result<()> {
    db.execute_raw(statement(
        "INSERT INTO call_usage_summary(
            root_thread_id,thread_id,purpose,cost_currency,amount,has_unpriced_usage,revision)
         SELECT root_thread_id,thread_id,COALESCE(purpose,''),cost_currency,
                SUM(cost_amount),MAX(has_unpriced_usage),0
         FROM model_calls
         WHERE terminal=1 AND root_thread_id IS NOT NULL
           AND cost_currency IS NOT NULL AND cost_amount IS NOT NULL
         GROUP BY root_thread_id,thread_id,COALESCE(purpose,''),cost_currency
         ON CONFLICT(root_thread_id,thread_id,purpose,cost_currency) DO UPDATE SET
            amount=excluded.amount,
            has_unpriced_usage=MAX(call_usage_summary.has_unpriced_usage,excluded.has_unpriced_usage)",
        vec![],
    ))
    .await?;
    db.execute_raw(statement(
        "INSERT INTO call_usage_summary(
            root_thread_id,thread_id,purpose,cost_currency,amount,has_unpriced_usage,revision)
         SELECT root_thread_id,thread_id,COALESCE(purpose,''),?,0.0,1,0
         FROM model_calls
         WHERE terminal=1 AND root_thread_id IS NOT NULL AND has_unpriced_usage=1
           AND (cost_currency IS NULL OR cost_amount IS NULL)
         GROUP BY root_thread_id,thread_id,COALESCE(purpose,'')
         ON CONFLICT(root_thread_id,thread_id,purpose,cost_currency) DO UPDATE SET
            has_unpriced_usage=1",
        vec![UNPRICED_CURRENCY.into()],
    ))
    .await?;
    Ok(())
}

/// 按 root/purpose 聚合调用库的累计摘要。
pub(super) async fn read_rollups(db: &DatabaseConnection) -> Result<Vec<SessionCostRollup>> {
    let mut rollups: Vec<SessionCostRollup> = Vec::new();
    let roots = db
        .query_all_raw(statement(
            "SELECT root_thread_id, MAX(has_unpriced_usage) AS unpriced
             FROM call_usage_summary
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
    let costs = db
        .query_all_raw(statement(
            "SELECT root_thread_id, purpose, cost_currency, SUM(amount) AS amount
             FROM call_usage_summary
             WHERE cost_currency <> ?
             GROUP BY root_thread_id, purpose, cost_currency
             ORDER BY root_thread_id, purpose, cost_currency",
            vec![UNPRICED_CURRENCY.into()],
        ))
        .await?;
    for row in &costs {
        let root_thread_id: String = row.try_get("", "root_thread_id")?;
        let purpose_raw: String = row.try_get("", "purpose")?;
        let purpose = (!purpose_raw.is_empty()).then_some(purpose_raw);
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
        match rollup
            .purpose_costs
            .iter()
            .position(|entry| entry.purpose == purpose)
        {
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
