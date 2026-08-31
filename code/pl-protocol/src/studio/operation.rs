//! Studio 命令/查询/流操作的稳定枚举。

use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

/// Canonical shared command/query/stream operations.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash, ToSchema)]
#[serde(rename_all = "camelCase")]
pub enum StudioOperation {
    ReadState,
    OpenProject,
    ActivateProject,
    ArchiveProject,
    ListThreadsPage,
    StartNewThread,
    ReadThread,
    ArchiveThread,
    SetThreadMode,
    ListThreadTurns,
    StartTurn,
    SteerTurn,
    InterruptTurn,
    AdmitAttachmentDrafts,
    UploadAttachmentDrafts,
    RemoveAttachmentDraft,
    ReadAttachmentDraft,
    ReadThreadAttachment,
    RespondInteraction,
    LoadProviderCatalog,
    ReadSettings,
    ReloadSettings,
    SaveWebSearchSettings,
    SavePermissionSettings,
    SaveProviderSettings,
    SaveInstructionsSettings,
    SaveSkillsSettings,
    SaveMcpSettings,
    SaveGeneralSettings,
    SetModelRole,
    ReadProviderUsage,
    CheckProviderUsage,
    ReadSkills,
    DiscoverSkills,
    SearchSkills,
    ReadMcp,
    ResetMcp,
    ReadLsp,
    ProbeLsp,
    RepairLsp,
    ResetLsp,
    ReadUpdate,
    CheckUpdate,
    RetryPersistence,
    SubscribeProduct,
    SubscribeThread,
}

impl StudioOperation {
    pub const ALL: [Self; 46] = [
        Self::ReadState,
        Self::OpenProject,
        Self::ActivateProject,
        Self::ArchiveProject,
        Self::ListThreadsPage,
        Self::StartNewThread,
        Self::ReadThread,
        Self::ArchiveThread,
        Self::SetThreadMode,
        Self::ListThreadTurns,
        Self::StartTurn,
        Self::SteerTurn,
        Self::InterruptTurn,
        Self::AdmitAttachmentDrafts,
        Self::UploadAttachmentDrafts,
        Self::RemoveAttachmentDraft,
        Self::ReadAttachmentDraft,
        Self::ReadThreadAttachment,
        Self::RespondInteraction,
        Self::LoadProviderCatalog,
        Self::ReadSettings,
        Self::ReloadSettings,
        Self::SaveWebSearchSettings,
        Self::SavePermissionSettings,
        Self::SaveProviderSettings,
        Self::SaveInstructionsSettings,
        Self::SaveSkillsSettings,
        Self::SaveMcpSettings,
        Self::SaveGeneralSettings,
        Self::SetModelRole,
        Self::ReadProviderUsage,
        Self::CheckProviderUsage,
        Self::ReadSkills,
        Self::DiscoverSkills,
        Self::SearchSkills,
        Self::ReadMcp,
        Self::ResetMcp,
        Self::ReadLsp,
        Self::ProbeLsp,
        Self::RepairLsp,
        Self::ResetLsp,
        Self::ReadUpdate,
        Self::CheckUpdate,
        Self::RetryPersistence,
        Self::SubscribeProduct,
        Self::SubscribeThread,
    ];

    pub const fn operation_id(self) -> &'static str {
        match self {
            Self::ReadState => "studio.readState",
            Self::OpenProject => "project.open",
            Self::ActivateProject => "project.activate",
            Self::ArchiveProject => "project.archive",
            Self::ListThreadsPage => "thread.listPage",
            Self::StartNewThread => "thread.create",
            Self::ReadThread => "thread.read",
            Self::ArchiveThread => "thread.archive",
            Self::SetThreadMode => "thread.setMode",
            Self::ListThreadTurns => "thread.listTurns",
            Self::StartTurn => "turn.start",
            Self::SteerTurn => "turn.steer",
            Self::InterruptTurn => "turn.interrupt",
            Self::AdmitAttachmentDrafts => "attachment.admit",
            Self::UploadAttachmentDrafts => "attachment.upload",
            Self::RemoveAttachmentDraft => "attachment.removeDraft",
            Self::ReadAttachmentDraft => "attachment.readDraft",
            Self::ReadThreadAttachment => "attachment.readThread",
            Self::RespondInteraction => "interaction.respond",
            Self::LoadProviderCatalog => "settings.loadProviderCatalog",
            Self::ReadSettings => "settings.read",
            Self::ReloadSettings => "settings.reload",
            Self::SaveWebSearchSettings => "settings.saveWebSearch",
            Self::SavePermissionSettings => "settings.savePermission",
            Self::SaveProviderSettings => "settings.saveProviders",
            Self::SaveInstructionsSettings => "settings.saveInstructions",
            Self::SaveSkillsSettings => "settings.saveSkills",
            Self::SaveMcpSettings => "settings.saveMcp",
            Self::SaveGeneralSettings => "settings.saveGeneral",
            Self::SetModelRole => "settings.setModelRole",
            Self::ReadProviderUsage => "providerUsage.read",
            Self::CheckProviderUsage => "providerUsage.check",
            Self::ReadSkills => "skills.read",
            Self::DiscoverSkills => "skills.discover",
            Self::SearchSkills => "skills.search",
            Self::ReadMcp => "mcp.read",
            Self::ResetMcp => "mcp.reset",
            Self::ReadLsp => "lsp.read",
            Self::ProbeLsp => "lsp.probe",
            Self::RepairLsp => "lsp.repair",
            Self::ResetLsp => "lsp.reset",
            Self::ReadUpdate => "update.read",
            Self::CheckUpdate => "update.check",
            Self::RetryPersistence => "persistence.retry",
            Self::SubscribeProduct => "studio.subscribeProduct",
            Self::SubscribeThread => "thread.subscribe",
        }
    }
}

/// Desktop-host-only operations intentionally omitted from the HTTP API.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash, ToSchema)]
#[serde(rename_all = "camelCase")]
pub enum StudioHostOperation {
    InitializeFrb,
    StartRuntime,
    ShutdownRuntime,
    SubscribeShutdownProgress,
    InstallUpdate,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn operation_ids_are_unique() {
        let ids = StudioOperation::ALL
            .into_iter()
            .map(StudioOperation::operation_id)
            .collect::<std::collections::HashSet<_>>();
        assert_eq!(ids.len(), StudioOperation::ALL.len());
    }
}
