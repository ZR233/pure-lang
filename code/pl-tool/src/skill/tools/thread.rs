//! Skill discovery and actual read snapshots at the opaque Thread boundary.
use super::{
    FrozenSkillCatalog, SKILL_FILE_NAME, SkillLoadInvocation, SkillViewInput, SkillViewOutput,
    SkillsListInput, is_main_skill_path, list_snapshot,
};
use pl_core::{
    context::{ContextContent, OpaquePayload},
    thread::extensions::ExtensionMutation,
    tool::{
        ToolOutput,
        opaque::{CallContext, Registration, RegistryError, Tool, ToolError},
    },
};
use sha2::{Digest, Sha256};
use std::sync::Arc;

/// Stable tool operations; catalog contents and activation state are result data.
#[derive(Debug, Clone, Copy)]
pub enum ThreadSkillKind {
    List,
    View,
}

impl ThreadSkillKind {
    /// Declares the input owned by this tool, without embedding mutable catalog rows.
    pub fn declaration(self) -> pl_protocol::ToolSpec {
        match self {
            Self::List => pl_protocol::ToolSpec::function(
                "skills_list",
                "List or search available skill names and descriptions before loading one by exact name.",
                schemars::schema_for!(SkillsListInput).to_value(),
            ),
            Self::View => pl_protocol::ToolSpec::function(
                "skill_view",
                "Read a skill or a support file. The actual text is retained in history; use it as tool-provided guidance.",
                schemars::schema_for!(SkillViewInput).to_value(),
            ),
        }
    }
}

/// An instance owns its catalog lease; replacing a directory does not rerender earlier results.
#[derive(Debug)]
pub struct ThreadSkillTool {
    catalog: Arc<FrozenSkillCatalog>,
    kind: ThreadSkillKind,
}

impl ThreadSkillTool {
    /// Binds this tool to a frozen provider directory.
    pub fn new(catalog: Arc<FrozenSkillCatalog>, kind: ThreadSkillKind) -> Self {
        Self { catalog, kind }
    }

    /// Transfers an instance, granting state updates only to the tool that saves read snapshots.
    ///
    /// # Errors
    /// Returns invalid tool identity errors.
    pub fn registration(self, declaration: OpaquePayload) -> Result<Registration, RegistryError> {
        let kind = self.kind;
        let name = match kind {
            ThreadSkillKind::List => "skills_list",
            ThreadSkillKind::View => "skill_view",
        };
        let mut policies = self
            .catalog
            .snapshot()
            .skills
            .iter()
            .map(|skill| {
                format!(
                    "{:?}",
                    (
                        skill.name.to_ascii_lowercase(),
                        skill.provider_id.as_str(),
                        skill.invocation,
                        &skill.resource_base
                    )
                )
            })
            .collect::<Vec<_>>();
        policies.sort();
        let mut digest = Sha256::new();
        for policy in policies {
            digest.update((policy.len() as u64).to_le_bytes());
            digest.update(policy.as_bytes());
        }
        let authorization = pl_core::tool::opaque::ToolAuthorization::new(format!(
            "pl.skills:{:x}",
            digest.finalize()
        ));
        let registration =
            Registration::new(name.into(), declaration, self)?.with_authorization(authorization);
        Ok(match kind {
            ThreadSkillKind::List => registration,
            ThreadSkillKind::View => registration.with_extension_updates(),
        })
    }
}

impl Tool for ThreadSkillTool {
    async fn execute(
        &self,
        input: OpaquePayload,
        context: CallContext,
    ) -> Result<ToolOutput, ToolError> {
        if input.format() != "application/json" || input.version() != 1 {
            return Err(error("unsupported skill argument encoding"));
        }
        if context.cancellation.is_cancelled() {
            return Err(error("skill operation cancelled"));
        }
        match self.kind {
            ThreadSkillKind::List => {
                let input: SkillsListInput =
                    serde_json::from_str(input.content()).map_err(ToolError::new)?;
                let value = list_snapshot(&self.catalog, input).map_err(ToolError::new)?;
                let encoded = serde_json::to_string(&value).map_err(ToolError::new)?;
                Ok(ToolOutput::new(
                    OpaquePayload::new("pl.tool.skill-list", 1, encoded.clone())
                        .map_err(ToolError::new)?,
                    vec![ContextContent::Text {
                        text: Arc::from(encoded),
                    }],
                ))
            }
            ThreadSkillKind::View => {
                let input: SkillViewInput =
                    serde_json::from_str(input.content()).map_err(ToolError::new)?;
                let definition = self
                    .catalog
                    .load(
                        &input.target.name,
                        SkillLoadInvocation::Model,
                        context.cancellation.clone(),
                    )
                    .await
                    .map_err(ToolError::new)?;
                let resource = input
                    .file_path
                    .as_deref()
                    .map(str::trim)
                    .filter(|path| !is_main_skill_path(path));
                let (file_path, content) = match resource {
                    Some(path) => (
                        path.to_owned(),
                        self.catalog
                            .read_resource(
                                &input.target.name,
                                path,
                                SkillLoadInvocation::Model,
                                context.cancellation.clone(),
                            )
                            .await
                            .map_err(ToolError::new)?,
                    ),
                    None => (SKILL_FILE_NAME.to_owned(), definition.content),
                };
                let visible = vec![ContextContent::Text {
                    text: Arc::from(format!(
                        "Skill: {}\nResource: {file_path}\n\n{content}",
                        definition.summary.name
                    )),
                }];
                let result = SkillViewOutput {
                    success: true,
                    skill: definition.summary.clone(),
                    file_path,
                    resource_base: definition.summary.resource_base,
                    resource_hint: "Use filePath to read support resources on demand.".into(),
                    content,
                };
                let payload = OpaquePayload::new(
                    "pl.tool.skill-view",
                    1,
                    serde_json::to_string(&result).map_err(ToolError::new)?,
                )
                .map_err(ToolError::new)?;
                let output = ToolOutput::new(payload.clone(), visible);
                if let Err(source) = self
                    .catalog
                    .record_model_view(&input.target.name, context.cancellation)
                    .await
                {
                    return Err(ToolError::new(source).with_output(output));
                }
                let id = format!(
                    "pl.tool.skill-view:{:x}",
                    Sha256::digest(result.skill.name.to_ascii_lowercase().as_bytes())
                );
                let previous = context.extensions.get(&id);
                if previous.is_some_and(|record| record.payload == payload) {
                    return Ok(output);
                }
                Ok(
                    output.with_extension_mutations(vec![ExtensionMutation::Put {
                        id,
                        expected_revision: previous.map(|record| record.revision),
                        payload,
                    }]),
                )
            }
        }
    }
}

/// Local project Skill mutation, explicitly installed by the product host.
#[derive(Debug)]
pub struct ThreadSkillManageTool {
    catalog: Arc<FrozenSkillCatalog>,
    workspace: crate::workspace::ToolWorkspace,
}

impl ThreadSkillManageTool {
    /// Binds a local project catalog to its writable workspace policy.
    pub fn new(
        catalog: Arc<FrozenSkillCatalog>,
        workspace: crate::workspace::ToolWorkspace,
    ) -> Self {
        Self { catalog, workspace }
    }

    /// Stable mutation schema; no live skill names or directory revisions are embedded.
    pub fn declaration() -> pl_protocol::ToolSpec {
        pl_protocol::ToolSpec::function(
            "skill_manage",
            "Create, patch, edit or remove local project skills and their support files within the configured writable paths.",
            crate::typed_tool_input_schema::<super::SkillManageInput>(),
        )
    }

    /// Installs a foreground mutator under the workspace's authorization identity.
    ///
    /// # Errors
    /// Returns invalid registration identity errors.
    pub fn registration(self, declaration: OpaquePayload) -> Result<Registration, RegistryError> {
        let authorization = self.workspace.authorization();
        Ok(Registration::new("skill_manage".into(), declaration, self)?
            .foreground()
            .with_authorization(authorization))
    }
}

impl Tool for ThreadSkillManageTool {
    async fn execute(
        &self,
        input: OpaquePayload,
        context: CallContext,
    ) -> Result<ToolOutput, ToolError> {
        if input.format() != "application/json" || input.version() != 1 {
            return Err(error("unsupported skill management argument encoding"));
        }
        let input: super::SkillManageInput =
            serde_json::from_str(input.content()).map_err(ToolError::new)?;
        let result = super::manage_snapshot(
            self.catalog.clone(),
            self.workspace.clone(),
            super::ManageRequest {
                input,
                cancellation: context.cancellation,
            },
        )
        .await
        .map_err(ToolError::new)?;
        let encoded = serde_json::to_string(&result).map_err(ToolError::new)?;
        Ok(ToolOutput::new(
            OpaquePayload::new("pl.tool.skill-mutation", 1, encoded.clone())
                .map_err(ToolError::new)?,
            vec![ContextContent::Text {
                text: Arc::from(encoded),
            }],
        ))
    }
}

fn error(message: &str) -> ToolError {
    ToolError::new(super::tool_error("skills", message))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::thread_context;
    use pretty_assertions::assert_eq;

    #[tokio::test]
    async fn frozen_skill_view_keeps_delivered_text_and_deduplicates_case_insensitive_state() {
        let directory = tempfile::tempdir().unwrap();
        let skill = directory.path().join(".agents/skills/demo");
        tokio::fs::create_dir_all(&skill).await.unwrap();
        let original = "---\nname: demo\ndescription: Original guidance\n---\n# Demo\nKeep these exact instructions.\n";
        tokio::fs::write(skill.join("SKILL.md"), original)
            .await
            .unwrap();
        let mut config = crate::skill::SkillsConfig {
            user_dir: directory
                .path()
                .join("user-skills")
                .to_string_lossy()
                .into_owned(),
            ..Default::default()
        };
        config.system.enabled = false;
        let catalog = Arc::new(
            crate::skill::discover_local_skills(
                directory.path(),
                &config,
                None,
                tokio_util::sync::CancellationToken::new(),
            )
            .await
            .unwrap(),
        );
        let tool = ThreadSkillTool::new(catalog, ThreadSkillKind::View);
        let input = |name: &str| {
            OpaquePayload::new(
                "application/json",
                1,
                serde_json::json!({"name": name}).to_string(),
            )
            .unwrap()
        };
        let output = tool.execute(input("demo"), thread_context()).await.unwrap();
        let saved: serde_json::Value = serde_json::from_str(output.payload().content()).unwrap();
        assert_eq!(saved["content"], original);
        let [ExtensionMutation::Put { id, payload, .. }] = output.extension_mutations() else {
            panic!("view creates a snapshot");
        };
        let mut context = thread_context();
        context.extensions = Arc::new(std::collections::BTreeMap::from([(
            id.clone(),
            pl_core::thread::extensions::ExtensionRecord {
                revision: 1,
                payload: payload.clone(),
            },
        )]));
        let repeated = tool.execute(input("DEMO"), context).await.unwrap();
        assert_eq!(repeated.payload(), output.payload());
        assert!(repeated.extension_mutations().is_empty());
        tokio::fs::write(skill.join("SKILL.md"), "new file contents")
            .await
            .unwrap();
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(output.payload().content()).unwrap()["content"],
            original
        );
        assert!(
            matches!(&output.context()[0], ContextContent::Text { text } if text.contains(original))
        );
    }

    #[tokio::test]
    async fn empty_catalog_lists_without_activating_any_skill() {
        let directory = tempfile::tempdir().unwrap();
        let catalog = Arc::new(FrozenSkillCatalog::empty(directory.path().to_owned()));
        let tool = ThreadSkillTool::new(catalog, ThreadSkillKind::List);
        let output = tool
            .execute(
                OpaquePayload::new("application/json", 1, "{}").unwrap(),
                thread_context(),
            )
            .await
            .unwrap();
        let saved: serde_json::Value = serde_json::from_str(output.payload().content()).unwrap();
        assert_eq!(saved["count"], 0);
        assert_eq!(saved["skills"], serde_json::json!([]));
        assert!(output.extension_mutations().is_empty());
    }
    #[tokio::test]
    async fn dynamic_skill_management_creates_project_data_and_enforces_readonly_policy() {
        let directory = tempfile::tempdir().unwrap();
        let project = directory.path().join(".agents/skills");
        let catalog = Arc::new(FrozenSkillCatalog::empty(project.clone()));
        let workspace = crate::workspace::ToolWorkspace::new(
            crate::workspace::AgentWorkspace::local(directory.path()),
        );
        let tool = ThreadSkillManageTool::new(catalog.clone(), workspace);
        let content = "---\nname: created\ndescription: created skill\n---\n# Saved skill\n";
        let input = OpaquePayload::new(
            "application/json",
            1,
            serde_json::json!({"action":"create", "name":"created", "content":content}).to_string(),
        )
        .unwrap();
        let output = tool.execute(input, thread_context()).await.unwrap();
        assert_eq!(output.payload().format(), "pl.tool.skill-mutation");
        assert_eq!(
            tokio::fs::read_to_string(project.join("created/SKILL.md"))
                .await
                .unwrap(),
            content
        );
        let readonly =
            crate::workspace::ToolWorkspace::new(crate::workspace::AgentWorkspace::confined(
                directory.path(),
                crate::workspace::WorkspaceMutability::ReadOnly,
            ));
        let tool = ThreadSkillManageTool::new(catalog, readonly);
        let input = OpaquePayload::new("application/json", 1, serde_json::json!({"action":"create", "name":"denied", "content":"---\nname: denied\ndescription: denied\n---\nbody"}).to_string()).unwrap();
        assert!(tool.execute(input, thread_context()).await.is_err());
        assert!(!project.join("denied").exists());
    }
}
