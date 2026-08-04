use anyhow::Result;
use sea_orm::{ActiveModelTrait, ActiveValue::Set, EntityTrait};

use crate::studio::entity as entities;
use crate::studio::ids::unix_seconds;
use crate::studio::store::StudioStore;

impl StudioStore {
    pub async fn save_setting(&self, key: &str, value: &str) -> Result<()> {
        use entities::app_setting;
        let now = unix_seconds();
        if let Some(existing) = app_setting::Entity::find_by_id(key.to_string())
            .one(&self.db)
            .await?
        {
            let mut active: app_setting::ActiveModel = existing.into();
            active.value = Set(value.to_string());
            active.updated_at = Set(now);
            active.update(&self.db).await?;
            return Ok(());
        }

        app_setting::ActiveModel {
            key: Set(key.to_string()),
            value: Set(value.to_string()),
            updated_at: Set(now),
        }
        .insert(&self.db)
        .await?;
        Ok(())
    }

    pub async fn load_setting(&self, key: &str) -> Result<Option<String>> {
        use entities::app_setting;
        Ok(app_setting::Entity::find_by_id(key.to_string())
            .one(&self.db)
            .await?
            .map(|setting| setting.value))
    }
}
