use crate::api::studio::bridge_runtime::active_bridge;
use crate::api::studio::convert::records::thread_from_record;
use crate::api::studio::convert::thread_stream::{
    bridge_thread, bridge_thread_item, bridge_thread_snapshot, bridge_turn,
};
use crate::api::studio::types::{
    BridgeError, BridgeListThreadsPageRequest, BridgeThread, BridgeThreadContextDisposition,
    BridgeThreadDirectoryPage, BridgeThreadSnapshot, BridgeThreadTurnHistory, BridgeThreadTurnPage,
    ListThreadTurnsRequest,
};

pub async fn list_threads(project_id: String) -> Result<Vec<BridgeThread>, BridgeError> {
    let bridge = active_bridge().await?;
    Ok(bridge
        .studio
        .store()
        .list_threads(&project_id)
        .await?
        .into_iter()
        .map(thread_from_record)
        .map(bridge_thread)
        .collect())
}

/// 从内存目录索引按 `(updatedAt, id)` 倒序 keyset 分页；GUI 触底加载使用。
pub async fn list_threads_page(
    request: BridgeListThreadsPageRequest,
) -> Result<BridgeThreadDirectoryPage, BridgeError> {
    let bridge = active_bridge().await?;
    let page = bridge
        .studio
        .product_events()
        .read_thread_directory_page(
            request.cursor.as_deref(),
            usize::try_from(request.limit).map_err(anyhow::Error::from)?,
        )
        .await?;
    Ok(BridgeThreadDirectoryPage {
        meta: page.meta.into(),
        threads: page.threads.into_iter().map(bridge_thread).collect(),
        next_cursor: page.next_cursor,
    })
}

pub async fn read_thread(thread_id: String) -> Result<BridgeThreadSnapshot, BridgeError> {
    let bridge = active_bridge().await?;
    Ok(bridge_thread_snapshot(
        bridge.studio.thread_snapshot(&thread_id).await?,
    )?)
}

pub async fn list_thread_turns(
    request: ListThreadTurnsRequest,
) -> Result<BridgeThreadTurnPage, BridgeError> {
    let bridge = active_bridge().await?;
    let page = bridge
        .studio
        .list_thread_turns(
            &request.thread_id,
            request.cursor.as_deref(),
            usize::try_from(request.limit).map_err(anyhow::Error::from)?,
        )
        .await?;
    let turns = page
        .turns
        .into_iter()
        .map(|history| {
            let items = history
                .items
                .into_iter()
                .map(bridge_thread_item)
                .collect::<anyhow::Result<Vec<_>>>()?
                .into_iter()
                .flatten()
                .collect();
            Ok(BridgeThreadTurnHistory {
                turn: bridge_turn(history.turn),
                items,
                context_disposition: match history.context_disposition {
                    pl_protocol::ThreadContextDisposition::Active => {
                        BridgeThreadContextDisposition::Active
                    }
                    pl_protocol::ThreadContextDisposition::RolledBack => {
                        BridgeThreadContextDisposition::RolledBack
                    }
                },
            })
        })
        .collect::<anyhow::Result<Vec<_>>>()?;
    Ok(BridgeThreadTurnPage {
        turns,
        next_cursor: page.next_cursor,
    })
}

/// 驱动验收专用：在真实 runtime 中预置 N 条确定性 root Thread。
///
/// 仅当进程以 `PURE_STUDIO_SEED_FIXTURES=1` 启动时可用（隔离验收环境）；
/// 普通运行直接拒绝，不触碰用户的会话数据。
pub async fn seed_driver_thread_fixtures(count: u32) -> Result<Vec<BridgeThread>, BridgeError> {
    if std::env::var("PURE_STUDIO_SEED_FIXTURES").ok().as_deref() != Some("1") {
        return Err(BridgeError::invalid_argument(
            "driver fixtures require PURE_STUDIO_SEED_FIXTURES=1",
        ));
    }
    let bridge = active_bridge().await?;
    let mut projects = bridge.studio.list_projects().await?;
    if projects.is_empty() {
        // 全新验收环境还没有 project：在隔离 home 下建一个 workspace 再打开。
        let workspace = std::env::var("PURE_STUDIO_HOME")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|_| std::env::temp_dir().join("pure-studio-seed"))
            .join("seed-workspace");
        std::fs::create_dir_all(&workspace)
            .map_err(|error| BridgeError::invalid_argument(error.to_string()))?;
        bridge.studio.open_project(&workspace).await?;
        projects = bridge.studio.list_projects().await?;
    }
    let project_id = projects
        .first()
        .map(|project| project.id.clone())
        .ok_or_else(|| BridgeError::invalid_argument("no project available for seeding"))?;
    let mut seeded = Vec::new();
    for index in 0..count {
        let title = format!("Driver fixture session {index}");
        let thread = bridge.studio.create_thread(&project_id, &title).await?;
        seeded.push(thread);
    }
    Ok(seeded
        .into_iter()
        .map(thread_from_record)
        .map(bridge_thread)
        .collect())
}
