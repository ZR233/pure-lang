//! Project 目录：启动基线装载、目录读取与 Project 事实的内存应用与移除。

use anyhow::Result;

use crate::studio::ids::unix_seconds;
use crate::studio::store::directory::ProjectRemoval;
use crate::{
    StudioProductEventEnvelope, StudioProductEventKind, StudioProjectDirectoryData,
    StudioProjectDirectoryState,
};

use super::ProductEventBus;

impl ProductEventBus {
    /// 启动命令显式建立目录初始 revision 与 Project 小集合；普通 read 不改变 revision。
    ///
    /// Thread 目录不做启动全量装载：活动热集合由钉住集合和运行期目录 delta
    /// 构成，旧数据在分页查询时回源 SQLite。
    pub async fn initialize_directories(&self) -> Result<()> {
        self.initialize_revision(&self.revisions.project);
        self.initialize_revision(&self.revisions.thread);
        self.initialize_revision(&self.revisions.agent);
        self.initialize_revision(&self.revisions.recovery);
        let durable_projects = self.store.list_projects().await?;
        let mut projects = self.project_snapshot.lock().await;
        for project in durable_projects {
            if !projects.iter().any(|hot| hot.id == project.id) {
                projects.push(project);
            }
        }
        projects.sort_by(|left, right| {
            right
                .updated_at
                .cmp(&left.updated_at)
                .then_with(|| right.id.cmp(&left.id))
        });
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
            ssh_server_id: None,
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
}
