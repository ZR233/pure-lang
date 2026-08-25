use std::collections::BTreeSet;
use std::path::PathBuf;
use std::pin::Pin;

use pl_protocol::{PureError, Result};
use sha2::{Digest, Sha256};
use tokio_util::sync::CancellationToken;

use super::{
    SkillCandidate, SkillDefinition, SkillProvider, SkillProviderId, SkillProviderObservation,
    SkillProviderRequest, cancelled_error,
};
use crate::skill::scanning::{metadata_from_file, scan_skill_files};
use crate::skill::util::platform_matches;
use crate::skill::{SkillMetadata, SkillResourceBase, SkillSummary};

const FILESYSTEM_PROVIDER_ID: &str = "local-filesystem";

/// Local directory provider preserving Pure's on-disk layout and path-safety rules.
#[derive(Debug)]
pub struct FileSystemSkillProvider {
    id: SkillProviderId,
}

impl FileSystemSkillProvider {
    /// Creates the built-in local filesystem Provider.
    pub fn new() -> Self {
        Self {
            id: SkillProviderId(FILESYSTEM_PROVIDER_ID.to_string()),
        }
    }
}

impl Default for FileSystemSkillProvider {
    fn default() -> Self {
        Self::new()
    }
}

impl SkillProvider for FileSystemSkillProvider {
    fn id(&self) -> &SkillProviderId {
        &self.id
    }

    fn list<'a>(
        &'a self,
        request: SkillProviderRequest,
    ) -> Pin<Box<dyn Future<Output = Result<SkillProviderObservation>> + Send + 'a>> {
        let id = self.id.clone();
        Box::pin(async move {
            tokio::task::spawn_blocking(move || list_local_skills(&id, request))
                .await
                .map_err(|error| {
                    PureError::ConfigError(format!(
                        "filesystem skill discovery task failed: {error}"
                    ))
                })?
        })
    }

    fn load<'a>(
        &'a self,
        candidate: &'a SkillCandidate,
        cancellation: CancellationToken,
    ) -> Pin<Box<dyn Future<Output = Result<SkillDefinition>> + Send + 'a>> {
        let candidate = candidate.clone();
        Box::pin(async move {
            tokio::task::spawn_blocking(move || load_local_skill(&candidate, cancellation))
                .await
                .map_err(|error| {
                    PureError::ConfigError(format!("filesystem skill load task failed: {error}"))
                })?
        })
    }

    fn read_resource<'a>(
        &'a self,
        candidate: &'a SkillCandidate,
        relative_path: &'a str,
        cancellation: CancellationToken,
    ) -> Pin<Box<dyn Future<Output = Result<String>> + Send + 'a>> {
        let candidate = candidate.clone();
        let relative_path = relative_path.to_string();
        Box::pin(async move {
            tokio::task::spawn_blocking(move || {
                read_local_resource(&candidate, &relative_path, cancellation)
            })
            .await
            .map_err(|error| {
                PureError::ConfigError(format!("filesystem skill resource task failed: {error}"))
            })?
        })
    }
}

fn load_local_skill(
    candidate: &SkillCandidate,
    cancellation: CancellationToken,
) -> Result<SkillDefinition> {
    if cancellation.is_cancelled() {
        return Err(cancelled_error());
    }
    let skill_path = PathBuf::from(&candidate.locator);
    let content = std::fs::read_to_string(skill_path.join(crate::skill::SKILL_FILE_NAME)).map_err(
        |error| {
            PureError::ConfigError(format!(
                "failed to load skill {}: {error}",
                candidate.summary.name
            ))
        },
    )?;
    let mut metadata = crate::skill::validate_skill_document(&content, None)?;
    metadata.source = candidate.summary.source;
    metadata.path = skill_path;
    metadata.provider_id = candidate.summary.provider_id.clone();
    metadata.resource_base = SkillResourceBase::Directory {
        path: metadata.path.clone(),
    };
    let summary = SkillSummary::from(metadata);
    Ok(SkillDefinition {
        summary,
        revision: content_revision(&content),
        content,
    })
}

fn read_local_resource(
    candidate: &SkillCandidate,
    relative_path: &str,
    cancellation: CancellationToken,
) -> Result<String> {
    if cancellation.is_cancelled() {
        return Err(cancelled_error());
    }
    let metadata = SkillMetadata::from(candidate.summary.clone());
    crate::skill::read_skill_file(&metadata, Some(relative_path)).map(|read| read.content)
}

fn list_local_skills(
    provider_id: &SkillProviderId,
    request: SkillProviderRequest,
) -> Result<SkillProviderObservation> {
    if request.cancellation.is_cancelled() {
        return Err(cancelled_error());
    }
    let agents_user_dir = crate::skill::util::agents_user_skills_dir().ok();
    let sources = crate::skill::catalog::skill_sources(
        &request.workspace_root,
        &request.config,
        request.system_dir.as_deref(),
        agents_user_dir.as_deref(),
    )?;
    let disabled = request
        .config
        .disabled
        .iter()
        .map(|name| name.to_ascii_lowercase())
        .collect::<BTreeSet<_>>();
    let mut observation = SkillProviderObservation::empty();
    let mut local_order = 0;
    for source in sources {
        if request.cancellation.is_cancelled() {
            return Err(cancelled_error());
        }
        let scan = scan_skill_files(&source.root);
        observation.complete &= scan.complete;
        observation.warnings.extend(scan.warnings);
        for skill_file in scan.files {
            if request.cancellation.is_cancelled() {
                return Err(cancelled_error());
            }
            match metadata_from_file(&skill_file, &source.root, source.kind) {
                Ok(mut metadata) => {
                    if disabled.contains(&metadata.name.to_ascii_lowercase())
                        || !platform_matches(&metadata.platforms)
                    {
                        continue;
                    }
                    metadata.provider_id = provider_id.clone();
                    metadata.resource_base = SkillResourceBase::Directory {
                        path: metadata.path.clone(),
                    };
                    let content = std::fs::read_to_string(&skill_file).map_err(|error| {
                        PureError::ConfigError(format!(
                            "failed to fingerprint skill {}: {error}",
                            skill_file.display()
                        ))
                    })?;
                    observation.candidates.push(SkillCandidate {
                        summary: SkillSummary::from(metadata.clone()),
                        locator: metadata.path.to_string_lossy().to_string(),
                        revision: content_revision(&content),
                        rank: source.priority.into(),
                        local_order,
                    });
                    local_order += 1;
                }
                Err(error) => {
                    let warning = error.to_string();
                    if warning.contains("failed to read skill ") {
                        observation.complete = false;
                    }
                    observation.warnings.push(warning);
                }
            }
        }
    }
    Ok(observation)
}

fn content_revision(content: &str) -> String {
    format!("{:x}", Sha256::digest(content.as_bytes()))
}
