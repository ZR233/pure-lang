//! Explicit context replacement and retained history versions.
use super::*;

impl Owner {
    pub(super) fn replace_context(
        &mut self,
        replacement: ReplaceContext,
    ) -> Result<ContextSnapshot, ThreadError> {
        if !self.pending.is_empty() {
            return Err(ThreadError::PendingTools);
        }
        let current = stage_replacement(&mut self.state, replacement)?;
        self.publish();
        Ok(current)
    }
}

pub(super) fn stage_replacement(
    state: &mut ThreadSnapshot,
    mut replacement: ReplaceContext,
) -> Result<ContextSnapshot, ThreadError> {
    if state.lifecycle != ThreadLifecycle::Open {
        return Err(ThreadError::Closed);
    }
    let actual = state.context.revision;
    if actual != replacement.expected_revision {
        return Err(ThreadError::ContextConflict {
            expected: replacement.expected_revision,
            actual,
        });
    }
    let mut identities = std::collections::BTreeSet::new();
    if replacement
        .records
        .iter()
        .any(|record| record.id.is_empty() || !identities.insert(record.id.as_str()))
    {
        return Err(ThreadError::InvalidContext);
    }
    let revision = actual
        .checked_add(1)
        .ok_or(ThreadError::RevisionExhausted)?;
    super::facts::restore_facts(&mut replacement.records, &state.runtime_facts, revision);
    let current = ContextSnapshot {
        revision,
        records: replacement.records.into(),
    };
    current.validate_complete()?;
    let mut replacements = state.context_replacements.to_vec();
    replacements.push(ContextReplacement {
        reason: replacement.reason,
        previous: state.context.clone(),
        current: current.clone(),
        previous_private_context: state.private_context.clone(),
    });
    state.context = current.clone();
    state.private_context = None;
    state.context_replacements = replacements.into();
    Ok(current)
}
