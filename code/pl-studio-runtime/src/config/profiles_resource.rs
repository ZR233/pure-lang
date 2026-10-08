//! 配置 owner 持有的 canonical Agent Profiles 资源。
//!
//! 启动装载、配置命令与显式重扫采用一次新的目录扫描；查询与订阅只读取已发布
//! 缓存，不扫描文件、不周期轮询（design/18 §18.4）。资源与运行期 Agent directory
//! 是两个领域，互不替代。

use std::sync::{Arc, Mutex};

use tokio::sync::watch;

use crate::studio::unix_seconds;
use crate::{
    StudioAgentProfileDiagnostic, StudioAgentProfilesData, StudioAgentProfilesStateSnapshot,
};

use super::{AgentProfileCatalog, ConfigPaths, StudioConfig};

/// 已发布的 Agent Profiles 资源事实。
///
/// `catalog` 是完整配置视图（含被禁用的系统与用户 Profile）加逐文件诊断；
/// `revision` 只在目录内容真实变化时前进，空 delta 不提升 revision。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentProfilesSnapshot {
    pub revision: u64,
    pub updated_at: i64,
    pub catalog: AgentProfileCatalog,
}

/// canonical Agent Profiles 资源：缓存事实 + 变更唤醒通道。
#[derive(Clone)]
pub struct AgentProfilesResource {
    state: Arc<Mutex<AgentProfilesSnapshot>>,
    updates: Arc<watch::Sender<u64>>,
}

impl AgentProfilesResource {
    /// 启动装载：执行一次目录扫描并发布首帧事实（revision 从 0 前进到 1）。
    pub fn new(paths: &ConfigPaths, config: &StudioConfig) -> Self {
        let catalog = AgentProfileCatalog::discover_for_settings(paths, config);
        let (updates, _) = watch::channel(0);
        Self {
            state: Arc::new(Mutex::new(AgentProfilesSnapshot {
                revision: 1,
                updated_at: unix_seconds(),
                catalog,
            })),
            updates: Arc::new(updates),
        }
    }

    /// 读取已发布资源；纯内存克隆，不触发文件系统访问。
    pub fn read(&self) -> AgentProfilesSnapshot {
        self.state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
    }

    /// 订阅资源变更唤醒；通道值为最新 revision。
    pub fn subscribe(&self) -> watch::Receiver<u64> {
        self.updates.subscribe()
    }

    /// 采用一次新的目录扫描；目录未变化时不提升 revision、不通知观察者。
    pub fn adopt_scan(&self, paths: &ConfigPaths, config: &StudioConfig) {
        let catalog = AgentProfileCatalog::discover_for_settings(paths, config);
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if state.catalog == catalog {
            return;
        }
        state.revision = state.revision.saturating_add(1);
        state.updated_at = unix_seconds();
        state.catalog = catalog;
        let revision = state.revision;
        drop(state);
        self.updates.send_replace(revision);
    }
}

impl From<AgentProfilesSnapshot> for StudioAgentProfilesStateSnapshot {
    fn from(snapshot: AgentProfilesSnapshot) -> Self {
        Self {
            state: pl_protocol::ObservedResource::ready(
                snapshot.revision,
                snapshot.updated_at,
                StudioAgentProfilesData {
                    profiles: snapshot.catalog.profiles,
                    diagnostics: snapshot
                        .catalog
                        .diagnostics
                        .into_iter()
                        .map(|diagnostic| StudioAgentProfileDiagnostic {
                            path: diagnostic.path.to_string_lossy().into_owned(),
                            message: diagnostic.message,
                        })
                        .collect(),
                },
            ),
        }
    }
}
