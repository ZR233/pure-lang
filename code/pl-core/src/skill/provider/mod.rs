use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, RwLock, Weak};

use futures::future::join_all;
use pl_protocol::{PureError, Result};
use pl_protocol::{SkillActivation, SkillActivationCause, SkillActivationResourceBase};
use serde::{Deserialize, Serialize};
use tokio_util::sync::CancellationToken;

use super::util::expand_home;
use super::{SkillCatalog, SkillMetadata, SkillSourceKind};
use crate::config::SkillsConfig;

mod filesystem;

pub use filesystem::FileSystemSkillProvider;

/// Stable identity of a process-registered Skill provider.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[serde(transparent)]
pub struct SkillProviderId(String);

impl SkillProviderId {
    /// Creates a non-empty Provider identity.
    ///
    /// # Errors
    ///
    /// Returns an error when the supplied identity is empty or whitespace-only.
    pub fn new(value: impl Into<String>) -> Result<Self> {
        let value = value.into();
        if value.trim().is_empty() {
            return Err(PureError::ConfigError(
                "skill provider id must not be empty".to_string(),
            ));
        }
        Ok(Self(value))
    }

    /// Returns the stable wire representation of this identity.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Independent model and direct-user invocation permissions.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SkillInvocationPolicy {
    pub model_invocable: bool,
    pub user_invocable: bool,
}

impl Default for SkillInvocationPolicy {
    fn default() -> Self {
        Self {
            model_invocable: true,
            user_invocable: true,
        }
    }
}

/// Provider-neutral base used to resolve a Skill's support resources.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", tag = "kind")]
pub enum SkillResourceBase {
    Directory { path: PathBuf },
    Url { url: String },
    Opaque { description: String },
}

/// Serializable catalog row exposed to prompt, Studio and HTTP projections.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SkillSummary {
    pub name: String,
    pub description: String,
    pub category: Option<String>,
    pub platforms: Vec<String>,
    pub source: SkillSourceKind,
    pub provider_id: SkillProviderId,
    pub invocation: SkillInvocationPolicy,
    pub resource_base: SkillResourceBase,
}

/// Provider-owned discovery candidate. Its locator is never serialized.
#[derive(Debug, Clone)]
pub struct SkillCandidate {
    pub summary: SkillSummary,
    pub locator: String,
    pub revision: String,
    pub rank: u16,
    pub local_order: usize,
}

/// A freshly loaded Skill main document.
#[derive(Debug, Clone)]
pub struct SkillDefinition {
    pub summary: SkillSummary,
    pub revision: String,
    pub content: String,
}

/// Result of a provider list operation, including whether its view was complete.
#[derive(Debug, Clone)]
pub struct SkillProviderObservation {
    pub candidates: Vec<SkillCandidate>,
    pub complete: bool,
    pub warnings: Vec<String>,
}

impl SkillProviderObservation {
    fn empty() -> Self {
        Self {
            candidates: Vec::new(),
            complete: true,
            warnings: Vec::new(),
        }
    }
}

/// Owned context supplied to provider discovery.
#[derive(Debug, Clone)]
pub struct SkillProviderRequest {
    pub workspace_root: PathBuf,
    pub config: SkillsConfig,
    pub system_dir: Option<PathBuf>,
    pub cancellation: CancellationToken,
}

/// Dynamically registered source of Skill summaries, documents and resources.
pub trait SkillProvider: Send + Sync + fmt::Debug {
    /// Returns the unique process-level Provider identity.
    fn id(&self) -> &SkillProviderId;

    /// Lists Provider-owned candidates for one workspace observation.
    fn list<'a>(
        &'a self,
        request: SkillProviderRequest,
    ) -> Pin<Box<dyn Future<Output = Result<SkillProviderObservation>> + Send + 'a>>;

    /// Loads the current main document for a frozen candidate.
    fn load<'a>(
        &'a self,
        candidate: &'a SkillCandidate,
        cancellation: CancellationToken,
    ) -> Pin<Box<dyn Future<Output = Result<SkillDefinition>> + Send + 'a>>;

    /// Reads one allowlisted support resource for a frozen candidate.
    fn read_resource<'a>(
        &'a self,
        candidate: &'a SkillCandidate,
        relative_path: &'a str,
        cancellation: CancellationToken,
    ) -> Pin<Box<dyn Future<Output = Result<String>> + Send + 'a>>;
}

#[derive(Debug)]
struct RegisteredProvider {
    registration: u64,
    provider: Arc<dyn SkillProvider>,
}

#[derive(Debug, Default)]
struct SkillRegistryInner {
    providers: RwLock<Vec<RegisteredProvider>>,
    generation: AtomicU64,
    next_registration: AtomicU64,
}

/// Process-level dynamic provider registry with monotonic invalidation.
#[derive(Debug, Clone, Default)]
pub struct SkillRegistry {
    inner: Arc<SkillRegistryInner>,
}

/// Dropping this guard releases the registered provider.
#[derive(Debug)]
pub struct SkillProviderRegistration {
    registry: Weak<SkillRegistryInner>,
    registration: u64,
}

/// A cloneable invalidator that makes subsequent discovery observe a new generation.
#[derive(Debug, Clone)]
pub struct SkillProviderInvalidator {
    registry: Weak<SkillRegistryInner>,
}

impl SkillRegistry {
    /// Creates an empty process-level registry.
    pub fn new() -> Self {
        Self::default()
    }

    /// Registers a unique Provider ID for the lifetime of the returned guard.
    ///
    /// # Errors
    ///
    /// Returns an error for duplicate Provider IDs or a poisoned registry lock.
    pub fn register(&self, provider: Arc<dyn SkillProvider>) -> Result<SkillProviderRegistration> {
        let mut providers = self.inner.providers.write().map_err(|_| {
            PureError::ConfigError("skill provider registry lock poisoned".to_string())
        })?;
        if providers
            .iter()
            .any(|registered| registered.provider.id() == provider.id())
        {
            return Err(PureError::ConfigError(format!(
                "duplicate skill provider id: {}",
                provider.id().as_str()
            )));
        }
        let registration = self.inner.next_registration.fetch_add(1, Ordering::Relaxed);
        providers.push(RegisteredProvider {
            registration,
            provider,
        });
        drop(providers);
        self.invalidate();
        Ok(SkillProviderRegistration {
            registry: Arc::downgrade(&self.inner),
            registration,
        })
    }

    /// Returns an invalidator tied to this registry's lifetime.
    pub fn invalidator(&self) -> SkillProviderInvalidator {
        SkillProviderInvalidator {
            registry: Arc::downgrade(&self.inner),
        }
    }

    /// Advances the registry generation for subsequent discoveries.
    pub fn invalidate(&self) {
        self.inner.generation.fetch_add(1, Ordering::AcqRel);
    }

    /// Discovers providers in parallel and retries once if registrations change.
    ///
    /// # Errors
    ///
    /// Returns an error when discovery is cancelled or its workspace/configuration is invalid.
    pub async fn discover(&self, request: SkillProviderRequest) -> Result<FrozenSkillCatalog> {
        let first = self.discover_generation(request.clone()).await?;
        if first.generation == self.inner.generation.load(Ordering::Acquire) {
            return Ok(first.catalog);
        }
        let mut second = self.discover_generation(request).await?;
        if second.generation != self.inner.generation.load(Ordering::Acquire) {
            second.catalog.snapshot.complete = false;
            second
                .catalog
                .snapshot
                .warnings
                .push("skill provider registry changed repeatedly during discovery".to_string());
        }
        Ok(second.catalog)
    }

    async fn discover_generation(
        &self,
        request: SkillProviderRequest,
    ) -> Result<DiscoveryGeneration> {
        if request.cancellation.is_cancelled() {
            return Err(cancelled_error());
        }
        let generation = self.inner.generation.load(Ordering::Acquire);
        let providers = self
            .inner
            .providers
            .read()
            .map_err(|_| {
                PureError::ConfigError("skill provider registry lock poisoned".to_string())
            })?
            .iter()
            .map(|registered| registered.provider.clone())
            .collect::<Vec<_>>();
        let observations = join_all(providers.iter().map(|provider| {
            let request = request.clone();
            async move {
                let result = provider.list(request).await;
                (provider.clone(), result)
            }
        }))
        .await;
        if request.cancellation.is_cancelled() {
            return Err(cancelled_error());
        }
        let project_dir = super::project_skills_dir(&request.workspace_root, &request.config)?;
        let mut warnings = Vec::new();
        let mut complete = true;
        let mut winners: BTreeMap<String, FrozenSkillEntry> = BTreeMap::new();
        for (provider_order, (provider, observation)) in observations.into_iter().enumerate() {
            let observation = match observation {
                Ok(observation) => observation,
                Err(error) => {
                    complete = false;
                    warnings.push(format!(
                        "skill provider {} failed: {error}",
                        provider.id().as_str()
                    ));
                    SkillProviderObservation::empty()
                }
            };
            complete &= observation.complete;
            warnings.extend(observation.warnings);
            for candidate in observation.candidates {
                if let Err(error) = validate_provider_candidate(provider.id(), &candidate) {
                    warnings.push(format!(
                        "skill provider {} returned an invalid candidate: {error}",
                        provider.id().as_str()
                    ));
                    continue;
                }
                let key = candidate.summary.name.to_ascii_lowercase();
                let ordering = (candidate.rank, provider_order, candidate.local_order);
                let replace = winners
                    .get(&key)
                    .is_none_or(|existing| ordering < existing.ordering);
                if replace {
                    winners.insert(
                        key,
                        FrozenSkillEntry {
                            ordering,
                            candidate,
                            provider: provider.clone(),
                        },
                    );
                }
            }
        }
        let skills = winners
            .values()
            .map(|entry| SkillMetadata::from(entry.candidate.summary.clone()))
            .collect();
        Ok(DiscoveryGeneration {
            generation,
            catalog: FrozenSkillCatalog {
                snapshot: SkillCatalog {
                    project_dir,
                    skills,
                    warnings,
                    complete,
                },
                entries: winners,
                invalidator: self.invalidator(),
            },
        })
    }
}

impl Drop for SkillProviderRegistration {
    fn drop(&mut self) {
        let Some(registry) = self.registry.upgrade() else {
            return;
        };
        if let Ok(mut providers) = registry.providers.write() {
            providers.retain(|provider| provider.registration != self.registration);
            registry.generation.fetch_add(1, Ordering::AcqRel);
        }
    }
}

impl SkillProviderInvalidator {
    /// Advances the owning registry generation when it is still alive.
    pub fn invalidate(&self) {
        if let Some(registry) = self.registry.upgrade() {
            registry.generation.fetch_add(1, Ordering::AcqRel);
        }
    }
}

struct DiscoveryGeneration {
    generation: u64,
    catalog: FrozenSkillCatalog,
}

#[derive(Clone)]
struct FrozenSkillEntry {
    ordering: (u16, usize, usize),
    candidate: SkillCandidate,
    provider: Arc<dyn SkillProvider>,
}

/// Turn-local immutable catalog retaining Provider locators and revisions.
#[derive(Clone)]
pub struct FrozenSkillCatalog {
    snapshot: SkillCatalog,
    entries: BTreeMap<String, FrozenSkillEntry>,
    invalidator: SkillProviderInvalidator,
}

impl fmt::Debug for FrozenSkillCatalog {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("FrozenSkillCatalog")
            .field("snapshot", &self.snapshot)
            .finish_non_exhaustive()
    }
}

impl PartialEq for FrozenSkillCatalog {
    fn eq(&self, other: &Self) -> bool {
        self.snapshot == other.snapshot
    }
}

impl Eq for FrozenSkillCatalog {}

impl FrozenSkillCatalog {
    /// Creates an empty complete catalog for a validated project directory.
    pub fn empty(project_dir: PathBuf) -> Self {
        let registry = SkillRegistry::new();
        Self {
            snapshot: SkillCatalog {
                project_dir,
                skills: Vec::new(),
                warnings: Vec::new(),
                complete: true,
            },
            entries: BTreeMap::new(),
            invalidator: registry.invalidator(),
        }
    }

    /// Returns the serializable catalog projection.
    pub fn snapshot(&self) -> &SkillCatalog {
        &self.snapshot
    }

    /// Invalidates the Provider registry that produced this catalog.
    pub fn invalidate(&self) {
        self.invalidator.invalidate();
    }

    /// Finds a winning Skill by case-insensitive name.
    pub fn find(&self, name: &str) -> Option<&SkillMetadata> {
        self.snapshot.find(name)
    }

    /// Finds a winning project-owned Skill by case-insensitive name.
    pub fn project_skill(&self, name: &str) -> Option<&SkillMetadata> {
        self.snapshot.project_skill(name)
    }

    /// Reloads a frozen candidate and revalidates its identity and invocation policy.
    ///
    /// # Errors
    ///
    /// Returns an error for missing, disallowed, changed, cancelled, or unloadable Skills.
    pub async fn load(
        &self,
        name: &str,
        invocation: SkillLoadInvocation,
        cancellation: CancellationToken,
    ) -> Result<SkillDefinition> {
        let key = name.to_ascii_lowercase();
        let entry = self.entries.get(&key).ok_or_else(|| {
            PureError::ConfigError(format!("skill not found in frozen catalog: {name}"))
        })?;
        ensure_invocation_allowed(&entry.candidate.summary, invocation)?;
        let loaded = entry.provider.load(&entry.candidate, cancellation).await?;
        if !loaded.summary.name.eq_ignore_ascii_case(name)
            || loaded.summary.invocation != entry.candidate.summary.invocation
        {
            self.invalidator.invalidate();
            return Err(PureError::ConfigError(format!(
                "skill identity or invocation policy changed since discovery: {name}"
            )));
        }
        ensure_invocation_allowed(&loaded.summary, invocation)?;
        Ok(loaded)
    }

    /// Reads one Provider-owned support resource under the frozen candidate.
    ///
    /// # Errors
    ///
    /// Returns an error for missing/disallowed Skills, unsafe paths, cancellation, or I/O failure.
    pub async fn read_resource(
        &self,
        name: &str,
        relative_path: &str,
        invocation: SkillLoadInvocation,
        cancellation: CancellationToken,
    ) -> Result<String> {
        let key = name.to_ascii_lowercase();
        let entry = self.entries.get(&key).ok_or_else(|| {
            PureError::ConfigError(format!("skill not found in frozen catalog: {name}"))
        })?;
        ensure_invocation_allowed(&entry.candidate.summary, invocation)?;
        entry
            .provider
            .read_resource(&entry.candidate, relative_path, cancellation)
            .await
    }

    /// Resolves exact whitespace-delimited `/name` gestures in first-seen order.
    ///
    /// # Errors
    ///
    /// Returns an error when a confirmed user-invocable Skill cannot be loaded safely.
    pub async fn load_user_invocations(
        &self,
        input: &str,
        turn_id: &str,
        cancellation: CancellationToken,
    ) -> Result<SkillUserInvocationLoad> {
        let mut seen = BTreeSet::new();
        let mut definitions = Vec::new();
        let mut activations = Vec::new();
        for token in input.split_whitespace() {
            let Some(name) = token.strip_prefix('/') else {
                continue;
            };
            if super::validate_skill_name(name).is_err() {
                continue;
            }
            let Some(skill) = self.find(name) else {
                continue;
            };
            if !skill.invocation.user_invocable {
                continue;
            }
            let key = skill.name.to_ascii_lowercase();
            if !seen.insert(key) {
                continue;
            }
            let definition = self
                .load(&skill.name, SkillLoadInvocation::User, cancellation.clone())
                .await?;
            let invocation_id = format!("user-skill-{}", activations.len());
            activations.push(SkillActivation {
                name: definition.summary.name.clone(),
                source: source_label(definition.summary.source).to_string(),
                provider_id: definition.summary.provider_id.as_str().to_string(),
                resource_base: activation_resource_base(&definition.summary.resource_base),
                turn_id: turn_id.to_string(),
                cause: SkillActivationCause::UserGesture { invocation_id },
                activated_at: crate::time::unix_seconds(),
            });
            definitions.push(definition);
        }
        let instruction = (!definitions.is_empty()).then(|| {
            let mut content = String::from(
                "The user directly loaded the following Skills. Follow them as turn-level user instructions and do not call `skill_view` for these Skills again in this turn.\n\n",
            );
            for definition in definitions {
                content.push_str(&format!(
                    "<skill_content name=\"{}\">\n{}\n</skill_content>\n\n",
                    definition.summary.name, definition.content
                ));
            }
            content.trim_end().to_string()
        });
        Ok(SkillUserInvocationLoad {
            instruction,
            activations,
        })
    }
}

/// Turn preparation output for direct user Skill gestures.
#[derive(Debug, Clone, Default)]
pub struct SkillUserInvocationLoad {
    pub instruction: Option<String>,
    pub activations: Vec<SkillActivation>,
}

/// Invocation channel checked again when loading a frozen candidate.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SkillLoadInvocation {
    Model,
    User,
}

fn ensure_invocation_allowed(
    summary: &SkillSummary,
    invocation: SkillLoadInvocation,
) -> Result<()> {
    let allowed = match invocation {
        SkillLoadInvocation::Model => summary.invocation.model_invocable,
        SkillLoadInvocation::User => summary.invocation.user_invocable,
    };
    if allowed {
        Ok(())
    } else {
        Err(PureError::ConfigError(format!(
            "skill '{}' is not invocable from {invocation:?}",
            summary.name
        )))
    }
}

fn validate_provider_candidate(
    provider_id: &SkillProviderId,
    candidate: &SkillCandidate,
) -> Result<()> {
    if &candidate.summary.provider_id != provider_id {
        return Err(PureError::ConfigError(format!(
            "candidate provider id '{}' does not match registered provider '{}'",
            candidate.summary.provider_id.as_str(),
            provider_id.as_str()
        )));
    }
    super::validate_skill_name(&candidate.summary.name)?;
    let description = candidate.summary.description.trim();
    if description.is_empty() || description.chars().count() > 1024 {
        return Err(PureError::ConfigError(
            "candidate description must contain 1 to 1024 characters".to_string(),
        ));
    }
    if candidate.locator.trim().is_empty() {
        return Err(PureError::ConfigError(
            "candidate locator must not be empty".to_string(),
        ));
    }
    if candidate.revision.trim().is_empty() {
        return Err(PureError::ConfigError(
            "candidate revision must not be empty".to_string(),
        ));
    }
    let resource_is_empty = match &candidate.summary.resource_base {
        SkillResourceBase::Directory { path } => path.as_os_str().is_empty(),
        SkillResourceBase::Url { url } => url.trim().is_empty(),
        SkillResourceBase::Opaque { description } => description.trim().is_empty(),
    };
    if resource_is_empty {
        return Err(PureError::ConfigError(
            "candidate resource base must not be empty".to_string(),
        ));
    }
    Ok(())
}

fn source_label(source: SkillSourceKind) -> &'static str {
    match source {
        SkillSourceKind::Project => "project",
        SkillSourceKind::User => "user",
        SkillSourceKind::System => "system",
        SkillSourceKind::External => "external",
    }
}

fn activation_resource_base(base: &SkillResourceBase) -> SkillActivationResourceBase {
    match base {
        SkillResourceBase::Directory { path } => SkillActivationResourceBase::Directory {
            path: path.to_string_lossy().to_string(),
        },
        SkillResourceBase::Url { url } => SkillActivationResourceBase::Url { url: url.clone() },
        SkillResourceBase::Opaque { description } => SkillActivationResourceBase::Opaque {
            description: description.clone(),
        },
    }
}

/// Builds the default local registry and returns the guard that owns its provider.
///
/// # Errors
///
/// Returns an error if the built-in Provider cannot be registered.
pub fn local_skill_registry() -> Result<(SkillRegistry, SkillProviderRegistration)> {
    let registry = SkillRegistry::new();
    let registration = registry.register(Arc::new(FileSystemSkillProvider::new()))?;
    Ok((registry, registration))
}

/// Convenience discovery for product-neutral callers without a process registry.
///
/// # Errors
///
/// Returns an error when local discovery is cancelled or its paths/configuration are invalid.
pub async fn discover_local_skills(
    workspace_root: &Path,
    config: &SkillsConfig,
    system_dir: Option<&Path>,
    cancellation: CancellationToken,
) -> Result<FrozenSkillCatalog> {
    let (registry, _registration) = local_skill_registry()?;
    registry
        .discover(SkillProviderRequest {
            workspace_root: workspace_root.to_path_buf(),
            config: config.clone(),
            system_dir: system_dir.map(Path::to_path_buf),
            cancellation,
        })
        .await
}

pub(super) fn external_source_root(path: &str) -> Result<PathBuf> {
    expand_home(path)
}

fn cancelled_error() -> PureError {
    PureError::ConfigError("skill provider operation cancelled".to_string())
}

#[cfg(test)]
#[path = "tests.rs"]
mod tests;
