use std::collections::BTreeMap;
use std::fmt;
use std::sync::{Arc, RwLock};

use tokio::sync::watch;

use super::contract::Tool;

/// 工具来源的 opaque 标识。
///
/// 来源只用于注册表的命名空间所有权与诊断；core 不按来源或名称前缀对工具
/// 做任何调度分支。新增来源不需要修改 core。
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ToolSourceId(Arc<str>);

impl ToolSourceId {
    /// 内建与宿主静态工具（`tool_set` 组装、`web_search`）。
    pub fn builtin() -> Self {
        Self::new("builtin")
    }

    /// agent 协作工具来源。
    pub fn collaboration() -> Self {
        Self::new("collaboration")
    }

    /// Task 协调工具来源。
    pub fn task() -> Self {
        Self::new("task")
    }

    /// LSP 能力 seam 来源。
    pub fn lsp() -> Self {
        Self::new("lsp")
    }

    /// MCP 来源（全部 server 合并发布）。
    pub fn mcp() -> Self {
        Self::new("mcp")
    }

    pub fn new(value: impl AsRef<str>) -> Self {
        Self(Arc::from(value.as_ref()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for ToolSourceId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

/// 延迟加载工具的命名空间描述。
///
/// 命名空间是 Tool Search catalog 的分组单位；描述参与检索文本。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NamespaceDescriptor {
    pub name: String,
    pub description: String,
}

impl NamespaceDescriptor {
    pub fn new(name: impl Into<String>, description: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            description: description.into(),
        }
    }
}

/// 工具条目的来源元数据。
///
/// `programmatic_eligible` 只表示发布方声明该工具可被 program 调用；最终资格
/// 仍要求 `effect == Some(Read)`。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolSourceMetadata {
    pub source: ToolSourceId,
    pub namespace: Option<NamespaceDescriptor>,
    pub programmatic_eligible: bool,
}

impl ToolSourceMetadata {
    pub fn new(source: ToolSourceId) -> Self {
        Self {
            source,
            namespace: None,
            programmatic_eligible: false,
        }
    }

    pub fn with_namespace(mut self, namespace: NamespaceDescriptor) -> Self {
        self.namespace = Some(namespace);
        self
    }

    /// 声明 programmatic 调用资格（仍要求 effect 为 `Read`）。
    pub fn programmatic(mut self) -> Self {
        self.programmatic_eligible = true;
        self
    }
}

/// 注册表中的单个工具：实现与来源元数据的不可变配对。
#[derive(Clone)]
pub struct ToolEntry {
    tool: Arc<dyn Tool>,
    metadata: ToolSourceMetadata,
}

impl ToolEntry {
    pub fn new(tool: impl Tool + 'static, metadata: ToolSourceMetadata) -> Self {
        Self {
            tool: Arc::new(tool),
            metadata,
        }
    }

    pub fn from_arc(tool: Arc<dyn Tool>, metadata: ToolSourceMetadata) -> Self {
        Self { tool, metadata }
    }

    pub fn name(&self) -> &str {
        self.tool.name()
    }

    pub fn tool(&self) -> &dyn Tool {
        self.tool.as_ref()
    }

    pub fn metadata(&self) -> &ToolSourceMetadata {
        &self.metadata
    }

    /// 判断两个条目是否语义相等（名称、canonical schema 与元数据）。
    pub(super) fn semantic_eq(&self, other: &Self) -> bool {
        self.name() == other.name()
            && self.metadata == other.metadata
            && schema_canonical_string(self.tool.to_schema())
                == schema_canonical_string(other.tool.to_schema())
    }
}

impl fmt::Debug for ToolEntry {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ToolEntry")
            .field("name", &self.name())
            .field("metadata", &self.metadata)
            .finish_non_exhaustive()
    }
}

fn schema_canonical_string(schema: pl_model::ToolSchema) -> String {
    serde_json::to_value(&schema).map_or_else(
        |_| String::new(),
        |value| crate::working_set::canonical_json_string(&value),
    )
}

/// 注册表全局发布代数，只作诊断，不参与缓存轮换。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct RegistryRevision(pub u64);

/// 注册表在某一时刻的全来源合并快照。
///
/// 条目按模型可见名称排序；持有序列化 Arc 的活动 Turn 不受后续 publish 影响。
#[derive(Debug, Clone)]
pub struct RegistrySnapshot {
    pub revision: RegistryRevision,
    pub entries: Arc<[ToolEntry]>,
}

impl RegistrySnapshot {
    pub fn names(&self) -> Vec<&str> {
        self.entries.iter().map(ToolEntry::name).collect()
    }

    pub fn entry(&self, name: &str) -> Option<&ToolEntry> {
        self.entries.iter().find(|entry| entry.name() == name)
    }
}

/// 发布状态；`seq` 用于 guard 的代际取代判定。
#[derive(Debug)]
pub(super) struct PublishedSource {
    pub(super) seq: u64,
    pub(super) entries: Arc<[ToolEntry]>,
}

#[derive(Debug, Default)]
pub(super) struct RegistryState {
    pub(super) revision: u64,
    pub(super) next_seq: u64,
    pub(super) sources: BTreeMap<ToolSourceId, PublishedSource>,
}

/// 按来源代际发布的工具注册表。
///
/// builtin、host、MCP、LSP 都是它的客户：每个来源通过 [`ToolRegistry::publish`]
/// 整组发布，注册表校验后原子替换该来源条目并递增全局 revision；活动快照持有
/// 序列化的条目集合，不受后续发布影响。
#[derive(Clone)]
pub struct ToolRegistry {
    inner: Arc<RegistryInner>,
}

struct RegistryInner {
    state: RwLock<RegistryState>,
    revision_tx: watch::Sender<RegistryRevision>,
}

impl fmt::Debug for ToolRegistry {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ToolRegistry")
            .field("revision", &self.revision())
            .finish_non_exhaustive()
    }
}

impl Default for ToolRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl ToolRegistry {
    pub fn new() -> Self {
        let (revision_tx, _) = watch::channel(RegistryRevision::default());
        Self {
            inner: Arc::new(RegistryInner {
                state: RwLock::new(RegistryState::default()),
                revision_tx,
            }),
        }
    }

    /// 返回当前全来源合并快照（短锁内克隆 Arc）。
    pub fn snapshot(&self) -> RegistrySnapshot {
        let state = self.lock_state();
        let mut entries = Vec::new();
        for source in state.sources.values() {
            entries.extend(source.entries.iter().cloned());
        }
        let revision = RegistryRevision(state.revision);
        drop(state);
        entries.sort_by(|left, right| left.name().cmp(right.name()));
        RegistrySnapshot {
            revision,
            entries: entries.into(),
        }
    }

    /// 当前全局 revision。
    pub fn revision(&self) -> RegistryRevision {
        RegistryRevision(self.lock_state().revision)
    }

    /// 订阅注册表变更通知；每次发布或注销后收到最新 revision。
    pub fn subscribe(&self) -> watch::Receiver<RegistryRevision> {
        self.inner.revision_tx.subscribe()
    }

    fn lock_state(&self) -> std::sync::RwLockReadGuard<'_, RegistryState> {
        // 内部状态只在 publish/unpublish 中短暂持锁，锁中毒表示发布路径出现
        // panic；此时注册表不可再用，按空状态降级而不是在快照路径 panic。
        self.inner
            .state
            .read()
            .unwrap_or_else(|poison| poison.into_inner())
    }

    /// 供 `registry.rs` 中的发布实现使用；不得跨 await 或回调持有。
    pub(super) fn lock_state_mut(&self) -> std::sync::RwLockWriteGuard<'_, RegistryState> {
        self.inner
            .state
            .write()
            .unwrap_or_else(|poison| poison.into_inner())
    }

    /// 广播一次 revision 变化；`watch::Sender::send` 不阻塞。
    pub(super) fn broadcast_revision(&self, revision: RegistryRevision) {
        let _ = self.inner.revision_tx.send(revision);
    }
}
