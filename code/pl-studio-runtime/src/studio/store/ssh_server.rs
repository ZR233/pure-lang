//! SSH server persistence adapter. Secrets never enter this table.

use anyhow::{Context, Result, ensure};
use pl_core::remote::{SshAuth, SshServerProfile};
use sea_orm::{ActiveModelTrait, ActiveValue::Set, EntityTrait, QueryOrder};

use crate::studio::entity as entities;
use crate::studio::ids::unix_seconds;
use crate::studio::store::StudioStore;

impl StudioStore {
    pub(crate) async fn list_ssh_servers(&self) -> Result<Vec<SshServerProfile>> {
        let models = entities::ssh_server::Entity::find()
            .order_by_asc(entities::ssh_server::Column::Name)
            .order_by_asc(entities::ssh_server::Column::Id)
            .all(&self.db)
            .await?;
        models.into_iter().map(profile_from_model).collect()
    }

    pub(crate) async fn save_ssh_server(
        &self,
        profile: &SshServerProfile,
    ) -> Result<SshServerProfile> {
        let now = unix_seconds();
        let auth_json = serde_json::to_string(&profile.auth)
            .context("failed to encode SSH authentication settings")?;
        let port = i32::from(profile.port);
        let model = match entities::ssh_server::Entity::find_by_id(profile.id.clone())
            .one(&self.db)
            .await?
        {
            Some(existing) => {
                let mut active: entities::ssh_server::ActiveModel = existing.into();
                active.name = Set(profile.name.clone());
                active.host = Set(profile.host.clone());
                active.port = Set(port);
                active.username = Set(profile.username.clone());
                active.auth_json = Set(auth_json);
                active.updated_at = Set(now);
                active.update(&self.db).await?
            }
            None => {
                entities::ssh_server::ActiveModel {
                    id: Set(profile.id.clone()),
                    name: Set(profile.name.clone()),
                    host: Set(profile.host.clone()),
                    port: Set(port),
                    username: Set(profile.username.clone()),
                    auth_json: Set(auth_json),
                    created_at: Set(now),
                    updated_at: Set(now),
                }
                .insert(&self.db)
                .await?
            }
        };
        profile_from_model(model)
    }

    pub(crate) async fn delete_ssh_server(&self, server_id: &str) -> Result<()> {
        entities::ssh_server::Entity::delete_by_id(server_id.to_string())
            .exec(&self.db)
            .await
            .with_context(|| format!("failed to delete SSH server {server_id}"))?;
        Ok(())
    }
}

fn profile_from_model(model: entities::ssh_server::Model) -> Result<SshServerProfile> {
    ensure!(
        (1..=i32::from(u16::MAX)).contains(&model.port),
        "SSH server {} has an invalid persisted port",
        model.id
    );
    let auth: SshAuth = serde_json::from_str(&model.auth_json)
        .with_context(|| format!("SSH server {} has invalid auth settings", model.id))?;
    Ok(SshServerProfile {
        id: model.id,
        name: model.name,
        host: model.host,
        port: u16::try_from(model.port)?,
        username: model.username,
        auth,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn ssh_server_round_trip_persists_only_non_secret_configuration() {
        let store = StudioStore::open_memory().await.expect("store");
        let profile = SshServerProfile {
            id: "server-1".to_string(),
            name: "Development".to_string(),
            host: "example.test".to_string(),
            port: 2222,
            username: "dev".to_string(),
            auth: SshAuth::AgentOrKey {
                identity_file: Some("/keys/development".to_string()),
            },
        };

        store.save_ssh_server(&profile).await.expect("save");

        assert_eq!(store.list_ssh_servers().await.expect("list"), vec![profile]);
    }
}
