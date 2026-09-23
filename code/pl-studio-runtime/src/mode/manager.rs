//! Thread Mode 的原子内存注册表。

use std::collections::{BTreeMap, BTreeSet};
use std::sync::{Arc, RwLock};

use pl_protocol::{ThreadModeCatalogSnapshot, ThreadModeDescriptor, ThreadModeId};
use thiserror::Error;

use super::{
    CompiledWorkflowDefinition, ThreadModeRegistration, WorkflowCompilerError,
    compile_workflow_definition,
};

const MAX_MODE_PROMPT_BYTES: usize = 128 * 1024;
const MAX_MODE_DISPLAY_NAME_CHARS: usize = 128;
const MAX_MODE_DESCRIPTION_BYTES: usize = 4 * 1024;

/// 一个注册来源的稳定身份。
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ThreadModeSourceId(String);

impl ThreadModeSourceId {
    /// 创建经过长度与非空校验的来源 ID。
    ///
    /// # Errors
    ///
    /// ID 为空或超过 128 bytes 时返回 [`ThreadModeManagerError::InvalidSourceId`]。
    pub fn new(value: impl Into<String>) -> Result<Self, ThreadModeManagerError> {
        let value = value.into();
        if value.is_empty() || value.len() > 128 {
            return Err(ThreadModeManagerError::InvalidSourceId(value));
        }
        Ok(Self(value))
    }

    /// 返回来源 ID 的稳定字符串形式。
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Manager 用于保护内置 Mode 的来源类别。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ThreadModeSourceKind {
    /// 随产品二进制发布的注册来源。
    Builtin,
    /// 由上游 loader 或其他内存集成提供的注册来源。
    External,
}

/// 一次批量注册所归属的来源。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ThreadModeSource {
    pub id: ThreadModeSourceId,
    pub kind: ThreadModeSourceKind,
}

impl ThreadModeSource {
    /// 创建随二进制发布的内置来源。
    pub fn builtin(id: ThreadModeSourceId) -> Self {
        Self {
            id,
            kind: ThreadModeSourceKind::Builtin,
        }
    }

    /// 创建由上游提供的外部来源。
    pub fn external(id: ThreadModeSourceId) -> Self {
        Self {
            id,
            kind: ThreadModeSourceKind::External,
        }
    }
}

/// 注册后供 Turn 固定捕获的完整 Mode。
#[derive(Debug, Clone)]
pub struct RegisteredThreadMode {
    descriptor: ThreadModeDescriptor,
    source: ThreadModeSource,
    prompt: Arc<str>,
    graph_revision: u64,
    workflow: Option<Arc<CompiledWorkflowDefinition>>,
}

impl RegisteredThreadMode {
    /// 返回 GUI 与其他目录消费者可见的元数据。
    pub fn descriptor(&self) -> &ThreadModeDescriptor {
        &self.descriptor
    }

    /// 返回提供该 Mode 的注册来源。
    pub fn source(&self) -> &ThreadModeSource {
        &self.source
    }

    /// 返回当前 Mode Prompt；Prompt 不参与图 hash。
    pub fn prompt(&self) -> &str {
        &self.prompt
    }

    /// 返回 Manager 为当前图分配的单调 revision；无图时为零。
    pub fn graph_revision(&self) -> u64 {
        self.graph_revision
    }

    /// 返回已规范化图的结构 hash。
    pub fn graph_hash(&self) -> Option<&str> {
        self.workflow.as_deref().map(|graph| graph.graph_hash())
    }

    /// 返回已校验并编译的不可变状态图。
    pub fn workflow(&self) -> Option<&Arc<CompiledWorkflowDefinition>> {
        self.workflow.as_ref()
    }
}

/// 读取方可跨 `.await` 持有的不可变注册表快照。
#[derive(Debug, Clone, Default)]
pub struct ThreadModeRegistrySnapshot {
    revision: u64,
    modes: BTreeMap<ThreadModeId, Arc<RegisteredThreadMode>>,
    catalog: ThreadModeCatalogSnapshot,
}

impl ThreadModeRegistrySnapshot {
    /// 返回整个注册表快照的 revision。
    pub fn revision(&self) -> u64 {
        self.revision
    }

    /// 按 ID 取得可跨 `.await` 持有的 Mode 快照。
    pub fn mode(&self, id: &ThreadModeId) -> Option<Arc<RegisteredThreadMode>> {
        self.modes.get(id).cloned()
    }

    /// 返回面向目录消费者的只读投影。
    pub fn catalog(&self) -> &ThreadModeCatalogSnapshot {
        &self.catalog
    }

    /// 迭代当前快照中的全部 Mode。
    pub fn modes(&self) -> impl Iterator<Item = &Arc<RegisteredThreadMode>> {
        self.modes.values()
    }
}

#[derive(Debug, Error)]
pub enum ThreadModeManagerError {
    #[error("invalid Thread Mode source id `{0}`")]
    InvalidSourceId(String),
    #[error("Thread Mode `{mode_id}` has invalid metadata: {message}")]
    InvalidRegistration { mode_id: String, message: String },
    #[error("Thread Mode `{mode_id}` has an invalid workflow: {source}")]
    InvalidWorkflow {
        mode_id: String,
        #[source]
        source: WorkflowCompilerError,
    },
    #[error("Thread Mode `{mode_id}` is duplicated in source `{source_id}`")]
    DuplicateInBatch { mode_id: String, source_id: String },
    #[error(
        "Thread Mode `{mode_id}` from source `{source_id}` conflicts with source `{existing_source_id}`"
    )]
    SourceConflict {
        mode_id: String,
        source_id: String,
        existing_source_id: String,
    },
    #[error(
        "Thread Mode source `{source_id}` is already registered as {existing_kind:?} and cannot be replaced as {requested_kind:?}"
    )]
    SourceKindConflict {
        source_id: String,
        existing_kind: ThreadModeSourceKind,
        requested_kind: ThreadModeSourceKind,
    },
}

#[derive(Debug, Clone)]
struct PreparedThreadMode {
    descriptor: ThreadModeDescriptor,
    prompt: Arc<str>,
    workflow: Option<Arc<CompiledWorkflowDefinition>>,
}

#[derive(Debug, Default)]
struct ThreadModeManagerState {
    revision: u64,
    next_graph_revision: u64,
    sources: BTreeMap<ThreadModeSourceId, SourceRegistration>,
    snapshot: Arc<ThreadModeRegistrySnapshot>,
}

#[derive(Debug, Clone)]
struct SourceRegistration {
    source: ThreadModeSource,
    modes: BTreeMap<ThreadModeId, Arc<RegisteredThreadMode>>,
}

/// 按来源原子注册并发布 Thread Mode 的内存 Manager。
#[derive(Debug, Clone, Default)]
pub struct ThreadModeManager {
    state: Arc<RwLock<ThreadModeManagerState>>,
}

impl ThreadModeManager {
    /// 原子替换某一来源的全部 Mode。
    ///
    /// 所有定义先在锁外校验和编译；任一失败时，已发布快照不发生变化。
    pub fn replace_source(
        &self,
        source: ThreadModeSource,
        registrations: impl IntoIterator<Item = ThreadModeRegistration>,
    ) -> Result<Arc<ThreadModeRegistrySnapshot>, ThreadModeManagerError> {
        let prepared = prepare_batch(&source, registrations)?;
        let mut state = self
            .state
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        validate_source_conflicts(&state, &source, &prepared)?;

        let previous = state.sources.get(&source.id).cloned();
        let mut registered = BTreeMap::new();
        for (id, mode) in prepared {
            let previous_mode = previous
                .as_ref()
                .and_then(|registration| registration.modes.get(&id));
            let graph_revision = match (&mode.workflow, previous_mode) {
                (None, _) => 0,
                (Some(graph), Some(previous))
                    if previous.graph_hash() == Some(graph.graph_hash()) =>
                {
                    previous.graph_revision
                }
                (Some(_), _) => {
                    state.next_graph_revision = state.next_graph_revision.saturating_add(1);
                    state.next_graph_revision
                }
            };
            registered.insert(
                id,
                Arc::new(RegisteredThreadMode {
                    descriptor: mode.descriptor,
                    source: source.clone(),
                    prompt: mode.prompt,
                    graph_revision,
                    workflow: mode.workflow,
                }),
            );
        }
        state.sources.insert(
            source.id.clone(),
            SourceRegistration {
                source,
                modes: registered,
            },
        );
        publish(&mut state);
        Ok(state.snapshot.clone())
    }

    /// 移除某一来源的整批 Mode；未知来源保持当前快照。
    pub fn remove_source(&self, source_id: &ThreadModeSourceId) -> Arc<ThreadModeRegistrySnapshot> {
        let mut state = self
            .state
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if state.sources.remove(source_id).is_some() {
            publish(&mut state);
        }
        state.snapshot.clone()
    }

    /// 无锁外借地取得当前不可变快照。
    pub fn snapshot(&self) -> Arc<ThreadModeRegistrySnapshot> {
        self.state
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .snapshot
            .clone()
    }
}

fn prepare_batch(
    source: &ThreadModeSource,
    registrations: impl IntoIterator<Item = ThreadModeRegistration>,
) -> Result<BTreeMap<ThreadModeId, PreparedThreadMode>, ThreadModeManagerError> {
    let mut prepared = BTreeMap::new();
    for registration in registrations {
        validate_registration(&registration)?;
        let id = registration.id.clone();
        let workflow = registration
            .workflow
            .map(compile_workflow_definition)
            .transpose()
            .map_err(|error| ThreadModeManagerError::InvalidWorkflow {
                mode_id: id.to_string(),
                source: error,
            })?
            .map(Arc::new);
        let mode = PreparedThreadMode {
            descriptor: ThreadModeDescriptor {
                id: id.clone(),
                display_name: registration.display_name.trim().to_string(),
                description: registration.description.trim().to_string(),
                order: registration.order,
                has_workflow: workflow.is_some(),
            },
            prompt: Arc::from(registration.prompt.trim()),
            workflow,
        };
        if prepared.insert(id.clone(), mode).is_some() {
            return Err(ThreadModeManagerError::DuplicateInBatch {
                mode_id: id.to_string(),
                source_id: source.id.as_str().to_string(),
            });
        }
    }
    Ok(prepared)
}

fn validate_registration(
    registration: &ThreadModeRegistration,
) -> Result<(), ThreadModeManagerError> {
    let error = |message: String| ThreadModeManagerError::InvalidRegistration {
        mode_id: registration.id.to_string(),
        message,
    };
    let display_name = registration.display_name.trim();
    if display_name.is_empty() || display_name.chars().count() > MAX_MODE_DISPLAY_NAME_CHARS {
        return Err(error(format!(
            "display name must contain 1 to {MAX_MODE_DISPLAY_NAME_CHARS} characters"
        )));
    }
    if registration.description.len() > MAX_MODE_DESCRIPTION_BYTES {
        return Err(error(format!(
            "description exceeds {MAX_MODE_DESCRIPTION_BYTES} bytes"
        )));
    }
    let prompt = registration.prompt.trim();
    if prompt.is_empty() || prompt.len() > MAX_MODE_PROMPT_BYTES {
        return Err(error(format!(
            "prompt must contain 1 to {MAX_MODE_PROMPT_BYTES} bytes"
        )));
    }
    Ok(())
}

fn validate_source_conflicts(
    state: &ThreadModeManagerState,
    source: &ThreadModeSource,
    prepared: &BTreeMap<ThreadModeId, PreparedThreadMode>,
) -> Result<(), ThreadModeManagerError> {
    if let Some(existing) = state.sources.get(&source.id)
        && existing.source.kind != source.kind
    {
        return Err(ThreadModeManagerError::SourceKindConflict {
            source_id: source.id.as_str().to_string(),
            existing_kind: existing.source.kind,
            requested_kind: source.kind,
        });
    }
    for registration in state.sources.values() {
        if registration.source.id == source.id {
            continue;
        }
        if let Some(id) = prepared
            .keys()
            .find(|id| registration.modes.contains_key(*id))
        {
            return Err(ThreadModeManagerError::SourceConflict {
                mode_id: id.to_string(),
                source_id: source.id.as_str().to_string(),
                existing_source_id: registration.source.id.as_str().to_string(),
            });
        }
    }
    Ok(())
}

fn publish(state: &mut ThreadModeManagerState) {
    state.revision = state.revision.saturating_add(1);
    let mut modes = BTreeMap::new();
    for registration in state.sources.values() {
        modes.extend(
            registration
                .modes
                .iter()
                .map(|(id, mode)| (id.clone(), mode.clone())),
        );
    }
    let mut descriptors = modes
        .values()
        .map(|mode| mode.descriptor.clone())
        .collect::<Vec<_>>();
    descriptors.sort_by(|left, right| {
        (left.order, left.display_name.as_str(), left.id.as_str()).cmp(&(
            right.order,
            right.display_name.as_str(),
            right.id.as_str(),
        ))
    });
    let ids = modes.keys().cloned().collect::<BTreeSet<_>>();
    debug_assert_eq!(ids.len(), modes.len());
    state.snapshot = Arc::new(ThreadModeRegistrySnapshot {
        revision: state.revision,
        modes,
        catalog: ThreadModeCatalogSnapshot {
            revision: state.revision,
            modes: descriptors,
        },
    });
}
