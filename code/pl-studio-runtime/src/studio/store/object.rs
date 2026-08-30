//! Stable versioned object storage for bounded Studio working state.

use anyhow::{Context, Result, bail};
use pl_core::canonical_content_hash;
use pl_protocol::AgentWorkingState;
use sea_orm::{ActiveModelTrait, ActiveValue::Set, ConnectionTrait, EntityTrait, IntoActiveModel};
use serde::Serialize;
use serde::de::DeserializeOwned;

use crate::studio::entity::studio_object;

/// A bounded domain value that can be reconstructed from `studio_objects`.
pub(in crate::studio) trait PersistedStudioObject: Sized {
    type PersistenceDto: Serialize + DeserializeOwned;

    const OWNER_KIND: &'static str;
    const OBJECT_KIND: &'static str;
    const SCHEMA_VERSION: i64;

    fn revision(&self) -> u64;
    fn to_persistence_dto(&self) -> Self::PersistenceDto;
    fn from_persistence_dto(dto: Self::PersistenceDto) -> Result<Self>;
}

#[derive(Serialize, serde::Deserialize)]
#[serde(transparent)]
pub(in crate::studio) struct AgentWorkingStateDto(AgentWorkingState);

impl PersistedStudioObject for AgentWorkingState {
    type PersistenceDto = AgentWorkingStateDto;

    const OWNER_KIND: &'static str = "thread";
    const OBJECT_KIND: &'static str = "agentWorkingState";
    const SCHEMA_VERSION: i64 = 1;

    fn revision(&self) -> u64 {
        self.revision
    }

    fn to_persistence_dto(&self) -> Self::PersistenceDto {
        AgentWorkingStateDto(self.clone())
    }

    fn from_persistence_dto(dto: Self::PersistenceDto) -> Result<Self> {
        Ok(dto.0)
    }
}

/// 最近一次成功应用的 Thread typed commit receipt。
///
/// receipt 使 worker 能区分“事务已提交但确认丢失”的精确重试与同 revision
/// 不同内容的内部冲突，不把编码结果带回热状态。
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub(in crate::studio) struct ThreadCommitReceipt {
    pub revision: u64,
    pub payload_hash: String,
}

#[derive(Serialize, serde::Deserialize)]
#[serde(transparent)]
pub(in crate::studio) struct ThreadCommitReceiptDto(ThreadCommitReceipt);

impl PersistedStudioObject for ThreadCommitReceipt {
    type PersistenceDto = ThreadCommitReceiptDto;

    const OWNER_KIND: &'static str = "thread";
    const OBJECT_KIND: &'static str = "commitReceipt";
    const SCHEMA_VERSION: i64 = 1;

    fn revision(&self) -> u64 {
        self.revision
    }

    fn to_persistence_dto(&self) -> Self::PersistenceDto {
        ThreadCommitReceiptDto(self.clone())
    }

    fn from_persistence_dto(dto: Self::PersistenceDto) -> Result<Self> {
        Ok(dto.0)
    }
}

pub(in crate::studio) async fn load_object<T>(
    db: &impl ConnectionTrait,
    owner_id: &str,
) -> Result<Option<T>>
where
    T: PersistedStudioObject,
{
    let Some(row) = load_object_row::<T>(db, owner_id).await? else {
        return Ok(None);
    };
    decode_object::<T>(row).map(Some)
}

pub(in crate::studio) async fn put_object<T>(
    db: &impl ConnectionTrait,
    owner_id: &str,
    value: &T,
    updated_at: i64,
) -> Result<()>
where
    T: PersistedStudioObject,
{
    let payload_json = serde_json::to_string(&value.to_persistence_dto())?;
    let payload_hash = canonical_content_hash(payload_json.as_bytes());
    let revision = i64::try_from(value.revision()).context("object revision exceeds SQLite")?;
    let existing = load_object_row::<T>(db, owner_id).await?;
    if let Some(existing) = existing {
        if existing.revision > revision {
            bail!("{} object {owner_id} revision regressed", T::OBJECT_KIND);
        }
        if existing.revision == revision {
            if existing.payload_hash == payload_hash && existing.payload_json == payload_json {
                return Ok(());
            }
            bail!("{} object {owner_id} revision conflicts", T::OBJECT_KIND);
        }
        let mut active = existing.into_active_model();
        active.revision = Set(revision);
        active.schema_version = Set(T::SCHEMA_VERSION);
        active.payload_json = Set(payload_json);
        active.payload_hash = Set(payload_hash);
        active.updated_at = Set(updated_at);
        active.update(db).await?;
        return Ok(());
    }
    studio_object::ActiveModel {
        owner_kind: Set(T::OWNER_KIND.to_string()),
        owner_id: Set(owner_id.to_string()),
        object_kind: Set(T::OBJECT_KIND.to_string()),
        revision: Set(revision),
        schema_version: Set(T::SCHEMA_VERSION),
        payload_json: Set(payload_json),
        payload_hash: Set(payload_hash),
        updated_at: Set(updated_at),
    }
    .insert(db)
    .await?;
    Ok(())
}

pub(in crate::studio) fn decode_object<T>(row: studio_object::Model) -> Result<T>
where
    T: PersistedStudioObject,
{
    if row.owner_kind != T::OWNER_KIND || row.object_kind != T::OBJECT_KIND {
        bail!("Studio object kind does not match the requested domain type");
    }
    if row.schema_version != T::SCHEMA_VERSION {
        bail!(
            "{} object {} has unsupported schema version {}",
            T::OBJECT_KIND,
            row.owner_id,
            row.schema_version
        );
    }
    let actual_hash = canonical_content_hash(row.payload_json.as_bytes());
    if actual_hash != row.payload_hash {
        bail!("{} object {} hash mismatch", T::OBJECT_KIND, row.owner_id);
    }
    let dto = serde_json::from_str::<T::PersistenceDto>(&row.payload_json)?;
    let value = T::from_persistence_dto(dto)?;
    let payload_revision =
        i64::try_from(value.revision()).context("object payload revision exceeds SQLite")?;
    if payload_revision != row.revision {
        bail!(
            "{} object {} revision mismatch: row={}, payload={payload_revision}",
            T::OBJECT_KIND,
            row.owner_id,
            row.revision
        );
    }
    Ok(value)
}

pub(in crate::studio) async fn load_object_row<T>(
    db: &impl ConnectionTrait,
    owner_id: &str,
) -> Result<Option<studio_object::Model>>
where
    T: PersistedStudioObject,
{
    Ok(studio_object::Entity::find_by_id((
        T::OWNER_KIND.to_string(),
        owner_id.to_string(),
        T::OBJECT_KIND.to_string(),
    ))
    .one(db)
    .await?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn additive_working_state_fields_use_serde_defaults_without_a_sqlite_migration() {
        let payload_json = r#"{"revision":7}"#.to_string();
        let restored = decode_object::<AgentWorkingState>(studio_object::Model {
            owner_kind: "thread".to_string(),
            owner_id: "thread-additive".to_string(),
            object_kind: "agentWorkingState".to_string(),
            revision: 7,
            schema_version: 1,
            payload_hash: canonical_content_hash(payload_json.as_bytes()),
            payload_json,
            updated_at: 1,
        })
        .expect("older additive object payload must decode");

        assert_eq!(
            restored,
            AgentWorkingState {
                revision: 7,
                ..AgentWorkingState::default()
            }
        );
    }
}
