//! Project 目录命令：打开、列出与归档 Project，并物化项目归档所需的冷目录范围。

use std::path::Path;

use anyhow::{Result, bail};

use crate::resolve_workspace_root;
use crate::studio::records::{ProjectRecord, ThreadRecord, ThreadVisibility};
use crate::studio::store::directory::{DirectoryDelta, ProjectDirectoryRecord, ProjectRemoval};

use super::super::StudioRuntime;
use super::super::thread_title::ThreadTitleCancellationCause;

impl StudioRuntime {
    pub async fn open_project(&self, path: impl AsRef<Path>) -> Result<ProjectRecord> {
        self.ensure_persistence_accepts_new_work()?;
        let path = path.as_ref();
        let _ = resolve_workspace_root(path)?;
        let path_text = path.to_string_lossy().to_string();
        let name = crate::studio::paths::project_name(path);
        let now = crate::studio::unix_seconds();
        // 聚合冷加载：按 path 找到既有行或分配新 id，然后内存先行提交目录 delta。
        let existing = self.store.find_project_by_path(&path_text, None).await?;
        let (record, delta_record) = match existing {
            Some(existing) => {
                let delta_record = ProjectDirectoryRecord {
                    id: existing.id.clone(),
                    name: name.clone(),
                    path: path_text.clone(),
                    ssh_server_id: None,
                    created_at: existing.created_at,
                    updated_at: now,
                    last_opened_at: Some(now),
                    closed: false,
                };
                let public = ProjectRecord {
                    id: existing.id.clone(),
                    name,
                    path: path_text,
                    ssh_server_id: None,
                    updated_at: now,
                };
                (public, delta_record)
            }
            None => {
                let id = crate::studio::ids::new_id("project");
                let delta_record = ProjectDirectoryRecord {
                    id: id.clone(),
                    name: name.clone(),
                    path: path_text.clone(),
                    ssh_server_id: None,
                    created_at: now,
                    updated_at: now,
                    last_opened_at: Some(now),
                    closed: false,
                };
                let public = ProjectRecord {
                    id,
                    name,
                    path: path_text,
                    ssh_server_id: None,
                    updated_at: now,
                };
                (public, delta_record)
            }
        };
        self.agent_facility
            .product_events
            .commit_directory(DirectoryDelta::upsert_project(delta_record))
            .await?;
        Ok(record)
    }

    pub async fn list_projects(&self) -> Result<Vec<ProjectRecord>> {
        Ok(self.agent_facility.product_events.project_snapshot().await)
    }

    pub async fn archive_project(&self, project_id: &str) -> Result<Option<ProjectRecord>> {
        self.ensure_persistence_accepts_new_work()?;
        let Some(project) = self
            .agent_facility
            .product_events
            .project_snapshot()
            .await
            .into_iter()
            .find(|project| project.id == project_id)
        else {
            return Ok(None);
        };
        let threads = self.activate_project_archive_scope(project_id).await?;
        let thread_ids = threads
            .iter()
            .map(|thread| thread.id.clone())
            .collect::<Vec<_>>();
        let active_threads = threads
            .iter()
            .filter(|thread| thread.visibility == ThreadVisibility::Active)
            .collect::<Vec<_>>();
        let _pins = self
            .residency
            .pin_many(active_threads.iter().map(|thread| thread.id.clone()));
        for thread in &active_threads {
            let _ = self.ensure_thread_agent(&thread.id).await?;
        }
        for thread in &active_threads {
            if self.thread_is_busy(&thread.id).await? {
                bail!("project has an active turn");
            }
        }
        for thread in &active_threads {
            let emitter = self.interaction_emitter(thread.id.clone());
            self.agent_facility
                .interactions
                .cancel_thread(
                    self.pending_thread_interactions(&thread.id).await?,
                    "project archived",
                    emitter,
                )
                .await?;
        }
        self.retire_archived_thread_tree(&thread_ids).await;
        self.agent_facility
            .product_events
            .commit_directory(DirectoryDelta {
                project_removals: vec![ProjectRemoval {
                    project_id: project.id.clone(),
                    thread_ids: thread_ids.clone(),
                    closed_at: crate::studio::unix_seconds(),
                }],
                ..Default::default()
            })
            .await?;
        for thread_id in &thread_ids {
            self.title_tasks
                .cancel(thread_id, ThreadTitleCancellationCause::ProjectArchive)
                .await;
            self.model_performance.remove_session(thread_id).await?;
        }
        Ok(Some(project))
    }

    async fn activate_project_archive_scope(&self, project_id: &str) -> Result<Vec<ThreadRecord>> {
        let threads = self.store.list_threads_for_project(project_id).await?;
        let entries = threads
            .iter()
            .filter(|thread| thread.visibility == ThreadVisibility::Active)
            .cloned()
            .map(pl_protocol::Thread::from)
            .collect();
        self.agent_facility
            .product_events
            .warm_thread_index(entries);
        Ok(threads)
    }
}
