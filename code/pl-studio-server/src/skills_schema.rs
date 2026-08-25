//! OpenAPI-only wire schema for the transport-neutral Studio Skills snapshot.

use serde::Serialize;
use utoipa::ToSchema;

#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SkillsStateSnapshotSchema {
    pub project_id: String,
    pub state: SkillsObservedStateSchema,
}

#[derive(Debug, Serialize, ToSchema)]
#[serde(tag = "kind", content = "data", rename_all = "camelCase")]
#[expect(
    dead_code,
    reason = "OpenAPI-only variants mirror the Skills wire union"
)]
pub(crate) enum SkillsObservedStateSchema {
    Uninitialized(UninitializedSchema),
    Loading(LoadingSchema),
    Ready(ReadySchema),
    Refreshing(RefreshingSchema),
    Stale(StaleSchema),
    Degraded(DegradedSchema),
    Failed(FailedSchema),
    Stopped(StoppedSchema),
}

#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct UninitializedSchema {
    pub revision: u64,
    pub updated_at: i64,
}

#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct LoadingSchema {
    pub revision: u64,
    pub operation: String,
    pub operation_id: String,
    pub started_at: i64,
}

#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ReadySchema {
    pub revision: u64,
    pub updated_at: i64,
    pub last_checked_at: Option<i64>,
    pub value: SkillsStateDataSchema,
}

#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct RefreshingSchema {
    pub revision: u64,
    pub operation: String,
    pub operation_id: String,
    pub started_at: i64,
    pub last_checked_at: Option<i64>,
    pub value: SkillsStateDataSchema,
}

#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct StaleSchema {
    pub revision: u64,
    pub stale_at: i64,
    pub last_checked_at: Option<i64>,
    pub value: SkillsStateDataSchema,
}

#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct DegradedSchema {
    pub revision: u64,
    pub failed_at: i64,
    pub last_checked_at: Option<i64>,
    pub operation: String,
    pub error: StateErrorSchema,
    pub value: SkillsStateDataSchema,
}

#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct FailedSchema {
    pub revision: u64,
    pub failed_at: i64,
    pub operation: String,
    pub error: StateErrorSchema,
}

#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct StoppedSchema {
    pub revision: u64,
    pub stopped_at: i64,
}

#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct StateErrorSchema {
    pub code: String,
    pub message: String,
    pub retryable: bool,
}

#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SkillsStateDataSchema {
    pub config_fingerprint: String,
    pub catalog_revision: u64,
    pub catalog: SkillCatalogSchema,
}

#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SkillCatalogSchema {
    pub project_dir: String,
    pub skills: Vec<SkillSummarySchema>,
    pub warnings: Vec<String>,
    pub complete: bool,
}

#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SkillSummarySchema {
    pub name: String,
    pub description: String,
    pub category: Option<String>,
    pub platforms: Vec<String>,
    pub source: SkillSourceSchema,
    pub provider_id: String,
    pub invocation: SkillInvocationPolicySchema,
    pub resource_base: SkillResourceBaseSchema,
}

#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
#[expect(
    dead_code,
    reason = "OpenAPI-only variants mirror the Skills wire enum"
)]
pub(crate) enum SkillSourceSchema {
    Project,
    User,
    System,
    External,
}

#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SkillInvocationPolicySchema {
    pub model_invocable: bool,
    pub user_invocable: bool,
}

#[derive(Debug, Serialize, ToSchema)]
#[serde(tag = "kind", rename_all = "camelCase")]
#[expect(
    dead_code,
    reason = "OpenAPI-only variants mirror the resource-base wire union"
)]
pub(crate) enum SkillResourceBaseSchema {
    Directory { path: String },
    Url { url: String },
    Opaque { description: String },
}
