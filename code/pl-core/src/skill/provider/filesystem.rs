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
use crate::skill::{SkillMetadata, SkillResourceBase, SkillSourceKind, SkillSummary};

const FILESYSTEM_PROVIDER_ID: &str = "local-filesystem";

/// Local directory provider preserving Pure's on-disk layout and path-safety rules.
#[derive(Debug)]
pub struct FileSystemSkillProvider {
    id: SkillProviderId,
    sources: FileSystemSkillSources,
}

#[derive(Debug)]
enum FileSystemSkillSources {
    Configured,
    Explicit(Vec<SkillDirectorySource>),
}

/// 宿主显式注册的只读 Skill 目录来源。
///
/// 目录按传入顺序决定同名 Skill 的优先级，靠前来源优先。每个来源仍由 PL 统一负责
/// SKILL.md 校验、平台过滤、revision 冻结和支持资源路径安全。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SkillDirectorySource {
    pub root: PathBuf,
    pub source: crate::skill::SkillSourceKind,
}

impl SkillDirectorySource {
    pub fn new(root: impl Into<PathBuf>, source: crate::skill::SkillSourceKind) -> Self {
        Self {
            root: root.into(),
            source,
        }
    }
}

impl FileSystemSkillProvider {
    /// Creates the built-in local filesystem Provider.
    pub fn new() -> Self {
        Self {
            id: SkillProviderId(FILESYSTEM_PROVIDER_ID.to_string()),
            sources: FileSystemSkillSources::Configured,
        }
    }

    /// 创建一个使用宿主显式目录集合的 Provider。
    ///
    /// 该模式不读取 `SkillProviderRequest` 中的默认 project/user/system 路径，只复用其中
    /// 的 enable/disable 配置与取消令牌。
    pub fn from_directories(
        id: impl Into<String>,
        sources: Vec<SkillDirectorySource>,
    ) -> Result<Self> {
        Ok(Self {
            id: SkillProviderId::new(id)?,
            sources: FileSystemSkillSources::Explicit(sources),
        })
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
        let sources = match &self.sources {
            FileSystemSkillSources::Configured => None,
            FileSystemSkillSources::Explicit(sources) => Some(sources.clone()),
        };
        Box::pin(async move {
            tokio::task::spawn_blocking(move || list_local_skills(&id, request, sources))
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

    fn record_model_view<'a>(
        &'a self,
        candidate: &'a SkillCandidate,
        cancellation: CancellationToken,
    ) -> Pin<Box<dyn Future<Output = Result<()>> + Send + 'a>> {
        let records_project_usage = matches!(self.sources, FileSystemSkillSources::Configured)
            && candidate.summary.source == SkillSourceKind::Project;
        let candidate = candidate.clone();
        Box::pin(async move {
            if !records_project_usage {
                return Ok(());
            }
            tokio::task::spawn_blocking(move || {
                if cancellation.is_cancelled() {
                    return Err(cancelled_error());
                }
                let skill = SkillMetadata::from(candidate.summary);
                let trusted_skill_dir = skill.path.clone();
                crate::skill::bump_project_view(&trusted_skill_dir, &skill)
            })
            .await
            .map_err(|error| {
                PureError::ConfigError(format!("filesystem skill usage task failed: {error}"))
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
    explicit_sources: Option<Vec<SkillDirectorySource>>,
) -> Result<SkillProviderObservation> {
    if request.cancellation.is_cancelled() {
        return Err(cancelled_error());
    }
    let sources = if let Some(sources) = explicit_sources {
        sources
            .into_iter()
            .enumerate()
            .map(|(rank, source)| (source.root, source.source, rank as u16))
            .collect::<Vec<_>>()
    } else {
        let agents_user_dir = crate::skill::util::agents_user_skills_dir().ok();
        crate::skill::catalog::skill_sources(
            &request.workspace_root,
            &request.config,
            request.system_dir.as_deref(),
            agents_user_dir.as_deref(),
        )?
        .into_iter()
        .map(|source| (source.root, source.kind, source.priority.into()))
        .collect::<Vec<_>>()
    };
    let disabled = request
        .config
        .disabled
        .iter()
        .map(|name| name.to_ascii_lowercase())
        .collect::<BTreeSet<_>>();
    let mut observation = SkillProviderObservation::empty();
    let mut local_order = 0;
    for (root, source_kind, rank) in sources {
        if source_kind == SkillSourceKind::System
            && !request.config.system.enabled
            && provider_id.as_str() != super::BUILTIN_MODE_PROVIDER_ID
        {
            continue;
        }
        if request.cancellation.is_cancelled() {
            return Err(cancelled_error());
        }
        let scan = scan_skill_files(&root);
        observation.complete &= scan.complete;
        observation.warnings.extend(scan.warnings);
        for skill_file in scan.files {
            if request.cancellation.is_cancelled() {
                return Err(cancelled_error());
            }
            match metadata_from_file(&skill_file, &root, source_kind) {
                Ok(mut metadata) => {
                    if disabled.contains(&metadata.name.to_ascii_lowercase())
                        && !(provider_id.as_str() == super::BUILTIN_MODE_PROVIDER_ID
                            && super::is_builtin_mode_id(&metadata.name))
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
                        rank,
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
