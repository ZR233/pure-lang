use std::collections::{BTreeMap, VecDeque};
use std::sync::{Arc, RwLock};
use std::time::{SystemTime, UNIX_EPOCH};

use pl_protocol::{
    ContextSectionId, PinnedContextSection, PureError, SessionNote, TodoListSnapshot,
    ToolResultReceipt,
};
use sha2::{Digest, Sha256};

use crate::AgentSession;

pub const CURRENT_TODO_SECTION_ID: &str = "pl.current_todo";
pub const CONVERSATION_RECOVERY_SECTION_ID: &str = "pl.conversation_recovery";
pub const EVIDENCE_LEDGER_SECTION_ID: &str = "pl.evidence_ledger";
pub const REVIEW_MANIFEST_SECTION_ID: &str = "mai.review_manifest";
pub const REVIEW_CHECKPOINT_SECTION_ID: &str = "mai.review_checkpoint";

pub const MAX_PINNED_SECTION_BYTES: usize = 32 * 1024;
pub const MAX_PINNED_CONTEXT_BYTES: usize = 96 * 1024;
pub const MAX_SESSION_NOTE_BYTES: usize = 1024 * 1024;
const MAX_TODO_ITEMS: usize = 32;
const MAX_TODO_STEP_CHARS: usize = 256;
const MAX_TODO_BYTES: usize = 8 * 1024;
const MAX_EVIDENCE_ITEMS: usize = 64;
const MAX_EVIDENCE_VISIBLE_BYTES: usize = 16 * 1024;
const MAX_ARCHIVED_ARTIFACT_REFS: usize = 16;

/// 单轮内可立即读取、并可同步回 canonical session 的工作上下文。
#[derive(Clone, Default)]
pub struct TurnWorkingSetHandle {
    inner: Arc<RwLock<TurnWorkingSet>>,
}

impl std::fmt::Debug for TurnWorkingSetHandle {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("TurnWorkingSetHandle")
            .field("section_count", &self.sections().len())
            .finish()
    }
}

/// 对 turn working set 的原子语义变更。
#[derive(Debug, Clone)]
pub enum TurnWorkingSetChange {
    ReplaceTodo(TodoListSnapshot),
    UpsertSection(PinnedContextSection),
    RemoveSection(ContextSectionId),
    AppendEvidence(ToolResultReceipt),
}

#[derive(Clone, Default)]
struct TurnWorkingSet {
    sections: BTreeMap<ContextSectionId, PinnedContextSection>,
    evidence: EvidenceLedgerDocument,
    session_note: Option<SessionNote>,
}

#[derive(Clone, Default, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct EvidenceLedgerDocument {
    recent: VecDeque<ToolResultReceipt>,
    archived: EvidenceArchiveIndex,
}

#[derive(Clone, Default, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct EvidenceArchiveIndex {
    count: u64,
    digest: String,
    artifact_refs: VecDeque<serde_json::Value>,
    artifact_refs_omitted: u64,
}

impl TurnWorkingSetHandle {
    pub fn from_session(session: &AgentSession) -> Result<Self, PureError> {
        let handle = Self::default();
        for section in session.pinned_context_sections() {
            handle.apply(TurnWorkingSetChange::UpsertSection(section.clone()))?;
        }
        if let Some(note) = session.session_note() {
            validate_session_note(note)?;
        }
        handle
            .inner
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .session_note = session.session_note().cloned();
        Ok(handle)
    }

    pub fn apply(&self, change: TurnWorkingSetChange) -> Result<(), PureError> {
        let mut state = self
            .inner
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let mut next = state.clone();
        match change {
            TurnWorkingSetChange::ReplaceTodo(snapshot) => {
                validate_todo(&snapshot)?;
                let content = serde_json::to_string_pretty(&snapshot)?;
                if content.len() > MAX_TODO_BYTES {
                    return Err(PureError::ConfigError(format!(
                        "todo working context exceeds {MAX_TODO_BYTES} bytes"
                    )));
                }
                let id = context_section_id(CURRENT_TODO_SECTION_ID);
                let revision = next_revision(&next.sections, &id);
                let section = section(id, revision, "Current Todo", content);
                validate_section(&section)?;
                next.sections.insert(section.id.clone(), section);
            }
            TurnWorkingSetChange::UpsertSection(section) => {
                validate_section(&section)?;
                if section.id.as_str() == CURRENT_TODO_SECTION_ID {
                    validate_todo_section(&section)?;
                }
                if section.id.as_str() == EVIDENCE_LEDGER_SECTION_ID {
                    if section.content.len() > MAX_EVIDENCE_VISIBLE_BYTES {
                        return Err(PureError::ConfigError(format!(
                            "evidence ledger exceeds {MAX_EVIDENCE_VISIBLE_BYTES} bytes"
                        )));
                    }
                    next.evidence = serde_json::from_str(&section.content).map_err(|error| {
                        PureError::ConfigError(format!(
                            "evidence ledger contains invalid receipts: {error}"
                        ))
                    })?;
                    while next.evidence.recent.len() > MAX_EVIDENCE_ITEMS {
                        archive_oldest_evidence(&mut next.evidence)?;
                    }
                }
                next.sections.insert(section.id.clone(), section);
            }
            TurnWorkingSetChange::RemoveSection(id) => {
                next.sections.remove(&id);
                if id.as_str() == EVIDENCE_LEDGER_SECTION_ID {
                    next.evidence = EvidenceLedgerDocument::default();
                }
            }
            TurnWorkingSetChange::AppendEvidence(receipt) => {
                next.evidence.recent.push_back(receipt);
                while next.evidence.recent.len() > MAX_EVIDENCE_ITEMS {
                    archive_oldest_evidence(&mut next.evidence)?;
                }
                let id = context_section_id(EVIDENCE_LEDGER_SECTION_ID);
                let revision = next_revision(&next.sections, &id);
                let content = bounded_evidence_content(&mut next.evidence)?;
                let section = section(id, revision, "Evidence Ledger", content);
                validate_section(&section)?;
                next.sections.insert(section.id.clone(), section);
            }
        }
        validate_total_size(next.sections.values())?;
        *state = next;
        Ok(())
    }

    pub fn sections(&self) -> Vec<PinnedContextSection> {
        self.inner
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .sections
            .values()
            .cloned()
            .collect()
    }

    /// 返回当前 todo list 快照；尚未建立时返回 `None`。
    ///
    /// 写入和会话恢复边界会拒绝损坏或违反 todo 约束的内置 section。
    pub fn current_todo(&self) -> Option<TodoListSnapshot> {
        let section = self
            .sections()
            .into_iter()
            .find(|section| section.id.as_str() == CURRENT_TODO_SECTION_ID)?;
        serde_json::from_str(&section.content).ok()
    }

    /// 返回当前会话笔记；尚未创建时返回 revision 为 0 的空快照。
    pub fn session_note(&self) -> SessionNote {
        self.inner
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .session_note
            .clone()
            .unwrap_or_else(empty_session_note)
    }

    /// 在 revision 匹配时原子替换会话笔记正文。
    pub fn replace_session_note(
        &self,
        expected_revision: u64,
        content: String,
    ) -> Result<SessionNote, PureError> {
        validate_session_note_content(&content)?;
        let mut state = self
            .inner
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let current_revision = state.session_note.as_ref().map_or(0, |note| note.revision);
        if expected_revision != current_revision {
            return Err(PureError::ConfigError(format!(
                "session note revision mismatch: expected {expected_revision}, current {current_revision}"
            )));
        }
        let note = SessionNote {
            revision: current_revision.saturating_add(1),
            content_hash: canonical_content_hash(content.as_bytes()),
            content,
            updated_at: unix_seconds(),
        };
        state.session_note = Some(note.clone());
        Ok(note)
    }

    pub fn sync_session(&self, session: &mut AgentSession) -> Result<bool, PureError> {
        let sections = self.sections();
        validate_total_size(sections.iter())?;
        let mut changed = session.replace_pinned_context_sections(sections);
        let note = self
            .inner
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .session_note
            .clone();
        if let Some(note) = note {
            changed |= session.replace_session_note(note);
        }
        Ok(changed)
    }
}

fn empty_session_note() -> SessionNote {
    SessionNote {
        revision: 0,
        content: String::new(),
        content_hash: canonical_content_hash(&[]),
        updated_at: 0,
    }
}

fn validate_session_note_content(content: &str) -> Result<(), PureError> {
    if content.len() > MAX_SESSION_NOTE_BYTES {
        return Err(PureError::ConfigError(format!(
            "session note exceeds {MAX_SESSION_NOTE_BYTES} bytes"
        )));
    }
    Ok(())
}

fn validate_session_note(note: &SessionNote) -> Result<(), PureError> {
    validate_session_note_content(&note.content)?;
    let actual_hash = canonical_content_hash(note.content.as_bytes());
    if note.content_hash != actual_hash {
        return Err(PureError::ConfigError(
            "session note has an invalid content hash".to_string(),
        ));
    }
    Ok(())
}

/// 计算可跨产品复用的稳定内容摘要。
pub fn canonical_content_hash(content: &[u8]) -> String {
    let digest = Sha256::digest(content);
    format!("sha256:{digest:x}")
}

/// 对 JSON 对象键排序后计算摘要，避免等价参数因字段顺序不同产生不同 receipt。
pub fn canonical_json_hash(value: &serde_json::Value) -> String {
    canonical_content_hash(canonical_json_string(value).as_bytes())
}

pub(crate) fn canonical_json_string(value: &serde_json::Value) -> String {
    match value {
        serde_json::Value::Object(map) => {
            let sorted = map
                .iter()
                .map(|(key, value)| (key.as_str(), canonical_json_string(value)))
                .collect::<BTreeMap<_, _>>();
            format!(
                "{{{}}}",
                sorted
                    .into_iter()
                    .map(|(key, value)| format!(
                        "{}:{value}",
                        serde_json::to_string(key)
                            .expect("canonical JSON key serialization must succeed")
                    ))
                    .collect::<Vec<_>>()
                    .join(",")
            )
        }
        serde_json::Value::Array(items) => format!(
            "[{}]",
            items
                .iter()
                .map(canonical_json_string)
                .collect::<Vec<_>>()
                .join(",")
        ),
        serde_json::Value::Null
        | serde_json::Value::Bool(_)
        | serde_json::Value::Number(_)
        | serde_json::Value::String(_) => value.to_string(),
    }
}

pub fn context_section(
    id: impl Into<String>,
    revision: u64,
    title: impl Into<String>,
    content: impl Into<String>,
) -> Result<PinnedContextSection, PureError> {
    let id =
        ContextSectionId::new(id).map_err(|error| PureError::ConfigError(error.to_string()))?;
    let section = section(id, revision, title, content.into());
    validate_section(&section)?;
    Ok(section)
}

fn section(
    id: ContextSectionId,
    revision: u64,
    title: impl Into<String>,
    content: String,
) -> PinnedContextSection {
    PinnedContextSection {
        id,
        revision,
        title: title.into(),
        content_hash: canonical_content_hash(content.as_bytes()),
        content,
        updated_at: unix_seconds(),
    }
}

fn context_section_id(value: &str) -> ContextSectionId {
    ContextSectionId::new(value).expect("built-in context section id must be valid")
}

fn next_revision(
    sections: &BTreeMap<ContextSectionId, PinnedContextSection>,
    id: &ContextSectionId,
) -> u64 {
    sections
        .get(id)
        .map_or(1, |section| section.revision.saturating_add(1))
}

fn validate_section(section: &PinnedContextSection) -> Result<(), PureError> {
    if section.content.len() > MAX_PINNED_SECTION_BYTES {
        return Err(PureError::ConfigError(format!(
            "pinned context section `{}` exceeds {MAX_PINNED_SECTION_BYTES} bytes",
            section.id
        )));
    }
    let actual_hash = canonical_content_hash(section.content.as_bytes());
    if section.content_hash != actual_hash {
        return Err(PureError::ConfigError(format!(
            "pinned context section `{}` has an invalid content hash",
            section.id
        )));
    }
    Ok(())
}

fn validate_total_size<'a>(
    sections: impl IntoIterator<Item = &'a PinnedContextSection>,
) -> Result<(), PureError> {
    let total = sections
        .into_iter()
        .map(|section| section.content.len())
        .sum::<usize>();
    if total > MAX_PINNED_CONTEXT_BYTES {
        return Err(PureError::ConfigError(format!(
            "pinned context exceeds {MAX_PINNED_CONTEXT_BYTES} bytes"
        )));
    }
    Ok(())
}

fn validate_todo(snapshot: &TodoListSnapshot) -> Result<(), PureError> {
    if snapshot.items.len() > MAX_TODO_ITEMS {
        return Err(PureError::ToolExecutionFailed {
            tool: "update_todo_list".to_string(),
            error: format!("todo list may contain at most {MAX_TODO_ITEMS} items"),
        });
    }
    if let Some(item) = snapshot
        .items
        .iter()
        .find(|item| item.step.chars().count() > MAX_TODO_STEP_CHARS)
    {
        return Err(PureError::ToolExecutionFailed {
            tool: "update_todo_list".to_string(),
            error: format!(
                "todo item exceeds {MAX_TODO_STEP_CHARS} characters: {}",
                item.step.chars().take(32).collect::<String>()
            ),
        });
    }
    Ok(())
}

fn validate_todo_section(section: &PinnedContextSection) -> Result<(), PureError> {
    if section.content.len() > MAX_TODO_BYTES {
        return Err(PureError::ConfigError(format!(
            "todo working context exceeds {MAX_TODO_BYTES} bytes"
        )));
    }
    let snapshot = serde_json::from_str::<TodoListSnapshot>(&section.content).map_err(|error| {
        PureError::ConfigError(format!(
            "current todo section contains invalid JSON: {error}"
        ))
    })?;
    validate_todo(&snapshot)
}

fn bounded_evidence_content(evidence: &mut EvidenceLedgerDocument) -> Result<String, PureError> {
    loop {
        let content = serde_json::to_string_pretty(evidence)?;
        if content.len() <= MAX_EVIDENCE_VISIBLE_BYTES {
            return Ok(content);
        }
        // 单个 receipt 也可能携带宿主提供的大型 artifact 描述。工作上下文是
        // 辅助索引，不能因为一个不可见的大对象而让 canonical tool history
        // 提交失败；放不下时将该 receipt 降级为摘要索引。
        if !evidence.recent.is_empty() {
            archive_oldest_evidence(evidence)?;
            continue;
        }
        if evidence.archived.artifact_refs.pop_front().is_some() {
            evidence.archived.artifact_refs_omitted =
                evidence.archived.artifact_refs_omitted.saturating_add(1);
            continue;
        }
        return Err(PureError::ConfigError(format!(
            "evidence ledger cannot fit within {MAX_EVIDENCE_VISIBLE_BYTES} bytes"
        )));
    }
}

fn archive_oldest_evidence(evidence: &mut EvidenceLedgerDocument) -> Result<(), PureError> {
    let Some(receipt) = evidence.recent.pop_front() else {
        return Ok(());
    };
    let receipt_json = serde_json::to_vec(&receipt)?;
    evidence.archived.digest = canonical_content_hash(
        [evidence.archived.digest.as_bytes(), receipt_json.as_slice()]
            .concat()
            .as_slice(),
    );
    evidence.archived.count = evidence.archived.count.saturating_add(1);
    evidence.archived.artifact_refs.extend(receipt.artifacts);
    while evidence.archived.artifact_refs.len() > MAX_ARCHIVED_ARTIFACT_REFS {
        evidence.archived.artifact_refs.pop_front();
        evidence.archived.artifact_refs_omitted =
            evidence.archived.artifact_refs_omitted.saturating_add(1);
    }
    Ok(())
}

fn unix_seconds() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}

#[cfg(test)]
mod tests {
    use pl_protocol::{TodoItem, TodoStatus};
    use pretty_assertions::assert_eq;

    use super::*;

    #[test]
    fn todo_and_evidence_are_kept_as_sorted_pinned_sections() {
        let handle = TurnWorkingSetHandle::default();
        handle
            .apply(TurnWorkingSetChange::ReplaceTodo(TodoListSnapshot {
                call_id: "todo-1".to_string(),
                agent_id: None,
                path: Some("/root".to_string()),
                parent_path: None,
                explanation: None,
                items: vec![TodoItem {
                    step: "Inspect".to_string(),
                    status: TodoStatus::InProgress,
                }],
            }))
            .unwrap();
        handle
            .apply(TurnWorkingSetChange::AppendEvidence(ToolResultReceipt {
                call_id: "read-1".to_string(),
                tool_name: "read_file".to_string(),
                arguments_hash: "sha256:args".to_string(),
                result_hash: "sha256:result".to_string(),
                total_bytes: 10,
                visible_bytes: 10,
                truncated: false,
                artifacts: Vec::new(),
                continuation: None,
                reused_from_call_id: None,
            }))
            .unwrap();

        assert_eq!(
            handle
                .sections()
                .iter()
                .map(|section| section.id.as_str())
                .collect::<Vec<_>>(),
            vec![CURRENT_TODO_SECTION_ID, EVIDENCE_LEDGER_SECTION_ID]
        );
    }

    #[test]
    fn current_todo_round_trips_the_pinned_section() {
        let handle = TurnWorkingSetHandle::default();
        assert!(handle.current_todo().is_none());

        handle
            .apply(TurnWorkingSetChange::ReplaceTodo(TodoListSnapshot {
                call_id: "todo-1".to_string(),
                agent_id: None,
                path: Some("/root".to_string()),
                parent_path: None,
                explanation: None,
                items: vec![
                    TodoItem {
                        step: "Done".to_string(),
                        status: TodoStatus::Completed,
                    },
                    TodoItem {
                        step: "Left".to_string(),
                        status: TodoStatus::Pending,
                    },
                ],
            }))
            .unwrap();

        let todo = handle.current_todo().unwrap();
        assert_eq!(todo.call_id, "todo-1");
        assert_eq!(todo.items.len(), 2);
        assert_eq!(todo.items[1].status, TodoStatus::Pending);
    }

    #[test]
    fn upsert_rejects_a_malformed_current_todo_section() {
        let handle = TurnWorkingSetHandle::default();
        let section =
            context_section(CURRENT_TODO_SECTION_ID, 1, "Current Todo", "not-json").unwrap();

        let error = handle
            .apply(TurnWorkingSetChange::UpsertSection(section))
            .unwrap_err();

        assert!(matches!(
            error,
            PureError::ConfigError(message)
                if message.contains("current todo section contains invalid JSON")
        ));
        assert!(handle.current_todo().is_none());
    }

    #[test]
    fn upsert_applies_todo_constraints_to_the_current_todo_section() {
        let handle = TurnWorkingSetHandle::default();
        let snapshot = TodoListSnapshot {
            call_id: "todo-1".to_string(),
            agent_id: None,
            path: Some("/root".to_string()),
            parent_path: None,
            explanation: None,
            items: (0..=MAX_TODO_ITEMS)
                .map(|index| TodoItem {
                    step: format!("Item {index}"),
                    status: TodoStatus::Pending,
                })
                .collect(),
        };
        let section = context_section(
            CURRENT_TODO_SECTION_ID,
            1,
            "Current Todo",
            serde_json::to_string(&snapshot).unwrap(),
        )
        .unwrap();

        let error = handle
            .apply(TurnWorkingSetChange::UpsertSection(section))
            .unwrap_err();

        assert!(matches!(
            error,
            PureError::ToolExecutionFailed { tool, error }
                if tool == "update_todo_list"
                    && error.contains("todo list may contain at most")
        ));
        assert!(handle.current_todo().is_none());
    }

    #[test]
    fn restored_evidence_ledger_is_extended_instead_of_replaced() {
        let first = TurnWorkingSetHandle::default();
        first
            .apply(TurnWorkingSetChange::AppendEvidence(receipt("read-1")))
            .unwrap();
        let mut session = AgentSession::new();
        first.sync_session(&mut session).unwrap();
        let restored = TurnWorkingSetHandle::from_session(&session).unwrap();

        restored
            .apply(TurnWorkingSetChange::AppendEvidence(receipt("read-2")))
            .unwrap();

        let evidence = restored
            .sections()
            .into_iter()
            .find(|section| section.id.as_str() == EVIDENCE_LEDGER_SECTION_ID)
            .unwrap();
        let receipts = serde_json::from_str::<EvidenceLedgerDocument>(&evidence.content).unwrap();
        assert_eq!(
            receipts
                .recent
                .into_iter()
                .map(|receipt| receipt.call_id)
                .collect::<Vec<_>>(),
            vec!["read-1".to_string(), "read-2".to_string()]
        );
    }

    #[test]
    fn session_note_round_trips_through_working_set_and_sync() {
        let handle = TurnWorkingSetHandle::default();
        let first = handle
            .replace_session_note(0, "重要节点".to_string())
            .unwrap();
        let mut session = AgentSession::new();

        assert!(handle.sync_session(&mut session).unwrap());
        let restored = TurnWorkingSetHandle::from_session(&session).unwrap();
        let second = restored
            .replace_session_note(first.revision, "更新节点".to_string())
            .unwrap();

        assert_eq!(restored.session_note(), second);
        assert_eq!(second.revision, 2);
        assert_eq!(second.content, "更新节点");
    }

    #[test]
    fn old_evidence_is_reduced_to_a_durable_archive_index() {
        let handle = TurnWorkingSetHandle::default();
        for index in 0..=MAX_EVIDENCE_ITEMS {
            handle
                .apply(TurnWorkingSetChange::AppendEvidence(receipt(&format!(
                    "read-{index}"
                ))))
                .unwrap();
        }
        let section = handle
            .sections()
            .into_iter()
            .find(|section| section.id.as_str() == EVIDENCE_LEDGER_SECTION_ID)
            .unwrap();
        let ledger = serde_json::from_str::<EvidenceLedgerDocument>(&section.content).unwrap();

        assert_eq!(ledger.recent.len(), MAX_EVIDENCE_ITEMS);
        assert_eq!(ledger.archived.count, 1);
        assert!(!ledger.archived.digest.is_empty());
    }

    #[test]
    fn oversized_single_receipt_is_archived_instead_of_failing_the_turn() {
        let handle = TurnWorkingSetHandle::default();
        let mut receipt = receipt("web-search-1");
        receipt.artifacts = vec![serde_json::json!({
            "kind": "webSearch",
            "results": "x".repeat(MAX_EVIDENCE_VISIBLE_BYTES * 2),
        })];

        handle
            .apply(TurnWorkingSetChange::AppendEvidence(receipt))
            .unwrap();

        let evidence = handle
            .sections()
            .into_iter()
            .find(|section| section.id.as_str() == EVIDENCE_LEDGER_SECTION_ID)
            .unwrap();
        let ledger = serde_json::from_str::<EvidenceLedgerDocument>(&evidence.content).unwrap();
        assert_eq!(ledger.recent.len(), 0);
        assert_eq!(ledger.archived.count, 1);
        assert!(evidence.content.len() <= MAX_EVIDENCE_VISIBLE_BYTES);
    }

    #[test]
    fn canonical_json_hash_ignores_object_order_but_keeps_json_types() {
        let first = serde_json::json!({"b": [2, 3], "a": 1});
        let reordered = serde_json::json!({"a": 1, "b": [2, 3]});
        let string_value = serde_json::json!({"a": "1", "b": [2, 3]});

        assert_eq!(canonical_json_hash(&first), canonical_json_hash(&reordered));
        assert_ne!(
            canonical_json_hash(&first),
            canonical_json_hash(&string_value)
        );
    }

    fn receipt(call_id: &str) -> ToolResultReceipt {
        ToolResultReceipt {
            call_id: call_id.to_string(),
            tool_name: "read_file".to_string(),
            arguments_hash: "sha256:args".to_string(),
            result_hash: "sha256:result".to_string(),
            total_bytes: 10,
            visible_bytes: 10,
            truncated: false,
            artifacts: Vec::new(),
            continuation: None,
            reused_from_call_id: None,
        }
    }
}
