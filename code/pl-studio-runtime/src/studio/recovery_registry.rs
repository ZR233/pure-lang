//! 恢复问题的独立注册表。
//!
//! 此前 [`StudioRuntimeStateInner`] 同时持有服务生命周期状态（status/error）和
//! 恢复问题列表（recovery_issues），两者变化原因完全不同：status 在每次生命周期
//! 转换时变，recovery_issues 只在启动恢复或用户清理后变。混在同一把锁里会让
//! recovery 的读取/清理阻塞 status 的快速转换，反之亦然。
//!
//! 拆出独立 registry 后，恢复问题拥有自己的锁和快照，调用方按需访问。

use std::sync::{Arc, Mutex};

use crate::studio::StudioRecoveryIssue;

/// 启动恢复与用户清理期间累积的可操作恢复问题。
#[derive(Debug, Clone)]
pub struct StudioRecoveryRegistry {
    inner: Arc<Mutex<Vec<StudioRecoveryIssue>>>,
}

impl StudioRecoveryRegistry {
    /// 创建空的恢复注册表。
    pub fn new() -> Self {
        Self {
            inner: Arc::new(Mutex::new(Vec::new())),
        }
    }

    /// 用给定问题列表整体替换当前内容。
    ///
    /// 典型调用点是 `initialize_runtime` 完成恢复扫描后的汇总写入。
    pub fn replace(&self, issues: Vec<StudioRecoveryIssue>) {
        let mut inner = self.inner.lock().expect("recovery registry mutex poisoned");
        *inner = issues;
    }

    /// 返回当前所有恢复问题的快照。
    pub fn snapshot(&self) -> Vec<StudioRecoveryIssue> {
        self.inner
            .lock()
            .expect("recovery registry mutex poisoned")
            .clone()
    }

    /// 删除指定 id 的恢复问题，返回剩余问题的快照。
    pub fn remove(&self, issue_id: &str) -> Vec<StudioRecoveryIssue> {
        let mut inner = self.inner.lock().expect("recovery registry mutex poisoned");
        inner.retain(|issue| issue.id != issue_id);
        inner.clone()
    }
}

impl Default for StudioRecoveryRegistry {
    fn default() -> Self {
        Self::new()
    }
}
