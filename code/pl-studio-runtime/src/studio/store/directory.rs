//! Thread/Project 目录事实：内存目录 owner 提交的 delta、write-behind 落库
//! applier 与 `catalog.toml` 冷分页查询。
//!
//! 目录 mutation 一律"内存先行 + delta 异步落库"（见 design/17 §17.2）；本模块
//! 只承载已经由 owner 决定的事实，不做业务校验或状态转换。SQLite `thread`/`project`
//! 表是 write-behind 镜像，冷分页/搜索/归档范围只从 `catalog.toml` 读取。

use anyhow::{Result, bail};
use pl_protocol::{Thread, ThreadModeId, ThreadWorkspaceMode};
use sea_orm::{ActiveModelTrait, ActiveValue::Set, EntityTrait};

use crate::studio::entity as entities;
use crate::studio::ids::unix_seconds;
use crate::studio::store::StudioStore;
use crate::studio::store_support::non_empty_title;

/// 一次目录事实提交。
///
/// 由内存目录 owner（ProductEventBus 侧命令）在更新热集合同一临界区构建；
/// FIFO 队列保证 `thread_upserts` 的注册先于该 Thread 的首个 state commit 落库。
#[derive(Debug, Clone, Default)]
pub(in crate::studio) struct DirectoryDelta {
    pub(in crate::studio) session_activity: Vec<(String, i64)>,
    pub(in crate::studio) session_registrations: Vec<String>,
    pub(in crate::studio) thread_upserts: Vec<Thread>,
    pub(in crate::studio) unregistered_faults: Vec<UnregisteredChildFault>,
    pub(in crate::studio) thread_removals: Vec<ThreadRemoval>,
    pub(in crate::studio) project_upserts: Vec<ProjectDirectoryRecord>,
    pub(in crate::studio) project_removals: Vec<ProjectRemoval>,
}

#[derive(Debug, Clone)]
pub(in crate::studio) struct UnregisteredChildFault {
    pub thread_id: String,
    pub state: crate::studio::records::DirectoryState,
}

impl DirectoryDelta {
    pub(in crate::studio) fn is_empty(&self) -> bool {
        self.unregistered_faults.is_empty()
            && self.session_registrations.is_empty()
            && self.session_activity.is_empty()
            && self.thread_upserts.is_empty()
            && self.thread_removals.is_empty()
            && self.project_upserts.is_empty()
            && self.project_removals.is_empty()
    }

    pub(in crate::studio) fn register_root_thread(
        id: String,
        project_id: &str,
        title: &str,
        mode: ThreadModeId,
        workspace_mode: ThreadWorkspaceMode,
        workspace_path: String,
    ) -> (Self, Thread) {
        let now = unix_seconds();
        let thread = Thread {
            root_thread_id: id.clone(),
            agent_path: id.clone(),
            id,
            project_id: project_id.to_string(),
            title: non_empty_title(title),
            mode: mode.clone(),
            workspace_mode,
            workspace_path,
            parent_thread_id: None,
            role: crate::config::StudioRole::Planner.key().to_string(),
            status: pl_protocol::ThreadStatus::Idle,
            created_at: now,
            updated_at: now,
            archived: false,
        };
        (
            Self {
                thread_upserts: vec![thread.clone()],
                ..Default::default()
            },
            thread,
        )
    }

    /// 注册一个 durable child Thread（executor/reviewer/explorer spawn）。
    pub(in crate::studio) fn register_child_thread(spec: RegisteredChildThread) -> Self {
        let now = unix_seconds();
        Self {
            thread_upserts: vec![Thread {
                agent_path: spec.agent_path,
                id: spec.id,
                project_id: spec.project_id,
                title: non_empty_title(&spec.title),
                mode: spec.mode,
                workspace_mode: spec.workspace_mode,
                workspace_path: spec.workspace_path,
                root_thread_id: spec.root_thread_id,
                parent_thread_id: Some(spec.parent_thread_id),
                role: spec.role,
                status: pl_protocol::ThreadStatus::Idle,
                created_at: now,
                updated_at: now,
                archived: false,
            }],
            ..Default::default()
        }
    }

    /// 归档一棵 Thread 树（调用方已确定全部成员 id）。
    pub(in crate::studio) fn archive_threads(thread_ids: Vec<String>) -> Self {
        Self {
            thread_removals: vec![ThreadRemoval {
                thread_ids,
                archived_at: unix_seconds(),
            }],
            ..Default::default()
        }
    }

    pub(in crate::studio) fn upsert_project(record: ProjectDirectoryRecord) -> Self {
        Self {
            project_upserts: vec![record],
            ..Default::default()
        }
    }
}

/// child Thread 注册事实；父 Thread 的目录事实由调用方（agent host）提供。
#[derive(Debug, Clone)]
pub(in crate::studio) struct RegisteredChildThread {
    pub(in crate::studio) id: String,
    pub(in crate::studio) parent_thread_id: String,
    pub(in crate::studio) agent_path: String,
    pub(in crate::studio) project_id: String,
    pub(in crate::studio) root_thread_id: String,
    pub(in crate::studio) mode: ThreadModeId,
    pub(in crate::studio) workspace_mode: ThreadWorkspaceMode,
    pub(in crate::studio) workspace_path: String,
    pub(in crate::studio) role: String,
    pub(in crate::studio) title: String,
}

#[derive(Debug, Clone)]
pub(in crate::studio) struct ThreadRemoval {
    pub(in crate::studio) thread_ids: Vec<String>,
    pub(in crate::studio) archived_at: i64,
}

#[derive(Debug, Clone)]
pub(in crate::studio) struct ProjectRemoval {
    pub(in crate::studio) project_id: String,
    pub(in crate::studio) thread_ids: Vec<String>,
    pub(in crate::studio) closed_at: i64,
}

/// Project 目录行事实；插入时需要完整列，更新时只改目录列。
#[derive(Debug, Clone)]
pub(in crate::studio) struct ProjectDirectoryRecord {
    pub(in crate::studio) id: String,
    pub(in crate::studio) name: String,
    pub(in crate::studio) path: String,
    pub(in crate::studio) ssh_alias: Option<String>,
    pub(in crate::studio) created_at: i64,
    pub(in crate::studio) updated_at: i64,
    pub(in crate::studio) last_opened_at: Option<i64>,
    pub(in crate::studio) closed: bool,
}

/// 在 writer 已开启的事务中幂等应用一次目录 delta。
///
/// 除 SQLite 目录镜像外，同一 delta 也经 [`StudioStore::persist_directory_summaries`] 幂等写入
/// canonical TOML（`catalog.toml` + `workspaces.toml`）。write-behind 批次因此成为目录摘要的
/// 异步持久化点：观察热路径只把状态摘要并入队列，文件 I/O 由 writer 完成；命令通道已同步写入的
/// delta 在此重放为 no-op，不推进 revision。
pub(in crate::studio) async fn apply_directory_delta(
    store: &StudioStore,
    tx: &sea_orm::DatabaseTransaction,
    delta: &DirectoryDelta,
) -> Result<()> {
    // Parent rows first: `threads.project_id` is a real foreign key to `projects(id)` (ON DELETE
    // CASCADE), and a coalesced write-behind batch can carry a brand-new Project together with a
    // Thread (or child Thread) that references it in the same transaction. Upserting the Project rows
    // before any Thread row keeps the association atomic instead of failing with SQLite extended code
    // 787 `FOREIGN KEY constraint failed`.
    for project in &delta.project_upserts {
        upsert_project_directory_row(tx, project).await?;
    }
    for thread in &delta.thread_upserts {
        upsert_thread_directory_row(tx, thread).await?;
    }
    for id in &delta.session_registrations {
        let row = entities::thread::Entity::find_by_id(id)
            .one(tx)
            .await?
            .ok_or_else(|| anyhow::anyhow!("registered session {id} has no product association"))?;
        let mut row: entities::thread::ActiveModel = row.into();
        row.runtime_revision = Set(Some(1));
        row.update(tx).await?;
    }
    for (id, updated_at) in &delta.session_activity {
        if let Some(row) = entities::thread::Entity::find_by_id(id).one(tx).await? {
            let updated_at = row.updated_at.max(*updated_at);
            let mut row: entities::thread::ActiveModel = row.into();
            row.updated_at = Set(updated_at);
            row.update(tx).await?;
        }
    }
    for fault in &delta.unregistered_faults {
        super::agent_framework::apply_unregistered_child_fault(tx, fault).await?;
    }
    for removal in &delta.thread_removals {
        archive_thread_rows(tx, &removal.thread_ids, removal.archived_at).await?;
    }
    for removal in &delta.project_removals {
        archive_thread_rows(tx, &removal.thread_ids, removal.closed_at).await?;
        close_project_row(tx, &removal.project_id, removal.closed_at).await?;
    }
    store.persist_directory_summaries(delta).await
}

async fn upsert_thread_directory_row(
    tx: &sea_orm::DatabaseTransaction,
    thread: &Thread,
) -> Result<()> {
    use entities::thread;
    anyhow::ensure!(
        thread.agent_path == thread.id,
        "Thread {} directory identity must match its runtime identity",
        thread.id
    );
    let existing = thread::Entity::find_by_id(thread.id.clone())
        .one(tx)
        .await?;
    let Some(existing) = existing else {
        let model = thread::ActiveModel {
            id: Set(thread.id.clone()),
            project_id: Set(thread.project_id.clone()),
            title: Set(thread.title.clone()),
            mode: Set(thread.mode.label().to_string()),
            workspace_mode: Set(thread.workspace_mode.label().to_string()),
            workspace_path: Set(thread.workspace_path.clone()),
            root_thread_id: Set(thread.root_thread_id.clone()),
            parent_thread_id: Set(thread.parent_thread_id.clone()),
            role: Set(thread.role.clone()),
            agent_path: Set(thread.agent_path.clone()),
            state_json: Set(serde_json::to_string(
                &crate::studio::records::DirectoryState::idle(),
            )?),
            revision: Set(0),
            runtime_revision: Set(None),
            event_sequence: Set(0),
            metadata_json: Set("{}".to_string()),
            usage_json: Set(serde_json::to_string(
                &pl_protocol::InferenceTokenUsage::default(),
            )?),
            last_context_tokens: Set(None),
            trace_sequence: Set(0),
            created_at: Set(thread.created_at),
            updated_at: Set(thread.updated_at),
            archived: Set(i32::from(thread.archived)),
            ..Default::default()
        };
        model.insert(tx).await?;
        return Ok(());
    };
    // 身份列不可变；目录更新只触碰标题/模式/角色/归档与时间戳，
    // runtime 列由 state commit 拥有。
    if existing.project_id != thread.project_id
        || existing.root_thread_id != thread.root_thread_id
        || existing.parent_thread_id != thread.parent_thread_id
        || existing.workspace_mode != thread.workspace_mode.label()
        || existing.workspace_path != thread.workspace_path
    {
        bail!(
            "Thread {} directory identity changed: persisted {:?} vs delta {:?}",
            thread.id,
            (
                existing.project_id.as_str(),
                existing.root_thread_id.as_str(),
                existing.parent_thread_id.as_deref(),
                existing.workspace_mode.as_str(),
                existing.workspace_path.as_str(),
            ),
            (
                thread.project_id.as_str(),
                thread.root_thread_id.as_str(),
                thread.parent_thread_id.as_deref(),
                thread.workspace_mode.label(),
                thread.workspace_path.as_str(),
            )
        );
    }
    let mut active: thread::ActiveModel = existing.into();
    active.title = Set(thread.title.clone());
    active.mode = Set(thread.mode.label().to_string());
    active.role = Set(thread.role.clone());
    active.updated_at = Set(thread.updated_at);
    active.archived = Set(i32::from(thread.archived));
    active.update(tx).await?;
    Ok(())
}

async fn archive_thread_rows(
    tx: &sea_orm::DatabaseTransaction,
    thread_ids: &[String],
    archived_at: i64,
) -> Result<()> {
    use entities::thread;
    for thread_id in thread_ids {
        if let Some(existing) = thread::Entity::find_by_id(thread_id.clone())
            .one(tx)
            .await?
        {
            let mut active: thread::ActiveModel = existing.into();
            active.archived = Set(1);
            active.updated_at = Set(archived_at);
            active.update(tx).await?;
        }
    }
    Ok(())
}

async fn upsert_project_directory_row(
    tx: &sea_orm::DatabaseTransaction,
    record: &ProjectDirectoryRecord,
) -> Result<()> {
    use entities::project;
    let existing = project::Entity::find_by_id(record.id.clone())
        .one(tx)
        .await?;
    let Some(existing) = existing else {
        project::ActiveModel {
            id: Set(record.id.clone()),
            name: Set(record.name.clone()),
            path: Set(record.path.clone()),
            ssh_alias: Set(record.ssh_alias.clone()),
            created_at: Set(record.created_at),
            updated_at: Set(record.updated_at),
            last_opened_at: Set(record.last_opened_at),
            closed: Set(i32::from(record.closed)),
        }
        .insert(tx)
        .await?;
        return Ok(());
    };
    if existing.path != record.path || existing.ssh_alias != record.ssh_alias {
        bail!(
            "Project {} directory identity changed: persisted path {}, delta path {}",
            record.id,
            existing.path,
            record.path
        );
    }
    let mut active: project::ActiveModel = existing.into();
    active.name = Set(record.name.clone());
    active.updated_at = Set(record.updated_at);
    active.last_opened_at = Set(record.last_opened_at);
    active.closed = Set(i32::from(record.closed));
    active.update(tx).await?;
    Ok(())
}

async fn close_project_row(
    tx: &sea_orm::DatabaseTransaction,
    project_id: &str,
    closed_at: i64,
) -> Result<()> {
    use entities::project;
    if let Some(existing) = project::Entity::find_by_id(project_id.to_string())
        .one(tx)
        .await?
    {
        let mut active: project::ActiveModel = existing.into();
        active.closed = Set(1);
        active.updated_at = Set(closed_at);
        active.update(tx).await?;
    }
    Ok(())
}

/// Thread 目录 keyset 分页游标：`v1:{updated_at}:{id}`。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::studio) struct ThreadDirectoryCursor {
    pub(in crate::studio) updated_at: i64,
    pub(in crate::studio) id: String,
}

impl ThreadDirectoryCursor {
    pub(in crate::studio) fn encode(&self) -> String {
        format!("v1:{}:{}", self.updated_at, self.id)
    }

    pub(in crate::studio) fn decode(raw: &str) -> Option<Self> {
        let rest = raw.strip_prefix("v1:")?;
        let (updated_at, id) = rest.split_once(':')?;
        Some(Self {
            updated_at: updated_at.parse().ok()?,
            id: id.to_string(),
        })
    }
}

impl StudioStore {
    /// 未归档 Thread 的冷分页：按 `(updated_at, id)` 倒序 keyset，
    /// cursor 为闭区间锚点（下一页取严格小于该键的条目）。
    pub(in crate::studio) async fn list_thread_directory_page(
        &self,
        cursor: Option<&ThreadDirectoryCursor>,
        limit: usize,
    ) -> Result<Vec<Thread>> {
        // `catalog.toml` is the only cold source for the first screen; no session state is read.
        self.catalog_page_active(cursor, limit).await
    }
}

#[cfg(test)]
mod tests {
    use pl_protocol::{Thread, ThreadModeId, ThreadWorkspaceMode};
    use sea_orm::{EntityTrait, TransactionTrait};

    use super::{DirectoryDelta, ProjectDirectoryRecord, apply_directory_delta};
    use crate::studio::entity as entities;
    use crate::studio::store::StudioStore;

    fn root_thread(project_id: &str, thread_id: &str, now: i64) -> Thread {
        Thread {
            id: thread_id.to_string(),
            project_id: project_id.to_string(),
            title: "Session".to_string(),
            mode: ThreadModeId::simple(),
            workspace_mode: ThreadWorkspaceMode::Local,
            workspace_path: "/repo".to_string(),
            root_thread_id: thread_id.to_string(),
            parent_thread_id: None,
            role: crate::config::StudioRole::Planner.key().to_string(),
            agent_path: thread_id.to_string(),
            status: pl_protocol::ThreadStatus::Idle,
            created_at: now,
            updated_at: now,
            archived: false,
        }
    }

    /// 回归：同一条（可能被 write-behind 合并的）delta 同时携带新 Project 与引用它的 Thread 时，
    /// `apply_directory_delta` 必须先写父行，否则 `threads.project_id` 外键会以 SQLite 扩展码 787
    /// `FOREIGN KEY constraint failed` 拒绝整批。
    #[tokio::test]
    async fn project_upsert_precedes_thread_upsert_within_one_delta() {
        let store = StudioStore::open_memory().await.unwrap();
        let now = 1_700_000_000;
        let project_id = "project-1";
        let delta = DirectoryDelta {
            project_upserts: vec![ProjectDirectoryRecord {
                id: project_id.to_string(),
                name: "workspace".to_string(),
                path: "/repo".to_string(),
                ssh_alias: None,
                created_at: now,
                updated_at: now,
                last_opened_at: Some(now),
                closed: false,
            }],
            thread_upserts: vec![root_thread(project_id, "thread-1", now)],
            ..Default::default()
        };

        let tx = store.database().begin().await.unwrap();
        let applied = apply_directory_delta(&store, &tx, &delta).await;
        applied.expect("project and thread rows must apply atomically in one transaction");
        tx.commit().await.unwrap();

        let row = entities::thread::Entity::find_by_id("thread-1".to_string())
            .one(store.database())
            .await
            .unwrap()
            .expect("thread directory row");
        assert_eq!(row.project_id, project_id);
        // The directory summary is still published from the same delta.
        assert!(store.read_thread("thread-1").await.unwrap().is_some());
    }
}
