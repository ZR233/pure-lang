use axum::Json;
use axum::extract::rejection::JsonRejection;
use axum::extract::rejection::QueryRejection;
use axum::extract::{FromRequest, FromRequestParts, Path, Query, Request, State};
use axum::http::StatusCode;
use axum::http::request::Parts;
use axum::response::IntoResponse;
use pl_protocol::studio::{
    CreateThreadRequest, ExpectedOpaqueRevisionRequest, ExpectedRevisionRequest, HealthResponse,
    LspResetRequest, McpResetRequest, OpenProjectRequest, ResolveInteractionRequest,
    SetModelRoleRequest, SetThreadModeRequest, StartTurnRequest, SteerTurnRequest, StudioError,
    StudioSettingsSnapshot, ThreadPageQuery, UpdateGeneralSettingsRequest,
    UpdateInstructionsSettingsRequest, UpdateMcpSettingsRequest, UpdatePermissionSettingsRequest,
    UpdateProviderSettingsRequest, UpdateSkillsSettingsRequest, UpdateWebSearchSettingsRequest,
};
use pl_studio_runtime::{StudioMode, StudioTaskRecoveryRequest};
use utoipa::{IntoResponses, OpenApi};
use utoipa_axum::{router::OpenApiRouter, routes};

use crate::AppState;
use crate::error::ApiError;
use crate::sse;

#[derive(OpenApi)]
#[openapi(
    info(
        title = "Pure Studio Runtime API",
        version = "0.1.0",
        description = "Loopback adapter over the canonical StudioRuntime"
    ),
    components(schemas(StudioError))
)]
struct StudioApi;

#[allow(dead_code)]
#[derive(IntoResponses)]
pub(crate) enum StudioApiErrors {
    #[response(status = 400, description = "Invalid request or protocol data")]
    BadRequest(StudioError),
    #[response(status = 403, description = "Permission denied")]
    Forbidden(StudioError),
    #[response(status = 404, description = "Resource not found")]
    NotFound(StudioError),
    #[response(status = 409, description = "Busy, conflict, stale, or cancelled")]
    Conflict(StudioError),
    #[response(status = 429, description = "Concurrency limit exceeded")]
    Overloaded(StudioError),
    #[response(
        status = 503,
        description = "Runtime, storage, update, or dependency unavailable"
    )]
    Unavailable(StudioError),
    #[response(status = 500, description = "Internal failure")]
    Internal(StudioError),
}

struct ApiJson<T>(T);

impl<S, T> FromRequest<S> for ApiJson<T>
where
    S: Send + Sync,
    T: serde::de::DeserializeOwned,
{
    type Rejection = ApiError;

    async fn from_request(request: Request, state: &S) -> Result<Self, Self::Rejection> {
        Json::<T>::from_request(request, state)
            .await
            .map(|Json(value)| Self(value))
            .map_err(json_rejection)
    }
}

fn json_rejection(rejection: JsonRejection) -> ApiError {
    tracing::debug!(error = %rejection, "rejected Studio HTTP JSON body");
    ApiError(StudioError::invalid_argument("invalid JSON request body"))
}

struct ApiQuery<T>(T);

impl<S, T> FromRequestParts<S> for ApiQuery<T>
where
    S: Send + Sync,
    T: serde::de::DeserializeOwned,
{
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        Query::<T>::from_request_parts(parts, state)
            .await
            .map(|Query(value)| Self(value))
            .map_err(query_rejection)
    }
}

fn query_rejection(rejection: QueryRejection) -> ApiError {
    tracing::debug!(error = %rejection, "rejected Studio HTTP query");
    ApiError(StudioError::invalid_argument("invalid query parameters"))
}

pub(crate) fn api_router() -> OpenApiRouter<AppState> {
    OpenApiRouter::with_openapi(StudioApi::openapi())
        .routes(routes!(health))
        .routes(routes!(read_state))
        .routes(routes!(open_project))
        .routes(routes!(activate_project))
        .routes(routes!(archive_project))
        .routes(routes!(list_threads))
        .routes(routes!(create_thread))
        .routes(routes!(read_thread))
        .routes(routes!(archive_thread))
        .routes(routes!(set_thread_mode))
        .routes(routes!(list_thread_turns))
        .routes(routes!(start_turn))
        .routes(routes!(steer_turn))
        .routes(routes!(interrupt_turn))
        .routes(routes!(resolve_interaction))
        .routes(routes!(provider_catalog))
        .routes(routes!(read_settings))
        .routes(routes!(reload_settings))
        .routes(routes!(save_web_search_settings))
        .routes(routes!(save_permission_settings))
        .routes(routes!(save_provider_settings))
        .routes(routes!(save_instructions_settings))
        .routes(routes!(save_skills_settings))
        .routes(routes!(save_mcp_settings))
        .routes(routes!(save_general_settings))
        .routes(routes!(save_model_role))
        .routes(routes!(read_provider_usage))
        .routes(routes!(check_provider_usage))
        .routes(routes!(read_skills))
        .routes(routes!(discover_skills))
        .routes(routes!(read_mcp))
        .routes(routes!(reset_mcp))
        .routes(routes!(read_lsp))
        .routes(routes!(probe_lsp))
        .routes(routes!(repair_lsp))
        .routes(routes!(reset_lsp))
        .routes(routes!(read_update))
        .routes(routes!(check_update))
        .routes(routes!(preview_task_recovery))
        .routes(routes!(apply_task_recovery))
        .routes(routes!(preview_issue_cleanup))
        .routes(routes!(cleanup_issue))
        .routes(routes!(retry_issue))
        .routes(routes!(preview_project_cleanup))
        .routes(routes!(cleanup_project))
        .routes(routes!(sse::product_events))
        .routes(routes!(sse::thread_events))
}

#[utoipa::path(get, path = "/health", operation_id = "health", responses(StudioApiErrors, (status = 200, body = HealthResponse)))]
async fn health(State(_state): State<AppState>) -> Result<Json<HealthResponse>, ApiError> {
    Ok(Json(HealthResponse {
        status: "ok".to_string(),
    }))
}

#[utoipa::path(get, path = "/api/v1/state", operation_id = "studio.readState", responses(StudioApiErrors, (status = 200, description = "Canonical Studio state")))]
async fn read_state(State(state): State<AppState>) -> Result<impl IntoResponse, ApiError> {
    Ok(Json(
        state.runtime.read_state().await.map_err(ApiError::from)?,
    ))
}

#[utoipa::path(post, path = "/api/v1/projects", operation_id = "project.open", request_body = OpenProjectRequest, responses(StudioApiErrors, (status = 200, description = "Opened Project")))]
async fn open_project(
    State(state): State<AppState>,
    ApiJson(request): ApiJson<OpenProjectRequest>,
) -> Result<impl IntoResponse, ApiError> {
    Ok(Json(
        state
            .runtime
            .open_project(request.path)
            .await
            .map_err(ApiError::from)?,
    ))
}

#[utoipa::path(post, path = "/api/v1/projects/{project_id}/activation", operation_id = "project.activate", params(("project_id" = String, Path)), responses(StudioApiErrors, (status = 204)))]
async fn activate_project(
    State(state): State<AppState>,
    Path(project_id): Path<String>,
) -> Result<StatusCode, ApiError> {
    state
        .runtime
        .activate_project(&project_id)
        .await
        .map_err(ApiError::from)?;
    Ok(StatusCode::NO_CONTENT)
}

#[utoipa::path(delete, path = "/api/v1/projects/{project_id}", operation_id = "project.archive", params(("project_id" = String, Path)), responses(StudioApiErrors, (status = 200)))]
async fn archive_project(
    State(state): State<AppState>,
    Path(project_id): Path<String>,
) -> Result<impl IntoResponse, ApiError> {
    let project = state
        .runtime
        .archive_project(&project_id)
        .await
        .map_err(ApiError::from)?
        .ok_or_else(|| ApiError(StudioError::not_found("Project")))?;
    Ok(Json(project))
}

#[utoipa::path(get, path = "/api/v1/threads", operation_id = "thread.listPage", params(("cursor" = Option<String>, Query), ("limit" = Option<u32>, Query)), responses(StudioApiErrors, (status = 200)))]
async fn list_threads(
    State(state): State<AppState>,
    ApiQuery(query): ApiQuery<ThreadPageQuery>,
) -> Result<impl IntoResponse, ApiError> {
    Ok(Json(
        state
            .runtime
            .list_threads_page(query.cursor.as_deref(), query.limit())
            .await
            .map_err(ApiError::from)?,
    ))
}

#[utoipa::path(post, path = "/api/v1/projects/{project_id}/threads", operation_id = "thread.create", params(("project_id" = String, Path)), request_body = CreateThreadRequest, responses(StudioApiErrors, (status = 200)))]
async fn create_thread(
    State(state): State<AppState>,
    Path(project_id): Path<String>,
    ApiJson(request): ApiJson<CreateThreadRequest>,
) -> Result<impl IntoResponse, ApiError> {
    Ok(Json(
        state
            .runtime
            .create_thread_command(project_id, request)
            .await
            .map_err(ApiError::from)?,
    ))
}

#[utoipa::path(get, path = "/api/v1/threads/{thread_id}", operation_id = "thread.read", params(("thread_id" = String, Path)), responses(StudioApiErrors, (status = 200)))]
async fn read_thread(
    State(state): State<AppState>,
    Path(thread_id): Path<String>,
) -> Result<impl IntoResponse, ApiError> {
    Ok(Json(
        state
            .runtime
            .read_thread(&thread_id)
            .await
            .map_err(ApiError::from)?,
    ))
}

#[utoipa::path(delete, path = "/api/v1/threads/{thread_id}", operation_id = "thread.archive", params(("thread_id" = String, Path)), responses(StudioApiErrors, (status = 200)))]
async fn archive_thread(
    State(state): State<AppState>,
    Path(thread_id): Path<String>,
) -> Result<impl IntoResponse, ApiError> {
    let result = state
        .runtime
        .archive_thread(thread_id)
        .await
        .map_err(ApiError::from)?
        .ok_or_else(|| ApiError(StudioError::not_found("Thread")))?;
    Ok(Json(result))
}

#[utoipa::path(put, path = "/api/v1/threads/{thread_id}/mode", operation_id = "thread.setMode", params(("thread_id" = String, Path)), request_body = SetThreadModeRequest, responses(StudioApiErrors, (status = 204)))]
async fn set_thread_mode(
    State(state): State<AppState>,
    Path(thread_id): Path<String>,
    ApiJson(request): ApiJson<SetThreadModeRequest>,
) -> Result<StatusCode, ApiError> {
    state
        .runtime
        .set_thread_mode(&thread_id, parse_mode(&request.mode)?)
        .await
        .map_err(ApiError::from)?;
    Ok(StatusCode::NO_CONTENT)
}

#[utoipa::path(get, path = "/api/v1/threads/{thread_id}/turns", operation_id = "thread.listTurns", params(("thread_id" = String, Path), ("cursor" = Option<String>, Query), ("limit" = Option<u32>, Query)), responses(StudioApiErrors, (status = 200)))]
async fn list_thread_turns(
    State(state): State<AppState>,
    Path(thread_id): Path<String>,
    ApiQuery(query): ApiQuery<ThreadPageQuery>,
) -> Result<impl IntoResponse, ApiError> {
    Ok(Json(
        state
            .runtime
            .list_thread_turns(&thread_id, query.cursor.as_deref(), query.limit())
            .await
            .map_err(ApiError::from)?,
    ))
}

#[utoipa::path(post, path = "/api/v1/threads/{thread_id}/turns", operation_id = "turn.start", params(("thread_id" = String, Path)), request_body = StartTurnRequest, responses(StudioApiErrors, (status = 200)))]
async fn start_turn(
    State(state): State<AppState>,
    Path(thread_id): Path<String>,
    ApiJson(request): ApiJson<StartTurnRequest>,
) -> Result<impl IntoResponse, ApiError> {
    Ok(Json(
        state
            .runtime
            .start_turn(thread_id, request)
            .await
            .map_err(ApiError::from)?,
    ))
}

#[utoipa::path(post, path = "/api/v1/threads/{thread_id}/active-turn/steer", operation_id = "turn.steer", params(("thread_id" = String, Path)), request_body = SteerTurnRequest, responses(StudioApiErrors, (status = 200)))]
async fn steer_turn(
    State(state): State<AppState>,
    Path(thread_id): Path<String>,
    ApiJson(request): ApiJson<SteerTurnRequest>,
) -> Result<impl IntoResponse, ApiError> {
    Ok(Json(
        state
            .runtime
            .steer_turn(thread_id, request)
            .await
            .map_err(ApiError::from)?,
    ))
}

#[utoipa::path(delete, path = "/api/v1/threads/{thread_id}/turns/{turn_id}", operation_id = "turn.interrupt", params(("thread_id" = String, Path), ("turn_id" = String, Path)), responses(StudioApiErrors, (status = 200)))]
async fn interrupt_turn(
    State(state): State<AppState>,
    Path((thread_id, turn_id)): Path<(String, String)>,
) -> Result<impl IntoResponse, ApiError> {
    Ok(Json(
        state
            .runtime
            .interrupt_prompt(thread_id, turn_id)
            .await
            .map_err(ApiError::from)?,
    ))
}

#[utoipa::path(put, path = "/api/v1/interactions/{interaction_id}/resolution", operation_id = "interaction.respond", params(("interaction_id" = String, Path)), request_body = ResolveInteractionRequest, responses(StudioApiErrors, (status = 200)))]
async fn resolve_interaction(
    State(state): State<AppState>,
    Path(interaction_id): Path<String>,
    ApiJson(request): ApiJson<ResolveInteractionRequest>,
) -> Result<impl IntoResponse, ApiError> {
    Ok(Json(
        state
            .runtime
            .resolve_interaction(interaction_id, request.resolution)
            .await
            .map_err(ApiError::from)?,
    ))
}

#[utoipa::path(get, path = "/api/v1/settings/provider-catalog", operation_id = "settings.loadProviderCatalog", responses(StudioApiErrors, (status = 200)))]
async fn provider_catalog(State(state): State<AppState>) -> Result<impl IntoResponse, ApiError> {
    let snapshot = state
        .runtime
        .load_provider_catalog()
        .map_err(ApiError::from)?;
    Ok(Json(snapshot))
}

#[utoipa::path(get, path = "/api/v1/settings", operation_id = "settings.read", responses(StudioApiErrors, (status = 200, body = StudioSettingsSnapshot)))]
async fn read_settings(
    State(state): State<AppState>,
) -> Result<Json<StudioSettingsSnapshot>, ApiError> {
    Ok(Json(state.runtime.read_settings().map_err(ApiError::from)?))
}

#[utoipa::path(post, path = "/api/v1/settings/reload", operation_id = "settings.reload", request_body = ExpectedRevisionRequest, responses(StudioApiErrors, (status = 200, body = StudioSettingsSnapshot)))]
async fn reload_settings(
    State(state): State<AppState>,
    ApiJson(request): ApiJson<ExpectedRevisionRequest>,
) -> Result<Json<StudioSettingsSnapshot>, ApiError> {
    Ok(Json(
        state
            .runtime
            .reload_settings(request.expected_revision)
            .await
            .map_err(ApiError::from)?,
    ))
}

#[utoipa::path(put, path = "/api/v1/settings/web-search", operation_id = "settings.saveWebSearch", request_body = UpdateWebSearchSettingsRequest, responses(StudioApiErrors, (status = 200, body = StudioSettingsSnapshot)))]
async fn save_web_search_settings(
    State(state): State<AppState>,
    ApiJson(request): ApiJson<UpdateWebSearchSettingsRequest>,
) -> Result<Json<StudioSettingsSnapshot>, ApiError> {
    Ok(Json(
        state
            .runtime
            .save_web_search_settings(request)
            .map_err(ApiError::from)?,
    ))
}

#[utoipa::path(put, path = "/api/v1/settings/permission", operation_id = "settings.savePermission", request_body = UpdatePermissionSettingsRequest, responses(StudioApiErrors, (status = 200, body = StudioSettingsSnapshot)))]
async fn save_permission_settings(
    State(state): State<AppState>,
    ApiJson(request): ApiJson<UpdatePermissionSettingsRequest>,
) -> Result<Json<StudioSettingsSnapshot>, ApiError> {
    Ok(Json(
        state
            .runtime
            .save_permission_settings(request)
            .map_err(ApiError::from)?,
    ))
}

#[utoipa::path(put, path = "/api/v1/settings/providers", operation_id = "settings.saveProviders", request_body = UpdateProviderSettingsRequest, responses(StudioApiErrors, (status = 200, body = StudioSettingsSnapshot)))]
async fn save_provider_settings(
    State(state): State<AppState>,
    ApiJson(request): ApiJson<UpdateProviderSettingsRequest>,
) -> Result<Json<StudioSettingsSnapshot>, ApiError> {
    Ok(Json(
        state
            .runtime
            .save_provider_settings(request)
            .await
            .map_err(ApiError::from)?,
    ))
}

#[utoipa::path(put, path = "/api/v1/settings/instructions", operation_id = "settings.saveInstructions", request_body = UpdateInstructionsSettingsRequest, responses(StudioApiErrors, (status = 200, body = StudioSettingsSnapshot)))]
async fn save_instructions_settings(
    State(state): State<AppState>,
    ApiJson(request): ApiJson<UpdateInstructionsSettingsRequest>,
) -> Result<Json<StudioSettingsSnapshot>, ApiError> {
    Ok(Json(
        state
            .runtime
            .save_instructions_settings(request)
            .map_err(ApiError::from)?,
    ))
}

#[utoipa::path(put, path = "/api/v1/settings/skills", operation_id = "settings.saveSkills", request_body = UpdateSkillsSettingsRequest, responses(StudioApiErrors, (status = 200, body = StudioSettingsSnapshot)))]
async fn save_skills_settings(
    State(state): State<AppState>,
    ApiJson(request): ApiJson<UpdateSkillsSettingsRequest>,
) -> Result<Json<StudioSettingsSnapshot>, ApiError> {
    Ok(Json(
        state
            .runtime
            .save_skills_settings(request)
            .await
            .map_err(ApiError::from)?,
    ))
}

#[utoipa::path(put, path = "/api/v1/settings/mcp", operation_id = "settings.saveMcp", request_body = UpdateMcpSettingsRequest, responses(StudioApiErrors, (status = 200, body = StudioSettingsSnapshot)))]
async fn save_mcp_settings(
    State(state): State<AppState>,
    ApiJson(request): ApiJson<UpdateMcpSettingsRequest>,
) -> Result<Json<StudioSettingsSnapshot>, ApiError> {
    Ok(Json(
        state
            .runtime
            .save_mcp_settings(request)
            .await
            .map_err(ApiError::from)?,
    ))
}

#[utoipa::path(put, path = "/api/v1/settings/general", operation_id = "settings.saveGeneral", request_body = UpdateGeneralSettingsRequest, responses(StudioApiErrors, (status = 200, body = StudioSettingsSnapshot)))]
async fn save_general_settings(
    State(state): State<AppState>,
    ApiJson(request): ApiJson<UpdateGeneralSettingsRequest>,
) -> Result<Json<StudioSettingsSnapshot>, ApiError> {
    Ok(Json(
        state
            .runtime
            .save_general_settings(request)
            .map_err(ApiError::from)?,
    ))
}

#[utoipa::path(put, path = "/api/v1/settings/role", operation_id = "settings.setModelRole", request_body = SetModelRoleRequest, responses(StudioApiErrors, (status = 200, body = StudioSettingsSnapshot)))]
async fn save_model_role(
    State(state): State<AppState>,
    ApiJson(request): ApiJson<SetModelRoleRequest>,
) -> Result<Json<StudioSettingsSnapshot>, ApiError> {
    Ok(Json(
        state
            .runtime
            .save_model_role(request)
            .map_err(ApiError::from)?,
    ))
}

#[utoipa::path(get, path = "/api/v1/runtime/provider-usage", operation_id = "providerUsage.read", responses(StudioApiErrors, (status = 200)))]
async fn read_provider_usage(State(state): State<AppState>) -> Result<impl IntoResponse, ApiError> {
    Ok(Json(state.runtime.read_provider_usage_state().await))
}

#[utoipa::path(post, path = "/api/v1/runtime/provider-usage/check", operation_id = "providerUsage.check", responses(StudioApiErrors, (status = 200)))]
async fn check_provider_usage(
    State(state): State<AppState>,
) -> Result<impl IntoResponse, ApiError> {
    Ok(Json(
        state
            .runtime
            .check_provider_usage()
            .await
            .map_err(ApiError::from)?,
    ))
}

#[utoipa::path(get, path = "/api/v1/runtime/projects/{project_id}/skills", operation_id = "skills.read", params(("project_id" = String, Path)), responses(StudioApiErrors, (status = 200)))]
async fn read_skills(
    State(state): State<AppState>,
    Path(project_id): Path<String>,
) -> Result<impl IntoResponse, ApiError> {
    Ok(Json(state.runtime.read_skills_state(&project_id).await))
}

#[utoipa::path(post, path = "/api/v1/runtime/projects/{project_id}/skills/discover", operation_id = "skills.discover", params(("project_id" = String, Path)), responses(StudioApiErrors, (status = 200)))]
async fn discover_skills(
    State(state): State<AppState>,
    Path(project_id): Path<String>,
) -> Result<impl IntoResponse, ApiError> {
    let snapshot = state
        .runtime
        .discover_skills(&project_id)
        .await
        .map_err(ApiError::from)?;
    Ok(Json(pl_studio_runtime::StudioSkillsStateSnapshot::from(
        snapshot,
    )))
}

#[utoipa::path(get, path = "/api/v1/runtime/mcp", operation_id = "mcp.read", responses(StudioApiErrors, (status = 200)))]
async fn read_mcp(State(state): State<AppState>) -> Result<impl IntoResponse, ApiError> {
    Ok(Json(
        state
            .runtime
            .read_mcp_state()
            .await
            .map_err(ApiError::from)?,
    ))
}

#[utoipa::path(post, path = "/api/v1/runtime/mcp/reset", operation_id = "mcp.reset", request_body = McpResetRequest, responses(StudioApiErrors, (status = 200)))]
async fn reset_mcp(
    State(state): State<AppState>,
    ApiJson(request): ApiJson<McpResetRequest>,
) -> Result<impl IntoResponse, ApiError> {
    state
        .runtime
        .reset_mcp(request)
        .await
        .map_err(ApiError::from)?;
    Ok(Json(
        state
            .runtime
            .read_mcp_state()
            .await
            .map_err(ApiError::from)?,
    ))
}

#[utoipa::path(get, path = "/api/v1/runtime/lsp", operation_id = "lsp.read", responses(StudioApiErrors, (status = 200)))]
async fn read_lsp(State(state): State<AppState>) -> Result<impl IntoResponse, ApiError> {
    Ok(Json(state.runtime.read_lsp_state().await))
}

#[utoipa::path(post, path = "/api/v1/runtime/projects/{project_id}/lsp/probe", operation_id = "lsp.probe", params(("project_id" = String, Path)), responses(StudioApiErrors, (status = 200)))]
async fn probe_lsp(
    State(state): State<AppState>,
    Path(project_id): Path<String>,
) -> Result<impl IntoResponse, ApiError> {
    state
        .runtime
        .probe_lsp_server(&project_id)
        .await
        .map_err(ApiError::from)?;
    Ok(Json(state.runtime.read_lsp_state().await))
}

#[utoipa::path(post, path = "/api/v1/runtime/projects/{project_id}/lsp/{server_id}/repair", operation_id = "lsp.repair", params(("project_id" = String, Path), ("server_id" = String, Path)), responses(StudioApiErrors, (status = 200)))]
async fn repair_lsp(
    State(state): State<AppState>,
    Path((project_id, server_id)): Path<(String, String)>,
) -> Result<impl IntoResponse, ApiError> {
    state
        .runtime
        .repair_lsp_server(&project_id, &server_id)
        .await
        .map_err(ApiError::from)?;
    Ok(Json(state.runtime.read_lsp_state().await))
}

#[utoipa::path(post, path = "/api/v1/runtime/lsp/reset", operation_id = "lsp.reset", request_body = LspResetRequest, responses(StudioApiErrors, (status = 200)))]
async fn reset_lsp(
    State(state): State<AppState>,
    ApiJson(request): ApiJson<LspResetRequest>,
) -> Result<impl IntoResponse, ApiError> {
    state
        .runtime
        .reset_lsp_request(request)
        .await
        .map_err(ApiError::from)?;
    Ok(Json(state.runtime.read_lsp_state().await))
}

#[utoipa::path(get, path = "/api/v1/runtime/update", operation_id = "update.read", responses(StudioApiErrors, (status = 200)))]
async fn read_update(State(state): State<AppState>) -> Result<impl IntoResponse, ApiError> {
    Ok(Json(state.runtime.read_update_state().await))
}

#[utoipa::path(post, path = "/api/v1/runtime/update/check", operation_id = "update.check", responses(StudioApiErrors, (status = 200)))]
async fn check_update(State(state): State<AppState>) -> Result<impl IntoResponse, ApiError> {
    Ok(Json(
        state
            .runtime
            .check_studio_update()
            .await
            .map_err(ApiError::from)?,
    ))
}

#[utoipa::path(post, path = "/api/v1/recovery/tasks/{root_thread_id}/preview", operation_id = "recovery.taskPreview", params(("root_thread_id" = String, Path)), responses(StudioApiErrors, (status = 200)))]
async fn preview_task_recovery(
    State(state): State<AppState>,
    Path(root_thread_id): Path<String>,
) -> Result<impl IntoResponse, ApiError> {
    Ok(Json(
        state
            .runtime
            .preview_task_recovery(&root_thread_id)
            .await
            .map_err(ApiError::from)?,
    ))
}

#[utoipa::path(post, path = "/api/v1/recovery/tasks/apply", operation_id = "recovery.taskApply", request_body = StudioTaskRecoveryRequest, responses(StudioApiErrors, (status = 200)))]
async fn apply_task_recovery(
    State(state): State<AppState>,
    ApiJson(request): ApiJson<StudioTaskRecoveryRequest>,
) -> Result<impl IntoResponse, ApiError> {
    Ok(Json(
        state
            .runtime
            .apply_task_recovery(request)
            .await
            .map_err(ApiError::from)?,
    ))
}

#[utoipa::path(post, path = "/api/v1/recovery/issues/{issue_id}/cleanup-preview", operation_id = "recovery.issueCleanupPreview", params(("issue_id" = String, Path)), responses(StudioApiErrors, (status = 200)))]
async fn preview_issue_cleanup(
    State(state): State<AppState>,
    Path(issue_id): Path<String>,
) -> Result<impl IntoResponse, ApiError> {
    Ok(Json(
        state
            .runtime
            .preview_recovery_issue_cleanup(&issue_id)
            .await
            .map_err(ApiError::from)?,
    ))
}

#[utoipa::path(post, path = "/api/v1/recovery/issues/{issue_id}/cleanup", operation_id = "recovery.issueCleanup", params(("issue_id" = String, Path)), request_body = ExpectedOpaqueRevisionRequest, responses(StudioApiErrors, (status = 200)))]
async fn cleanup_issue(
    State(state): State<AppState>,
    Path(issue_id): Path<String>,
    ApiJson(request): ApiJson<ExpectedOpaqueRevisionRequest>,
) -> Result<impl IntoResponse, ApiError> {
    Ok(Json(
        state
            .runtime
            .cleanup_recovery_issue(&issue_id, &request.expected_revision)
            .await
            .map_err(ApiError::from)?,
    ))
}

#[utoipa::path(post, path = "/api/v1/recovery/issues/{issue_id}/retry", operation_id = "recovery.issueRetry", params(("issue_id" = String, Path)), responses(StudioApiErrors, (status = 200)))]
async fn retry_issue(
    State(state): State<AppState>,
    Path(issue_id): Path<String>,
) -> Result<impl IntoResponse, ApiError> {
    Ok(Json(
        state
            .runtime
            .retry_recovery_issue(&issue_id)
            .await
            .map_err(ApiError::from)?,
    ))
}

#[utoipa::path(post, path = "/api/v1/projects/{project_id}/cleanup-preview", operation_id = "recovery.projectCleanupPreview", params(("project_id" = String, Path)), responses(StudioApiErrors, (status = 200)))]
async fn preview_project_cleanup(
    State(state): State<AppState>,
    Path(project_id): Path<String>,
) -> Result<impl IntoResponse, ApiError> {
    Ok(Json(
        state
            .runtime
            .preview_project_cleanup(&project_id)
            .await
            .map_err(ApiError::from)?,
    ))
}

#[utoipa::path(post, path = "/api/v1/projects/{project_id}/cleanup", operation_id = "recovery.projectCleanup", params(("project_id" = String, Path)), request_body = ExpectedOpaqueRevisionRequest, responses(StudioApiErrors, (status = 200)))]
async fn cleanup_project(
    State(state): State<AppState>,
    Path(project_id): Path<String>,
    ApiJson(request): ApiJson<ExpectedOpaqueRevisionRequest>,
) -> Result<impl IntoResponse, ApiError> {
    Ok(Json(
        state
            .runtime
            .cleanup_project(&project_id, &request.expected_revision)
            .await
            .map_err(ApiError::from)?,
    ))
}

fn parse_mode(mode: &str) -> Result<StudioMode, ApiError> {
    StudioMode::from_label(mode.trim()).map_err(|_| {
        ApiError(StudioError::invalid_argument(format!(
            "unsupported Thread mode: {}",
            mode.trim()
        )))
    })
}
