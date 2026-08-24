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
    ReadMcp,
    ResetMcp,
    ReadLsp,
    ProbeLsp,
    RepairLsp,
    ResetLsp,
    ReadUpdate,
    CheckUpdate,
    PreviewTaskRecovery,
    ApplyTaskRecovery,
    PreviewRecoveryCleanup,
    CleanupRecoveryIssue,
    RetryRecoveryIssue,
    RetryPersistence,
    PreviewProjectCleanup,
    CleanupProject,
    SubscribeProduct,
    SubscribeThread,
}

impl StudioOperation {
    pub const ALL: [Self; 47] = [
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
        Self::ReadMcp,
        Self::ResetMcp,
        Self::ReadLsp,
        Self::ProbeLsp,
        Self::RepairLsp,
        Self::ResetLsp,
        Self::ReadUpdate,
        Self::CheckUpdate,
        Self::PreviewTaskRecovery,
        Self::ApplyTaskRecovery,
        Self::PreviewRecoveryCleanup,
        Self::CleanupRecoveryIssue,
        Self::RetryRecoveryIssue,
        Self::RetryPersistence,
        Self::PreviewProjectCleanup,
        Self::CleanupProject,
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
            Self::ReadMcp => "mcp.read",
            Self::ResetMcp => "mcp.reset",
            Self::ReadLsp => "lsp.read",
            Self::ProbeLsp => "lsp.probe",
            Self::RepairLsp => "lsp.repair",
            Self::ResetLsp => "lsp.reset",
            Self::ReadUpdate => "update.read",
            Self::CheckUpdate => "update.check",
            Self::PreviewTaskRecovery => "recovery.taskPreview",
            Self::ApplyTaskRecovery => "recovery.taskApply",
            Self::PreviewRecoveryCleanup => "recovery.issueCleanupPreview",
            Self::CleanupRecoveryIssue => "recovery.issueCleanup",
            Self::RetryRecoveryIssue => "recovery.issueRetry",
            Self::RetryPersistence => "persistence.retry",
            Self::PreviewProjectCleanup => "recovery.projectCleanupPreview",
            Self::CleanupProject => "recovery.projectCleanup",
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
