use std::collections::BTreeSet;
use std::path::Path;
use std::pin::Pin;
use std::sync::Arc;

use pl_protocol::{PureError, Result};
use sha2::{Digest, Sha256};
use tokio_util::sync::CancellationToken;

use crate::skill::{
    SKILL_FILE_NAME, SkillCandidate, SkillDefinition, SkillProvider, SkillProviderId,
    SkillProviderObservation, SkillProviderRequest, SkillResourceBase, SkillSourceKind,
    SkillSummary, support_file_path, validate_skill_document,
};
use crate::tool::{WorkspaceFileBackend, WorkspaceFileListRequest, WorkspaceFileReadRequest};

use super::RemoteWorkspaceFileBackend;

const REMOTE_SKILL_PROVIDER_ID: &str = "remote-workspace";
const MAX_REMOTE_SKILLS: usize = 512;

/// 在本地解析和校验远端 workspace Skill 文档的 Provider。
#[derive(Debug)]
pub struct RemoteSkillProvider {
    id: SkillProviderId,
    backend: Arc<RemoteWorkspaceFileBackend>,
}

impl RemoteSkillProvider {
    /// 创建绑定到一个已打开远端 workspace 的 Skill provider。
    pub fn new(backend: Arc<RemoteWorkspaceFileBackend>) -> Result<Self> {
        Ok(Self {
            id: SkillProviderId::new(REMOTE_SKILL_PROVIDER_ID)?,
            backend,
        })
    }
}

impl SkillProvider for RemoteSkillProvider {
    fn id(&self) -> &SkillProviderId {
        &self.id
    }

    fn list<'a>(
        &'a self,
        request: SkillProviderRequest,
    ) -> Pin<Box<dyn Future<Output = Result<SkillProviderObservation>> + Send + 'a>> {
        Box::pin(async move {
            ensure_not_cancelled(&request.cancellation)?;
            let root = request.config.project_dir.clone();
            if self
                .backend
                .stat_optional(root.clone(), None)
                .await?
                .is_none()
            {
                return Ok(SkillProviderObservation {
                    candidates: Vec::new(),
                    complete: true,
                    warnings: Vec::new(),
                });
            }
            let listing = self
                .backend
                .list(WorkspaceFileListRequest {
                    path: root.clone(),
                    cwd: None,
                    glob: format!("**/{SKILL_FILE_NAME}"),
                    max_files: MAX_REMOTE_SKILLS,
                    include_dirs: false,
                })
                .await?;
            let disabled = request
                .config
                .disabled
                .iter()
                .map(|name| name.to_ascii_lowercase())
                .collect::<BTreeSet<_>>();
            let mut candidates = Vec::new();
            let mut warnings = Vec::new();
            for (local_order, path) in listing.files.into_iter().enumerate() {
                ensure_not_cancelled(&request.cancellation)?;
                match self.read_candidate(&root, path, local_order).await {
                    Ok(candidate)
                        if !disabled.contains(&candidate.summary.name.to_ascii_lowercase())
                            && remote_platform_matches(&candidate.summary.platforms) =>
                    {
                        candidates.push(candidate);
                    }
                    Ok(_) => {}
                    Err(error) => warnings.push(error.to_string()),
                }
            }
            Ok(SkillProviderObservation {
                candidates,
                complete: !listing.truncated,
                warnings,
            })
        })
    }

    fn load<'a>(
        &'a self,
        candidate: &'a SkillCandidate,
        cancellation: CancellationToken,
    ) -> Pin<Box<dyn Future<Output = Result<SkillDefinition>> + Send + 'a>> {
        Box::pin(async move {
            ensure_not_cancelled(&cancellation)?;
            let skill_file = join_remote(&candidate.locator, SKILL_FILE_NAME)?;
            let content = self.read_text(skill_file).await?;
            let mut metadata = validate_skill_document(&content, None)?;
            metadata.source = SkillSourceKind::Project;
            metadata.provider_id = self.id.clone();
            metadata.resource_base = SkillResourceBase::Opaque {
                description: candidate.locator.clone(),
            };
            Ok(SkillDefinition {
                summary: SkillSummary::from(metadata),
                revision: content_revision(&content),
                content,
            })
        })
    }

    fn read_resource<'a>(
        &'a self,
        candidate: &'a SkillCandidate,
        relative_path: &'a str,
        cancellation: CancellationToken,
    ) -> Pin<Box<dyn Future<Output = Result<String>> + Send + 'a>> {
        Box::pin(async move {
            ensure_not_cancelled(&cancellation)?;
            let relative = support_file_path(relative_path)?;
            let relative = relative.to_str().ok_or_else(|| {
                PureError::ConfigError("remote Skill resource path must be UTF-8".to_string())
            })?;
            self.read_text(join_remote(&candidate.locator, relative)?)
                .await
        })
    }
}

impl RemoteSkillProvider {
    async fn read_candidate(
        &self,
        root: &str,
        skill_file: String,
        local_order: usize,
    ) -> Result<SkillCandidate> {
        let content = self.read_text(skill_file.clone()).await?;
        let mut metadata = validate_skill_document(&content, None)?;
        let locator = Path::new(&skill_file)
            .parent()
            .and_then(Path::to_str)
            .ok_or_else(|| {
                PureError::ConfigError(format!("invalid remote Skill path: {skill_file}"))
            })?
            .replace('\\', "/");
        if metadata.category.is_none() {
            metadata.category = remote_category(root, &locator);
        }
        metadata.source = SkillSourceKind::Project;
        metadata.provider_id = self.id.clone();
        metadata.resource_base = SkillResourceBase::Opaque {
            description: locator.clone(),
        };
        Ok(SkillCandidate {
            summary: SkillSummary::from(metadata),
            locator,
            revision: content_revision(&content),
            rank: 0,
            local_order,
        })
    }

    async fn read_text(&self, path: String) -> Result<String> {
        self.backend
            .read_text(WorkspaceFileReadRequest { path, cwd: None })
            .await
    }
}

fn join_remote(base: &str, relative: &str) -> Result<String> {
    let relative = relative.replace('\\', "/");
    if relative.starts_with('/') || relative.split('/').any(|part| part == "..") {
        return Err(PureError::ConfigError(
            "remote Skill path escapes its directory".to_string(),
        ));
    }
    Ok(format!("{}/{relative}", base.trim_end_matches('/')))
}

fn remote_category(root: &str, locator: &str) -> Option<String> {
    let relative = locator
        .strip_prefix(root.trim_end_matches('/'))?
        .trim_start_matches('/');
    Path::new(relative)
        .parent()
        .and_then(Path::to_str)
        .map(|category| category.replace('\\', "/"))
        .filter(|category| !category.is_empty())
}

fn remote_platform_matches(platforms: &[String]) -> bool {
    platforms.is_empty() || platforms.iter().any(|platform| platform == "linux")
}

fn ensure_not_cancelled(cancellation: &CancellationToken) -> Result<()> {
    if cancellation.is_cancelled() {
        Err(PureError::ConfigError(
            "remote Skill operation was cancelled".to_string(),
        ))
    } else {
        Ok(())
    }
}

fn content_revision(content: &str) -> String {
    format!("{:x}", Sha256::digest(content.as_bytes()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn support_resource_join_stays_inside_skill() {
        assert_eq!(
            join_remote(".agents/skills/example", "references/info.md").expect("path"),
            ".agents/skills/example/references/info.md"
        );
        assert!(join_remote(".agents/skills/example", "../AGENTS.md").is_err());
    }
}
