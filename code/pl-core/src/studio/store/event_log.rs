use anyhow::{Context, Result, bail};
use pl_protocol::{StudioEventEnvelope, StudioEventKind};
use sea_orm::{
    ActiveModelTrait, ActiveValue::Set, ColumnTrait, ConnectionTrait, EntityTrait, QueryFilter,
    QueryOrder, QuerySelect, TransactionTrait,
};

use crate::studio::entities;
use crate::studio::mappers::{
    studio_event_envelope, studio_event_record, studio_message_record, studio_part_record,
};
use crate::studio::records::{StudioMessageRecord, StudioPartRecord};

use super::StudioStore;
use super::projection::{apply_studio_event_projection_with_tx, studio_event_kind_label};
impl StudioStore {
    pub async fn next_studio_event_sequence(&self, session_id: &str) -> Result<i64> {
        use entities::studio_event;
        let max_seq = studio_event::Entity::find()
            .filter(studio_event::Column::SessionId.eq(session_id.to_string()))
            .order_by_desc(studio_event::Column::Sequence)
            .one(&self.db)
            .await?
            .map(|row| row.sequence);
        Ok(max_seq.map(|sequence| sequence + 1).unwrap_or(0))
    }

    pub async fn append_studio_event(
        &self,
        mut envelope: StudioEventEnvelope,
    ) -> Result<StudioEventEnvelope> {
        if matches!(envelope.kind, StudioEventKind::MessagePartDelta { .. }) {
            bail!("messagePartDelta is live-only and must not be persisted");
        }
        if matches!(envelope.kind, StudioEventKind::Stale { .. }) {
            bail!("stale is live-only and must not be persisted");
        }
        let tx = self.db.begin().await?;
        if let Some(session_id) = envelope.session_id.as_deref() {
            let next_sequence = next_studio_event_sequence_with_tx(&tx, session_id).await?;
            envelope.sequence = next_sequence as u64;
        }
        envelope = canonicalize_studio_event_with_connection(&tx, envelope).await?;
        apply_studio_event_projection_with_tx(&tx, &envelope).await?;
        insert_studio_event_with_tx(&tx, &envelope).await?;
        tx.commit().await?;
        Ok(envelope)
    }

    pub async fn load_studio_events(
        &self,
        session_id: &str,
        after_sequence: Option<i64>,
        limit: Option<i64>,
    ) -> Result<Vec<StudioEventEnvelope>> {
        use entities::studio_event;
        let mut query = studio_event::Entity::find()
            .filter(studio_event::Column::SessionId.eq(session_id.to_string()))
            .order_by_asc(studio_event::Column::Sequence)
            .order_by_asc(studio_event::Column::Id);
        if let Some(after_sequence) = after_sequence {
            query = query.filter(studio_event::Column::Sequence.gt(after_sequence));
        }
        if let Some(limit) = limit.and_then(|value| u64::try_from(value).ok()) {
            query = query.limit(limit);
        }
        let mut envelopes = Vec::new();
        for row in query.all(&self.db).await? {
            envelopes.push(studio_event_envelope(studio_event_record(row))?);
        }
        Ok(envelopes)
    }

    pub async fn load_studio_messages(&self, session_id: &str) -> Result<Vec<StudioMessageRecord>> {
        use entities::studio_message;
        let rows = studio_message::Entity::find()
            .filter(studio_message::Column::SessionId.eq(session_id.to_string()))
            .order_by_asc(studio_message::Column::CreatedAt)
            .order_by_asc(studio_message::Column::Sequence)
            .order_by_asc(studio_message::Column::Id)
            .all(&self.db)
            .await?;
        rows.into_iter().map(studio_message_record).collect()
    }

    pub async fn read_studio_message(
        &self,
        message_id: &str,
    ) -> Result<Option<StudioMessageRecord>> {
        use entities::studio_message;
        studio_message::Entity::find_by_id(message_id.to_string())
            .one(&self.db)
            .await?
            .map(studio_message_record)
            .transpose()
    }

    pub async fn load_message_parts(&self, session_id: &str) -> Result<Vec<StudioPartRecord>> {
        use entities::message_part;
        let rows = message_part::Entity::find()
            .filter(message_part::Column::SessionId.eq(session_id.to_string()))
            .order_by_asc(message_part::Column::PartOrder)
            .order_by_asc(message_part::Column::Sequence)
            .order_by_asc(message_part::Column::Id)
            .all(&self.db)
            .await?;
        rows.into_iter().map(studio_part_record).collect()
    }

    pub async fn read_message_part(&self, part_id: &str) -> Result<Option<StudioPartRecord>> {
        use entities::message_part;
        message_part::Entity::find_by_id(part_id.to_string())
            .one(&self.db)
            .await?
            .map(studio_part_record)
            .transpose()
    }

    pub async fn next_message_part_order(&self, message_id: &str) -> Result<u64> {
        use entities::message_part;
        let Some(row) = message_part::Entity::find()
            .filter(message_part::Column::MessageId.eq(message_id.to_string()))
            .order_by_desc(message_part::Column::PartOrder)
            .one(&self.db)
            .await?
        else {
            return Ok(0);
        };
        Ok(u64::try_from(row.part_order).context("message part order must be non-negative")? + 1)
    }
}

async fn next_studio_event_sequence_with_tx(
    tx: &sea_orm::DatabaseTransaction,
    session_id: &str,
) -> Result<i64> {
    use entities::studio_event;
    let max_seq = studio_event::Entity::find()
        .filter(studio_event::Column::SessionId.eq(session_id.to_string()))
        .order_by_desc(studio_event::Column::Sequence)
        .one(tx)
        .await?
        .map(|row| row.sequence);
    Ok(max_seq.map(|sequence| sequence + 1).unwrap_or(0))
}

async fn insert_studio_event_with_tx(
    tx: &sea_orm::DatabaseTransaction,
    envelope: &StudioEventEnvelope,
) -> Result<()> {
    use entities::studio_event;
    let payload_json = serde_json::to_string(envelope)?;
    studio_event::ActiveModel {
        id: Set(envelope.event_id.clone()),
        project_id: Set(envelope.project_id.clone()),
        session_id: Set(envelope.session_id.clone()),
        turn_id: Set(envelope.turn_id.clone()),
        sequence: Set(envelope.sequence as i64),
        created_at: Set(envelope.created_at),
        kind: Set(studio_event_kind_label(&envelope.kind).to_string()),
        payload_json: Set(payload_json),
    }
    .insert(tx)
    .await?;
    Ok(())
}

async fn canonicalize_studio_event_with_connection<C>(
    conn: &C,
    mut envelope: StudioEventEnvelope,
) -> Result<StudioEventEnvelope>
where
    C: ConnectionTrait,
{
    if let StudioEventKind::MessagePartUpdated { part } = &mut envelope.kind {
        if let Some(existing_order) =
            existing_message_part_order_with_connection(conn, &part.part_id).await?
        {
            if part.order != existing_order {
                bail!("part order cannot change");
            }
        } else if message_part_order_exists_with_connection(conn, &part.message_id, part.order)
            .await?
        {
            bail!("part order already exists for message");
        }
    }
    if let StudioEventKind::AgentTimelineChanged { event } = &mut envelope.kind {
        event.event_id = envelope.event_id.clone();
        event.session_id = envelope.session_id.clone().unwrap_or_default();
        event.sequence = envelope.sequence;
        event.created_at = envelope.created_at;
    }
    Ok(envelope)
}

async fn existing_message_part_order_with_connection<C>(
    conn: &C,
    part_id: &str,
) -> Result<Option<u64>>
where
    C: ConnectionTrait,
{
    use entities::message_part;
    Ok(message_part::Entity::find_by_id(part_id.to_string())
        .one(conn)
        .await?
        .map(|row| row.part_order as u64))
}

async fn message_part_order_exists_with_connection<C>(
    conn: &C,
    message_id: &str,
    order: u64,
) -> Result<bool>
where
    C: ConnectionTrait,
{
    use entities::message_part;
    Ok(message_part::Entity::find()
        .filter(message_part::Column::MessageId.eq(message_id.to_string()))
        .filter(message_part::Column::PartOrder.eq(order as i64))
        .one(conn)
        .await?
        .is_some())
}
