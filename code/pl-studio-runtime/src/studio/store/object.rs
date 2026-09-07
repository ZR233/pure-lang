//! Stable versioned object storage for bounded Studio working state.

use anyhow::{Context, Result, bail};
use pl_core::canonical_content_hash;
use sea_orm::{
    ActiveModelTrait, ActiveValue::Set, ColumnTrait, ConnectionTrait, EntityTrait, IntoActiveModel,
    QueryFilter,
};
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

pub(in crate::studio) async fn load_objects<T>(db: &impl ConnectionTrait) -> Result<Vec<T>>
where
    T: PersistedStudioObject,
{
    studio_object::Entity::find()
        .filter(studio_object::Column::OwnerKind.eq(T::OWNER_KIND))
        .filter(studio_object::Column::ObjectKind.eq(T::OBJECT_KIND))
        .all(db)
        .await?
        .into_iter()
        .map(decode_object::<T>)
        .collect()
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
