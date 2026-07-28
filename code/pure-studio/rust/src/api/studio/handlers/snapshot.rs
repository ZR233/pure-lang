use crate::api::studio::convert::records::{project_dto, session_dto};
use crate::api::studio::convert::runtime::{bridge_recovery_issue, bridge_task_runtime};
use crate::api::studio::convert::settings::{studio_config_projection, web_search_settings_dto};
use crate::api::studio::runtime::BridgeRuntime;
use crate::api::studio::types::BridgeStudioSnapshotResponse;
use anyhow::Result;
// ── Inner async helpers ──

pub(super) async fn bootstrap_studio_inner(
    bridge: &'static BridgeRuntime,
) -> Result<BridgeStudioSnapshotResponse> {
    let mut projects = bridge.studio.list_projects().await?;
    if projects.is_empty()
        && !bridge.studio.store().has_projects().await?
        && let Ok(cwd) = std::env::current_dir()
    {
        projects.push(bridge.studio.open_project(cwd).await?);
    }
    let selected_project_id = projects.first().map(|project| project.id.clone());
    studio_snapshot_from_projects_inner(bridge, projects, selected_project_id, None).await
}

pub(super) async fn studio_snapshot_inner(
    bridge: &'static BridgeRuntime,
    requested_project_id: Option<String>,
    requested_session_id: Option<String>,
) -> Result<BridgeStudioSnapshotResponse> {
    let projects = bridge.studio.list_projects().await?;
    studio_snapshot_from_projects_inner(
        bridge,
        projects,
        requested_project_id,
        requested_session_id,
    )
    .await
}

pub(super) async fn studio_snapshot_from_projects_inner(
    bridge: &'static BridgeRuntime,
    projects: Vec<pl_studio_runtime::ProjectRecord>,
    requested_project_id: Option<String>,
    requested_session_id: Option<String>,
) -> Result<BridgeStudioSnapshotResponse> {
    let recovery_issues = bridge.studio.runtime_snapshot().recovery_issues;
    let blocked_project_ids = recovery_issues
        .iter()
        .filter(|issue| issue.scope == pl_studio_runtime::StudioRecoveryIssueScope::Project)
        .filter_map(|issue| issue.project_id.as_deref())
        .collect::<std::collections::HashSet<_>>();
    let blocked_session_ids = recovery_issues
        .iter()
        .filter(|issue| issue.scope == pl_studio_runtime::StudioRecoveryIssueScope::Session)
        .filter_map(|issue| issue.session_id.as_deref())
        .collect::<std::collections::HashSet<_>>();
    let selected_project = requested_project_id
        .as_ref()
        .and_then(|project_id| {
            projects
                .iter()
                .find(|project| {
                    project.id == *project_id && !blocked_project_ids.contains(project.id.as_str())
                })
                .cloned()
        })
        .or_else(|| {
            projects
                .iter()
                .find(|project| !blocked_project_ids.contains(project.id.as_str()))
                .cloned()
        });
    let selected_project_id = selected_project.as_ref().map(|project| project.id.clone());
    let mut sessions = Vec::new();
    let mut selected_session_id = None;

    if let Some(project) = selected_project {
        bridge
            .studio
            .reconcile_lsp_runtime_for_project(&project.id)
            .await?;
        let roots = bridge.studio.ensure_project_sessions(&project.id).await?;
        sessions = bridge.studio.store().list_all_sessions(&project.id).await?;
        selected_session_id = requested_session_id
            .filter(|session_id| {
                !blocked_session_ids.contains(session_id.as_str())
                    && sessions.iter().any(|session| session.id == *session_id)
            })
            .or_else(|| {
                roots
                    .iter()
                    .find(|session| !blocked_session_ids.contains(session.id.as_str()))
                    .map(|session| session.id.clone())
            });
    }
    let selected_session_task = match selected_session_id.as_deref() {
        Some(session_id) => bridge
            .studio
            .session_task_view(session_id)
            .await?
            .map(bridge_task_runtime),
        None => None,
    };
    let config = bridge.studio.config_store().load_or_default()?;
    let web_search_role = selected_session_id
        .as_deref()
        .and_then(|session_id| sessions.iter().find(|session| session.id == session_id))
        .map(|session| pl_studio_runtime::StudioMode::from_label(&session.mode))
        .map_or(pl_studio_runtime::StudioRole::Executor, |mode| match mode {
            pl_studio_runtime::StudioMode::Simple => pl_studio_runtime::StudioRole::Executor,
            pl_studio_runtime::StudioMode::Task => pl_studio_runtime::StudioRole::Planner,
        });
    let web_search = web_search_settings_dto(&config, web_search_role)?;
    let config_json = serde_json::to_string(&studio_config_projection(&config)?)?;
    let general_settings = bridge
        .studio
        .store()
        .load_setting("flutterSettings:general")
        .await?
        .and_then(|value| serde_json::from_str::<serde_json::Value>(&value).ok())
        .unwrap_or_else(|| serde_json::json!({}));
    let general_settings_json = serde_json::to_string(&general_settings)?;

    Ok(BridgeStudioSnapshotResponse {
        projects: projects.into_iter().map(project_dto).collect(),
        selected_project_id,
        sessions: sessions.into_iter().map(session_dto).collect(),
        selected_session_id,
        selected_session_task,
        recovery_issues: recovery_issues
            .into_iter()
            .map(bridge_recovery_issue)
            .collect(),
        config_json,
        general_settings_json,
        web_search,
    })
}

pub(super) fn ensure_project_recovery_available(
    bridge: &'static BridgeRuntime,
    project_id: &str,
) -> Result<()> {
    if bridge
        .studio
        .runtime_snapshot()
        .recovery_issues
        .iter()
        .any(|issue| {
            issue.scope == pl_studio_runtime::StudioRecoveryIssueScope::Project
                && issue.project_id.as_deref() == Some(project_id)
        })
    {
        anyhow::bail!("selected project is blocked by a recovery issue");
    }
    Ok(())
}
