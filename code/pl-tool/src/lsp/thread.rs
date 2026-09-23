//! Dynamic LSP invocation without a product session or mutable tool call context.
use super::{LspCapabilitiesInput, LspQueryInput};
use crate::workspace::ToolWorkspace;
use pl_core::{
    context::{ContextContent, OpaquePayload},
    tool::{
        ToolOutput,
        opaque::{CallContext, Registration, RegistryError, Tool, ToolError},
    },
};
use pl_lsp::{query::LspQuery, runtime::LspRuntimeRegistry};
use std::sync::Arc;

/// Paths belong to the physical service host selected during Thread assembly.
#[derive(Debug, Clone)]
pub enum LspPathBinding {
    Local(ToolWorkspace),
    Remote {
        workspace: ToolWorkspace,
        files: Arc<crate::remote::RemoteWorkspaceFileBackend>,
    },
}

impl LspPathBinding {
    fn workspace(&self) -> &ToolWorkspace {
        match self {
            Self::Local(workspace) | Self::Remote { workspace, .. } => workspace,
        }
    }

    async fn resolve(
        &self,
        mut query: LspQuery,
        grant: &pl_core::tool::execution_policy::ExecutionGrant,
    ) -> Result<LspQuery, ToolError> {
        if let Some(path) = query.file_path.take() {
            let operation = query.operation;
            let resolved = match self {
                Self::Local(workspace) => {
                    let root = workspace.root().to_owned();
                    let host_access = workspace.workspace().boundary().allows_host_paths()
                        && grant.contains(crate::approval::HOST_WORKSPACE_ACCESS);
                    tokio::task::spawn_blocking(move || {
                        let policy =
                            crate::workspace::ToolPathPolicy::new(root, host_access, "lsp_query")?;
                        let display = path.display().to_string();
                        if operation.requires_file() {
                            policy.resolve_existing_path(&path, &display)
                        } else {
                            policy.resolve_existing_or_parent_path(&path, &display)
                        }
                    })
                    .await
                    .map_err(ToolError::new)?
                    .map_err(ToolError::new)?
                }
                Self::Remote { files, .. } => {
                    crate::remote::resolve_lsp_query_path(files, &path, operation)
                        .await
                        .map_err(ToolError::new)?
                }
            };
            query.file_path = Some(resolved);
        }
        Ok(query)
    }
}

/// Stable LSP tool operations; current service readiness is returned as data, not inserted into schemas.
#[derive(Debug, Clone, Copy)]
pub enum LspToolKind {
    Capabilities,
    Query,
}

impl LspToolKind {
    /// Stable model declaration for this operation.
    pub fn declaration(self) -> pl_protocol::ToolSpec {
        let spec = match self {
            Self::Capabilities => pl_protocol::ToolSpec::function(
                "lsp_capabilities",
                "List language IDs, query operations and readiness for this workspace.",
                schemars::schema_for!(LspCapabilitiesInput).to_value(),
            ),
            Self::Query => pl_protocol::ToolSpec::function(
                "lsp_query",
                "Query workspace language services for definitions, references, hover, symbols, call hierarchy or diagnostics. Discover supported languages with lsp_capabilities.",
                schemars::schema_for!(LspQueryInput).to_value(),
            ),
        };
        spec.allow_programmatic(serde_json::json!({"type":"object", "additionalProperties":true}))
    }
}

/// A Thread-owned tool instance over an explicitly shared language-service registry.
#[derive(Debug)]
pub struct ThreadLspTool {
    registry: LspRuntimeRegistry,
    paths: LspPathBinding,
    kind: LspToolKind,
}

impl ThreadLspTool {
    /// Binds service lookup and path validation to the same workspace.
    pub fn new(registry: LspRuntimeRegistry, paths: LspPathBinding, kind: LspToolKind) -> Self {
        Self {
            registry,
            paths,
            kind,
        }
    }

    /// Transfers an instance as a deferred tool with no framework control permissions.
    ///
    /// # Errors
    /// Returns invalid registration identity errors.
    pub fn registration(self, declaration: OpaquePayload) -> Result<Registration, RegistryError> {
        let name = match self.kind {
            LspToolKind::Capabilities => "lsp_capabilities",
            LspToolKind::Query => "lsp_query",
        };
        let authorization = self.paths.workspace().authorization();
        Ok(Registration::new(name.into(), declaration, self)?
            .deferred()
            .with_authorization(authorization))
    }
}

impl Tool for ThreadLspTool {
    async fn execute(
        &self,
        input: OpaquePayload,
        context: CallContext,
    ) -> Result<ToolOutput, ToolError> {
        if input.format() != "application/json" || input.version() != 1 {
            return Err(ToolError::new(crate::tool_error(
                "lsp",
                "unsupported argument encoding",
            )));
        }
        if context.cancellation.is_cancelled() {
            return Err(ToolError::new(pl_core::thread::ThreadError::Cancelled));
        }
        let encoded = match self.kind {
            LspToolKind::Capabilities => {
                let _: LspCapabilitiesInput =
                    serde_json::from_str(input.content()).map_err(ToolError::new)?;
                let result = self
                    .registry
                    .capabilities_for_workspace(self.paths.workspace().root())
                    .await;
                serde_json::to_string(&result).map_err(ToolError::new)?
            }
            LspToolKind::Query => {
                let input: LspQueryInput =
                    serde_json::from_str(input.content()).map_err(ToolError::new)?;
                let language = input.language_id.clone();
                let query = self
                    .paths
                    .resolve(
                        LspQuery {
                            operation: input.operation,
                            file_path: input.file_path,
                            line: input.line,
                            character: input.character,
                            query: input.query,
                            max_results: input.max_results,
                            language_id: Some(input.language_id),
                        },
                        &context.grant,
                    )
                    .await?;
                if context.cancellation.is_cancelled() {
                    return Err(ToolError::new(pl_core::thread::ThreadError::Cancelled));
                }
                let result = self
                    .registry
                    .query_in_workspace(self.paths.workspace().root(), query)
                    .await
                    .map_err(|source| ToolError::new(QueryFailure { language, source }))?;
                serde_json::to_string(&result).map_err(ToolError::new)?
            }
        };
        let preview = pl_output::bounded_text(&encoded, 12 * 1024, 0);
        let text = if preview.truncated {
            format!(
                "{}\n[LSP preview omitted {} bytes; full result retained in history.]",
                preview.text, preview.bytes_omitted
            )
        } else {
            preview.text
        };
        Ok(ToolOutput::new(
            OpaquePayload::new("pl.tool.lsp", 1, encoded).map_err(ToolError::new)?,
            vec![ContextContent::Text {
                text: Arc::from(text),
            }],
        ))
    }
}

#[derive(Debug, thiserror::Error)]
#[error(
    "LSP query for languageId `{language}` failed: {source}; lsp_capabilities lists current service readiness"
)]
struct QueryFailure {
    language: String,
    #[source]
    source: pl_lsp::runtime::LspRuntimeError,
}
