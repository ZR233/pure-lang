//! Direct SQL history queries; execution owners are never activated for historical pages.
//!
//! 冷历史读取复用 `history.rs` 的只读句柄：它只读打开一个**已存在**的 history 数据库，
//! 不创建目录/数据库、不执行 schema 变更；缺失的库以空 Turn 页回答，损坏或身份不符的库
//! 直接失败闭锁。只有显式激活/写入才会由历史 writer 建库。

use anyhow::Result;

use crate::studio::StudioRuntime;

impl StudioRuntime {
    pub async fn list_thread_turns(
        &self,
        thread_id: &str,
        cursor: Option<&str>,
        limit: usize,
    ) -> Result<pl_protocol::ThreadTurnPage> {
        self.read_owned_thread(thread_id).await?;
        self.ensure_timeline_history(thread_id)
            .await?
            .turn_page(cursor, limit)
            .await
    }
}
