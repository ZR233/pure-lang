//! Project 目录：启动基线装载、目录读取与 Project 事实的内存应用与移除。

use std::collections::BTreeMap;

use anyhow::Result;

use crate::studio::ids::unix_seconds;
use crate::studio::store::directory::ProjectRemoval;
use crate::{
    StudioProductEventEnvelope, StudioProductEventKind, StudioProjectDirectoryData,
    StudioProjectDirectoryState,
};

use super::ProductEventBus;

/// Complete internal Project fact: canonical declaration fields plus full dynamic state.
///
/// It is the runtime source of truth for Project reads and edits; the product database is
/// only the startup baseline and the asynchronous search index.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::studio) struct ProjectFact {
    pub(in crate::studio) id: String,
    pub(in crate::studio) name: String,
    pub(in crate::studio) path: String,
    pub(in crate::studio) ssh_alias: Option<String>,
    pub(in crate::studio) created_at: i64,
    pub(in crate::studio) updated_at: i64,
    pub(in crate::studio) last_opened_at: Option<i64>,
    pub(in crate::studio) closed: bool,
}

impl ProjectFact {
    /// The public (open-only) Project projection of this fact.
    pub(in crate::studio) fn projection(&self) -> crate::ProjectRecord {
        crate::ProjectRecord {
            id: self.id.clone(),
            name: self.name.clone(),
            path: self.path.clone(),
            ssh_alias: self.ssh_alias.clone(),
            updated_at: self.updated_at,
        }
    }
}

impl ProductEventBus {
    /// 启动命令显式建立目录初始 revision 与 Project 小集合；普通 read 不改变 revision。
    ///
    /// Thread 目录不做启动全量装载：活动热集合由钉住集合和运行期目录 delta
    /// 构成，旧数据在分页查询时回源 SQLite。
    pub async fn initialize_directories(&self) -> Result<()> {
        self.initialize_revision(&self.revisions.project);
        self.initialize_revision(&self.revisions.thread);
        self.initialize_revision(&self.revisions.agent);
        // The declaration files are the canonical Project source. The product directory is
        // exported once before the layout is published; afterwards the files are loaded as
        // the published set, never overwritten from the database.
        let directory = self.store.list_project_directory_rows().await?;
        self.store
            .workspaces()
            .initialize(&directory)
            .map_err(crate::studio::workspace_declarations::declared_workspace_dir_error)?;
        self.rebuild_project_directory().await
    }

    /// Rebuilds the canonical memory Project set from published declarations plus dynamic
    /// directory state. It never writes or overwrites a declaration file.
    pub(in crate::studio) async fn rebuild_project_directory(&self) -> Result<()> {
        let declarations = self.store.workspaces().snapshot();
        let dynamic = self.store.list_project_dynamic().await?;
        let now = unix_seconds();
        let mut facts = BTreeMap::new();
        for (id, declaration) in &declarations {
            let dynamic = dynamic.get(id);
            facts.insert(
                id.clone(),
                ProjectFact {
                    id: id.clone(),
                    name: declaration.name.clone(),
                    path: declaration.path.clone(),
                    ssh_alias: declaration.ssh_alias.clone(),
                    created_at: dynamic.map_or(now, |dynamic| dynamic.created_at),
                    updated_at: dynamic.map_or(now, |dynamic| dynamic.updated_at),
                    last_opened_at: dynamic.and_then(|dynamic| dynamic.last_opened_at),
                    closed: dynamic.is_some_and(|dynamic| dynamic.closed != 0),
                },
            );
        }
        *self.project_facts.lock().await = facts;
        self.publish_project_projection().await;
        Ok(())
    }

    /// Publishes the public Project projection from the canonical in-memory facts.
    async fn publish_project_projection(&self) {
        let mut projects: Vec<crate::ProjectRecord> = self
            .project_facts
            .lock()
            .await
            .values()
            .filter(|fact| !fact.closed)
            .map(ProjectFact::projection)
            .collect();
        projects.sort_by(|left, right| {
            right
                .updated_at
                .cmp(&left.updated_at)
                .then_with(|| right.id.cmp(&left.id))
        });
        *self.project_snapshot.lock().await = projects;
    }

    /// Records one canonical Project fact (declaration fields + dynamic state) in memory.
    pub(in crate::studio) async fn record_project_fact(&self, fact: ProjectFact) {
        self.project_facts
            .lock()
            .await
            .insert(fact.id.clone(), fact);
        self.publish_project_projection().await;
    }

    /// Reads one canonical in-memory Project fact; runtime reads never touch the database.
    pub(in crate::studio) async fn project_fact(&self, id: &str) -> Option<ProjectFact> {
        self.project_facts.lock().await.get(id).cloned()
    }

    /// Explicit reload: re-read declarations from disk, then atomically publish the rebuilt
    /// set. A failed reload keeps the previously published snapshot.
    pub(in crate::studio) async fn reload_project_declarations(&self) -> Result<()> {
        // Prepare: read and validate the whole file set without publishing anything.
        let candidate = self
            .store
            .workspaces()
            .load_candidate()
            .map_err(crate::studio::workspace_declarations::declared_workspace_dir_error)?;
        // Keep the full dynamic state from the in-memory owner; a reload never re-reads the
        // database, so it cannot overwrite unsaved facts, revive a closed Project, or diverge
        // from the declaration snapshot.
        let current = self.project_facts.lock().await.clone();
        let now = unix_seconds();
        let mut facts = BTreeMap::new();
        for (id, declaration) in &candidate {
            let mut fact = current.get(id).cloned().unwrap_or(ProjectFact {
                id: id.clone(),
                name: declaration.name.clone(),
                path: declaration.path.clone(),
                ssh_alias: declaration.ssh_alias.clone(),
                created_at: now,
                updated_at: now,
                last_opened_at: None,
                closed: false,
            });
            fact.name = declaration.name.clone();
            fact.path = declaration.path.clone();
            fact.ssh_alias = declaration.ssh_alias.clone();
            facts.insert(id.clone(), fact);
        }
        // Commit: declaration marker (file gate) first, then one atomic publish of the full
        // fact set and its projection, then exactly one event.
        self.store
            .workspaces()
            .commit_snapshot(candidate)
            .map_err(crate::studio::workspace_declarations::declared_workspace_dir_error)?;
        *self.project_facts.lock().await = facts;
        self.publish_project_projection().await;
        self.bump(&self.revisions.project);
        let state = self.read_project_directory().await?;
        self.emit(StudioProductEventKind::ProjectDirectoryChanged(state));
        Ok(())
    }

    pub async fn read_project_directory(&self) -> Result<StudioProjectDirectoryState> {
        Ok(StudioProjectDirectoryState {
            state: self.resource(
                &self.revisions.project,
                StudioProjectDirectoryData {
                    projects: self.project_snapshot.lock().await.clone(),
                },
            ),
        })
    }

    pub(in crate::studio) async fn project_snapshot(&self) -> Vec<crate::ProjectRecord> {
        self.project_snapshot.lock().await.clone()
    }

    pub(super) async fn apply_project_delta(
        &self,
        upserted: &[crate::ProjectRecord],
        removed: &[ProjectRemoval],
    ) -> Result<Option<StudioProductEventEnvelope>> {
        if upserted.is_empty() && removed.is_empty() {
            return Ok(None);
        }
        let mut projects = self.project_snapshot.lock().await;
        for project in upserted {
            if let Some(existing) = projects.iter_mut().find(|entry| entry.id == project.id) {
                *existing = project.clone();
            } else {
                projects.push(project.clone());
            }
        }
        for removal in removed {
            projects.retain(|project| project.id != removal.project_id);
        }
        projects.sort_by(|left, right| {
            right
                .updated_at
                .cmp(&left.updated_at)
                .then_with(|| right.id.cmp(&left.id))
        });
        drop(projects);
        self.bump(&self.revisions.project);
        let state = self.read_project_directory().await?;
        Ok(Some(self.emit(
            StudioProductEventKind::ProjectDirectoryChanged(state),
        )))
    }

    /// 把调用方已经提交的 Project 事实直接应用到内存目录。
    pub async fn apply_project_entry(
        &self,
        project: crate::ProjectRecord,
    ) -> Result<StudioProductEventEnvelope> {
        self.apply_project_delta(std::slice::from_ref(&project), &[])
            .await?
            .ok_or_else(|| anyhow::anyhow!("project upsert did not produce an event"))
    }

    /// 从活动 Project 目录移除一个已归档或隔离的 Project。
    pub async fn remove_project_entry(
        &self,
        project_id: &str,
    ) -> Result<StudioProductEventEnvelope> {
        self.apply_project_delta(
            &[],
            &[ProjectRemoval {
                project_id: project_id.to_string(),
                thread_ids: Vec::new(),
                closed_at: unix_seconds(),
            }],
        )
        .await?
        .ok_or_else(|| anyhow::anyhow!("project removal did not produce an event"))
    }
}

#[cfg(test)]
mod tests {
    use crate::StudioProductEventKind;
    use crate::studio::ids::unix_seconds;
    use crate::studio::store::directory::DirectoryDelta;
    use crate::studio::store::directory::ProjectDirectoryRecord;

    use super::super::tests::{memory_bus, seed_project};
    use super::ProjectFact;

    #[tokio::test]
    async fn project_directory_changes_only_when_the_memory_owner_applies_a_fact() {
        let (store, runtime) = memory_bus().await;
        let project = seed_project(&store).await;
        runtime
            .apply_project_entry(project.clone())
            .await
            .expect("hot project");

        let hot = runtime
            .read_project_directory()
            .await
            .expect("hot directory");
        assert_eq!(hot.state.value().unwrap().projects, vec![project.clone()]);

        runtime
            .remove_project_entry(&project.id)
            .await
            .expect("remove hot project");
        assert!(
            runtime
                .read_project_directory()
                .await
                .unwrap()
                .state
                .value()
                .unwrap()
                .projects
                .is_empty()
        );
    }

    #[tokio::test]
    async fn project_only_commit_does_not_emit_an_empty_thread_directory_change() {
        let (_store, bus) = memory_bus().await;
        let mut events = bus.subscribe();
        let now = unix_seconds();

        bus.commit_directory(DirectoryDelta::upsert_project(ProjectDirectoryRecord {
            id: "project-only".to_string(),
            name: "Project only".to_string(),
            path: "/tmp/project-only".to_string(),
            ssh_alias: None,
            created_at: now,
            updated_at: now,
            last_opened_at: Some(now),
            closed: false,
        }))
        .await
        .expect("project directory commit");

        let mut kinds = Vec::new();
        while let Ok(event) = events.try_recv() {
            kinds.push(event.kind);
        }
        assert_eq!(kinds.len(), 1);
        assert!(matches!(
            kinds[0],
            StudioProductEventKind::ProjectDirectoryChanged(_)
        ));
    }

    #[tokio::test]
    async fn reload_keeps_a_closed_project_closed() {
        let (store, bus) = memory_bus().await;
        let project = seed_project(&store).await;
        store
            .workspaces()
            .declare(&crate::config::WorkspaceDeclaration::new(
                project.id.clone(),
                project.name.clone(),
                project.path.clone(),
                project.ssh_alias.clone(),
            ))
            .unwrap();
        bus.rebuild_project_directory().await.unwrap();
        assert!(
            bus.project_snapshot()
                .await
                .iter()
                .any(|entry| entry.id == project.id)
        );

        let mut fact = bus.project_fact(&project.id).await.unwrap();
        fact.closed = true;
        bus.record_project_fact(fact).await;
        assert!(
            !bus.project_snapshot()
                .await
                .iter()
                .any(|entry| entry.id == project.id)
        );

        bus.reload_project_declarations().await.unwrap();
        assert!(
            !bus.project_snapshot()
                .await
                .iter()
                .any(|entry| entry.id == project.id),
            "a reload must not revive a closed Project"
        );
    }

    #[tokio::test]
    async fn runtime_reads_see_a_declared_project_before_it_reaches_the_database() {
        let (store, bus) = memory_bus().await;
        bus.initialize_directories().await.unwrap();
        bus.record_project_fact(ProjectFact {
            id: "project-hot".into(),
            name: "Hot".into(),
            path: "/tmp/hot".into(),
            ssh_alias: None,
            created_at: 5,
            updated_at: 5,
            last_opened_at: None,
            closed: false,
        })
        .await;

        // The database index has not been written yet, but runtime reads see the fact.
        assert!(
            store
                .list_project_dynamic()
                .await
                .unwrap()
                .get("project-hot")
                .is_none()
        );
        assert_eq!(bus.project_fact("project-hot").await.unwrap().name, "Hot");
        let mut renamed = bus.project_fact("project-hot").await.unwrap();
        renamed.name = "Renamed".into();
        bus.record_project_fact(renamed).await;
        assert_eq!(
            bus.project_fact("project-hot").await.unwrap().name,
            "Renamed"
        );
        assert!(
            bus.project_snapshot()
                .await
                .iter()
                .any(|entry| entry.name == "Renamed")
        );
    }

    #[tokio::test]
    async fn a_failed_reload_leaves_both_snapshots_unchanged() {
        let (store, bus) = memory_bus().await;
        let project = seed_project(&store).await;
        store
            .workspaces()
            .declare(&crate::config::WorkspaceDeclaration::new(
                project.id.clone(),
                project.name.clone(),
                project.path.clone(),
                project.ssh_alias.clone(),
            ))
            .unwrap();
        bus.rebuild_project_directory().await.unwrap();
        let before_fact = bus.project_fact(&project.id).await;
        let before_projection = bus.project_snapshot().await;

        // Corrupt the declaration set so `load_candidate` fails after validating the files.
        let path = store.workspaces().declaration_path(&project.id).unwrap();
        std::fs::remove_file(&path).unwrap();
        assert!(bus.reload_project_declarations().await.is_err());
        assert_eq!(bus.project_fact(&project.id).await, before_fact);
        assert_eq!(bus.project_snapshot().await, before_projection);
    }
}
