//! RAII ownership of one exact registration generation.

use std::fmt;

use super::scope::{ToolGroupId, ToolScope};

/// RAII ownership of one exact registration generation.
///
/// The guard owns exactly the generation it published. Dropping an obsolete
/// guard cannot unregister a newer replacement of the same group.
pub struct ToolRegistration {
    pub(super) entries: Vec<ToolRegistrationEntry>,
}

pub(super) struct ToolRegistrationEntry {
    pub(super) scope: ToolScope,
    pub(super) group: ToolGroupId,
    pub(super) generation: u64,
}

impl fmt::Debug for ToolRegistration {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ToolRegistration")
            .field(
                "groups",
                &self
                    .entries
                    .iter()
                    .map(|entry| (&entry.group, entry.generation))
                    .collect::<Vec<_>>(),
            )
            .finish_non_exhaustive()
    }
}

impl ToolRegistration {
    pub(super) fn group(&self) -> &ToolGroupId {
        debug_assert_eq!(self.entries.len(), 1);
        &self.entries[0].group
    }

    pub(super) fn into_single_group_registrations(mut self) -> Vec<Self> {
        std::mem::take(&mut self.entries)
            .into_iter()
            .map(|entry| Self {
                entries: vec![entry],
            })
            .collect()
    }
}

impl Drop for ToolRegistration {
    fn drop(&mut self) {
        let entries = std::mem::take(&mut self.entries);
        for entry in entries {
            entry
                .scope
                .remove_generation(&entry.group, entry.generation);
        }
    }
}
