//! Project 目录命令：打开、列出与归档 Project，并物化项目归档所需的冷目录范围。

use std::path::Path;

use anyhow::{Result, bail};

use crate::studio::records::{ProjectRecord, ThreadRecord, ThreadVisibility};
use crate::studio::store::directory::{DirectoryDelta, ProjectDirectoryRecord, ProjectRemoval};

use super::super::StudioRuntime;
use super::super::thread_title::ThreadTitleCancellationCause;

impl StudioRuntime {
    pub async fn open_project(&self, path: impl AsRef<Path>) -> Result<ProjectRecord> {
        let canonical = dunce::canonicalize(path.as_ref())?;
        anyhow::ensure!(canonical.is_dir(), "workspace path is not a directory");
        let path = canonical.as_path();
        let _guard = self.lifecycle_lock.lock().await;
        let path_text = path.to_string_lossy().to_string();
        if let Some(project) = self
            .agent_facility
            .product_events
            .project_snapshot()
            .await
            .into_iter()
            .find(|project| project.path == path_text && project.ssh_alias.is_none())
        {
            return Ok(project);
        }
        let now = crate::studio::unix_seconds();
        // The declaration file is the sole persistent source: resolve or create it first,
        // and never publish a directory mutation when the declaration write fails.
        let declaration = self
            .store
            .workspaces()
            .declaration_for_path(&path_text, None);
        // 聚合冷加载：按 path 找到既有行以复用身份，然后内存先行提交目录 delta。
        let existing = self.store.find_project_by_path(&path_text, None).await?;
        let (id, name, created_at) = match (&declaration, &existing) {
            (Some(declaration), existing) => (
                declaration.id.clone(),
                declaration.name.clone(),
                existing.as_ref().map_or(now, |row| row.created_at),
            ),
            (None, Some(row)) => (row.id.clone(), row.name.clone(), row.created_at),
            (None, None) => (
                crate::studio::ids::new_id("project"),
                crate::studio::paths::project_name(path),
                now,
            ),
        };
        self.store
            .workspaces()
            .declare(&crate::config::WorkspaceDeclaration::new(
                id.clone(),
                name.clone(),
                path_text.clone(),
                None,
            ))?;
        self.agent_facility
            .product_events
            .record_project_fact(crate::studio::product_event_bus::ProjectFact {
                id: id.clone(),
                name: name.clone(),
                path: path_text.clone(),
                ssh_alias: None,
                created_at,
                updated_at: now,
                last_opened_at: Some(now),
                closed: false,
            })
            .await;
        self.agent_facility
            .product_events
            .commit_directory(DirectoryDelta::upsert_project(ProjectDirectoryRecord {
                id: id.clone(),
                name: name.clone(),
                path: path_text.clone(),
                ssh_alias: None,
                created_at,
                updated_at: now,
                last_opened_at: Some(now),
                closed: false,
            }))
            .await?;
        Ok(ProjectRecord {
            id,
            name,
            path: path_text,
            ssh_alias: None,
            updated_at: now,
        })
    }

    /// Renames the project label without changing its filesystem identity.
    pub async fn rename_project(&self, id: &str, name: &str) -> Result<ProjectRecord> {
        let _guard = self.lifecycle_lock.lock().await;
        let name = name.trim();
        anyhow::ensure!(
            !name.is_empty() && name.chars().count() <= 80,
            "project name must contain 1 to 80 characters"
        );
        // Runtime reads/edits consume the canonical in-memory fact; the database is only
        // the asynchronous search index.
        let existing = self
            .agent_facility
            .product_events
            .project_fact(id)
            .await
            .ok_or_else(|| anyhow::anyhow!("Project not found"))?;
        let now = crate::studio::unix_seconds();
        let fact = crate::studio::product_event_bus::ProjectFact {
            name: name.to_string(),
            updated_at: now,
            ..existing
        };
        // The canonical declaration is written before the directory mutation is published;
        // a failed declaration write leaves the snapshot and directory untouched.
        self.store
            .workspaces()
            .declare(&crate::config::WorkspaceDeclaration::new(
                fact.id.clone(),
                fact.name.clone(),
                fact.path.clone(),
                fact.ssh_alias.clone(),
            ))?;
        self.agent_facility
            .product_events
            .record_project_fact(fact.clone())
            .await;
        self.agent_facility
            .product_events
            .commit_directory(DirectoryDelta::upsert_project(ProjectDirectoryRecord {
                id: fact.id.clone(),
                name: fact.name.clone(),
                path: fact.path.clone(),
                ssh_alias: fact.ssh_alias.clone(),
                created_at: fact.created_at,
                updated_at: fact.updated_at,
                last_opened_at: fact.last_opened_at,
                closed: fact.closed,
            }))
            .await?;
        Ok(fact.projection())
    }

    pub async fn list_projects(&self) -> Result<Vec<ProjectRecord>> {
        Ok(self.agent_facility.product_events.project_snapshot().await)
    }

    /// Explicitly applies external declaration edits: validate the whole set, then publish
    /// atomically. A failed reload keeps the previously published canonical snapshot.
    pub async fn reload_workspaces(&self) -> Result<Vec<ProjectRecord>> {
        let _guard = self.lifecycle_lock.lock().await;
        self.agent_facility
            .product_events
            .reload_project_declarations()
            .await?;
        self.list_projects().await
    }

    pub async fn archive_project(&self, project_id: &str) -> Result<Option<ProjectRecord>> {
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
            let _ = self.ensure_thread_owner(&thread.id).await?;
        }
        for thread in &active_threads {
            if self.thread_is_busy(&thread.id).await? {
                bail!("project has an active turn");
            }
        }
        self.retire_archived_thread_tree(&thread_ids).await?;
        // Keep the in-memory fact closed so a later reload cannot revive it.
        if let Some(mut fact) = self
            .agent_facility
            .product_events
            .project_fact(project_id)
            .await
        {
            fact.closed = true;
            self.agent_facility
                .product_events
                .record_project_fact(fact)
                .await;
        }
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
