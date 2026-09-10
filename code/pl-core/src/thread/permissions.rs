//! Typed execution decisions bound to a live tool call; business prompts remain opaque.
use super::*;

/// Explicit host decision, never derived from business payload fields.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum PermissionDecision {
    Allow,
    Deny,
}

/// Saved permission state. A historical Allow is not a reusable executor capability.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum PermissionState {
    Pending,
    Allowed,
    Denied,
    Cancelled,
}

/// Original approval prompt and its exact framework association.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PermissionRecord {
    pub created_at: i64,
    pub updated_at: i64,
    pub response: Option<OpaquePayload>,
    pub id: String,
    pub task_id: String,
    pub call_id: String,
    pub turn_id: String,
    pub tool_id: String,
    pub revision: u64,
    pub payload: OpaquePayload,
    pub state: PermissionState,
}

/// Revision-checked decision supplied through a trusted host command.
#[derive(Debug, Clone)]
pub struct PermissionResolution {
    pub id: String,
    pub expected_revision: u64,
    pub decision: PermissionDecision,
    pub payload: Option<OpaquePayload>,
}

impl Owner {
    pub(super) fn request_execution_permission(
        &mut self,
        caller: String,
        authority: crate::tool::opaque::ExecutionAuthority,
        payload: OpaquePayload,
    ) -> Result<PermissionRecord, ThreadError> {
        if self.state.lifecycle != ThreadLifecycle::Open || self.interrupt.is_closing() {
            return Err(ThreadError::Closed);
        }
        let task = self
            .state
            .tasks
            .get(&caller)
            .ok_or(ThreadError::InvalidIdentity)?;
        if task.status != task::TaskStatus::Running
            || task.cancel_requested
            || self
                .task_tokens
                .get(&caller)
                .is_none_or(CancellationToken::is_cancelled)
        {
            return Err(ThreadError::TaskAccessExpired);
        }
        if !authority.remains_authorized(&self.tools) {
            return Err(ThreadError::ToolPermissionRevoked);
        }
        let id = format!("permission:{}", task.call_id);
        if let Some(previous) = self.state.permissions.get(&id) {
            return if previous.payload == payload && self.permission_leases.contains_key(&id) {
                Ok(previous.clone())
            } else {
                Err(ThreadError::InvalidIdentity)
            };
        }
        let now = crate::time::unix_seconds();
        let record = PermissionRecord {
            created_at: now,
            updated_at: now,
            response: None,
            id: id.clone(),
            task_id: caller,
            call_id: task.call_id.clone(),
            turn_id: task.turn_id.clone(),
            tool_id: task.tool_id.clone(),
            revision: 1,
            payload,
            state: PermissionState::Pending,
        };
        self.permission_leases.insert(id, authority);
        record_change(&mut self.state, record.clone());
        self.publish();
        Ok(record)
    }

    pub(super) fn resolve_execution_permission(
        &mut self,
        resolution: PermissionResolution,
    ) -> Result<PermissionRecord, ThreadError> {
        if self.state.lifecycle != ThreadLifecycle::Open || self.interrupt.is_closing() {
            return Err(ThreadError::Closed);
        }
        let previous = self
            .state
            .permissions
            .get(&resolution.id)
            .ok_or(ThreadError::InvalidIdentity)?;
        let state = match resolution.decision {
            PermissionDecision::Allow => PermissionState::Allowed,
            PermissionDecision::Deny => PermissionState::Denied,
        };
        if previous.state == state
            && previous.response == resolution.payload
            && resolution.expected_revision.checked_add(1) == Some(previous.revision)
        {
            return Ok(previous.clone());
        }
        let authority = self
            .permission_leases
            .get(&resolution.id)
            .ok_or(ThreadError::ToolPermissionRevoked)?;
        if !authority.remains_authorized(&self.tools) {
            return Err(ThreadError::ToolPermissionRevoked);
        }
        let task = self
            .state
            .tasks
            .get(&previous.task_id)
            .ok_or(ThreadError::InvalidIdentity)?;
        if task.status != task::TaskStatus::Running
            || task.cancel_requested
            || self
                .task_tokens
                .get(&previous.task_id)
                .is_none_or(CancellationToken::is_cancelled)
        {
            return Err(ThreadError::TaskAccessExpired);
        }
        if previous.state != PermissionState::Pending
            || previous.revision != resolution.expected_revision
        {
            return Err(ThreadError::InvalidIdentity);
        }
        let mut record = previous.clone();
        record.revision = record
            .revision
            .checked_add(1)
            .ok_or(ThreadError::RevisionExhausted)?;
        record.state = state;
        record.response = resolution.payload;
        record.updated_at = crate::time::unix_seconds();
        record_change(&mut self.state, record.clone());
        self.publish();
        Ok(record)
    }

    pub(super) fn revoke_stale_permissions(&mut self) -> Result<(), ThreadError> {
        let stale = self
            .permission_leases
            .iter()
            .filter(|(_, authority)| !authority.remains_authorized(&self.tools))
            .map(|(id, _)| id.clone())
            .collect::<Vec<_>>();
        for id in stale {
            cancel_pending(&mut self.state, &id)?;
            self.permission_leases.remove(&id);
        }
        Ok(())
    }

    pub(super) fn settle_execution_permission(&mut self, call_id: &str) -> Result<(), ThreadError> {
        let id = format!("permission:{call_id}");
        cancel_pending(&mut self.state, &id)?;
        Ok(())
    }
}

pub(super) fn cancel_pending(state: &mut ThreadSnapshot, id: &str) -> Result<(), ThreadError> {
    if let Some(previous) = state
        .permissions
        .get(id)
        .filter(|record| record.state == PermissionState::Pending)
    {
        let mut record = previous.clone();
        record.revision = record
            .revision
            .checked_add(1)
            .ok_or(ThreadError::RevisionExhausted)?;
        record.state = PermissionState::Cancelled;
        record.updated_at = crate::time::unix_seconds();
        record_change(state, record);
    }
    Ok(())
}

fn record_change(state: &mut ThreadSnapshot, record: PermissionRecord) {
    state.permissions.insert(record.id.clone(), record.clone());
    let mut changes = state.permission_changes.to_vec();
    changes.push(record);
    state.permission_changes = changes.into();
}

pub(super) fn replay(
    state: &mut ThreadSnapshot,
    record: &PermissionRecord,
) -> Result<(), ThreadError> {
    let task = state
        .tasks
        .get(&record.task_id)
        .ok_or(ThreadError::InvalidOutput)?;
    if record.id != format!("permission:{}", record.call_id)
        || task.call_id != record.call_id
        || task.tool_id != record.tool_id
        || task.turn_id != record.turn_id
    {
        return Err(ThreadError::InvalidOutput);
    }
    if matches!(
        record.state,
        PermissionState::Pending | PermissionState::Cancelled
    ) && record.response.is_some()
    {
        return Err(ThreadError::InvalidOutput);
    }
    match state.permissions.get(&record.id) {
        None if record.revision == 1
            && record.state == PermissionState::Pending
            && task.status == task::TaskStatus::Running => {}
        Some(previous)
            if previous.state == PermissionState::Pending
                && record.state != PermissionState::Pending
                && previous.revision.checked_add(1) == Some(record.revision)
                && previous.created_at == record.created_at
                && previous.payload == record.payload
                && previous.task_id == record.task_id
                && previous.call_id == record.call_id
                && previous.turn_id == record.turn_id
                && previous.tool_id == record.tool_id => {}
        None | Some(_) => return Err(ThreadError::InvalidOutput),
    }
    if matches!(
        record.state,
        PermissionState::Allowed | PermissionState::Denied
    ) && (task.status != task::TaskStatus::Running || task.cancel_requested)
    {
        return Err(ThreadError::InvalidOutput);
    }
    record_change(state, record.clone());
    Ok(())
}
