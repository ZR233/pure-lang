use std::sync::atomic::{AtomicUsize, Ordering};

use pretty_assertions::assert_eq;

use super::*;

#[derive(Debug)]
struct TestProvider {
    id: SkillProviderId,
    description: String,
    candidate_provider_id: Option<SkillProviderId>,
    invalidator: Option<SkillProviderInvalidator>,
    invalidate_every_time: bool,
    calls: AtomicUsize,
}

impl TestProvider {
    fn new(id: &str, description: &str) -> Self {
        Self {
            id: SkillProviderId::new(id).unwrap(),
            description: description.to_string(),
            candidate_provider_id: None,
            invalidator: None,
            invalidate_every_time: false,
            calls: AtomicUsize::new(0),
        }
    }

    fn invalidating(mut self, invalidator: SkillProviderInvalidator, every: bool) -> Self {
        self.invalidator = Some(invalidator);
        self.invalidate_every_time = every;
        self
    }

    fn with_candidate_provider_id(mut self, provider_id: &str) -> Self {
        self.candidate_provider_id = Some(SkillProviderId::new(provider_id).unwrap());
        self
    }
}

impl SkillProvider for TestProvider {
    fn id(&self) -> &SkillProviderId {
        &self.id
    }

    fn list<'a>(
        &'a self,
        _request: SkillProviderRequest,
    ) -> Pin<Box<dyn Future<Output = Result<SkillProviderObservation>> + Send + 'a>> {
        Box::pin(async move {
            let call = self.calls.fetch_add(1, Ordering::SeqCst);
            if (self.invalidate_every_time || call == 0)
                && let Some(invalidator) = &self.invalidator
            {
                invalidator.invalidate();
            }
            Ok(SkillProviderObservation {
                candidates: vec![candidate(
                    self.candidate_provider_id
                        .clone()
                        .unwrap_or_else(|| self.id.clone()),
                    &self.description,
                    0,
                )],
                complete: true,
                warnings: Vec::new(),
            })
        })
    }

    fn load<'a>(
        &'a self,
        _candidate: &'a SkillCandidate,
        _cancellation: CancellationToken,
    ) -> Pin<Box<dyn Future<Output = Result<SkillDefinition>> + Send + 'a>> {
        Box::pin(async { Err(PureError::ConfigError("not loadable".to_string())) })
    }

    fn read_resource<'a>(
        &'a self,
        _candidate: &'a SkillCandidate,
        _relative_path: &'a str,
        _cancellation: CancellationToken,
    ) -> Pin<Box<dyn Future<Output = Result<String>> + Send + 'a>> {
        Box::pin(async { Err(PureError::ConfigError("not readable".to_string())) })
    }
}

fn candidate(provider_id: SkillProviderId, description: &str, rank: u16) -> SkillCandidate {
    SkillCandidate {
        summary: SkillSummary {
            name: "shared".to_string(),
            description: description.to_string(),
            category: None,
            platforms: Vec::new(),
            source: SkillSourceKind::Project,
            provider_id,
            invocation: SkillInvocationPolicy::default(),
            resource_base: SkillResourceBase::Opaque {
                description: "test".to_string(),
            },
        },
        locator: "opaque".to_string(),
        revision: "1".to_string(),
        rank,
        local_order: 0,
    }
}

fn request(root: &Path) -> SkillProviderRequest {
    SkillProviderRequest {
        workspace_root: root.to_path_buf(),
        config: SkillsConfig::default(),
        system_dir: None,
        cancellation: CancellationToken::new(),
    }
}

fn write_skill(root: &Path, name: &str, policy: &str, body: &str) {
    let directory = root.join("skills").join(name);
    std::fs::create_dir_all(&directory).unwrap();
    std::fs::write(
        directory.join(super::super::SKILL_FILE_NAME),
        format!("---\nname: {name}\ndescription: {name}\n{policy}---\n{body}\n"),
    )
    .unwrap();
}

#[test]
fn duplicate_provider_id_is_rejected_until_guard_is_released() {
    let registry = SkillRegistry::new();
    let first = registry
        .register(Arc::new(TestProvider::new("test", "first")))
        .unwrap();
    let error = registry
        .register(Arc::new(TestProvider::new("test", "second")))
        .unwrap_err();
    assert!(error.to_string().contains("duplicate skill provider id"));

    drop(first);
    registry
        .register(Arc::new(TestProvider::new("test", "second")))
        .unwrap();
}

#[tokio::test]
async fn provider_registration_order_breaks_equal_rank_ties() {
    let root = tempfile::tempdir().unwrap();
    let registry = SkillRegistry::new();
    let _first = registry
        .register(Arc::new(TestProvider::new("first", "winner")))
        .unwrap();
    let _second = registry
        .register(Arc::new(TestProvider::new("second", "loser")))
        .unwrap();

    let catalog = registry.discover(request(root.path())).await.unwrap();

    assert_eq!(catalog.snapshot().skills[0].description, "winner");
}

#[tokio::test]
async fn explicit_directories_are_frozen_by_the_shared_provider_kernel() {
    let source = tempfile::tempdir().unwrap();
    let unrelated_workspace = tempfile::tempdir().unwrap();
    write_skill(source.path(), "project-review", "", "review body");
    let provider = FileSystemSkillProvider::from_directories(
        "mai-project",
        vec![SkillDirectorySource::new(
            source.path().join("skills"),
            SkillSourceKind::Project,
        )],
    )
    .unwrap();
    let registry = SkillRegistry::new();
    let _guard = registry.register(Arc::new(provider)).unwrap();

    let catalog = registry
        .discover(request(unrelated_workspace.path()))
        .await
        .unwrap();
    let skill = catalog.find("project-review").expect("explicit skill");
    assert_eq!(skill.source, SkillSourceKind::Project);
    assert_eq!(skill.provider_id.as_str(), "mai-project");
    let loaded = catalog
        .load(
            "project-review",
            SkillLoadInvocation::Model,
            CancellationToken::new(),
        )
        .await
        .unwrap();
    assert!(loaded.content.contains("review body"));
}

#[tokio::test]
async fn explicit_directories_obey_shared_system_policy() {
    let source = tempfile::tempdir().unwrap();
    let workspace = tempfile::tempdir().unwrap();
    write_skill(source.path(), "system-review", "", "review body");
    let provider = FileSystemSkillProvider::from_directories(
        "mai-system",
        vec![SkillDirectorySource::new(
            source.path().join("skills"),
            SkillSourceKind::System,
        )],
    )
    .unwrap();
    let registry = SkillRegistry::new();
    let _guard = registry.register(Arc::new(provider)).unwrap();

    let mut disabled_system = request(workspace.path());
    disabled_system.config.system.enabled = false;
    assert!(
        registry
            .discover(disabled_system)
            .await
            .unwrap()
            .snapshot()
            .skills
            .is_empty()
    );
}

#[tokio::test]
async fn invalid_provider_candidates_are_warned_and_excluded() {
    let root = tempfile::tempdir().unwrap();
    let registry = SkillRegistry::new();
    let provider = Arc::new(
        TestProvider::new("registered", "valid description")
            .with_candidate_provider_id("different-provider"),
    );
    let _guard = registry.register(provider).unwrap();

    let catalog = registry.discover(request(root.path())).await.unwrap();

    assert!(catalog.snapshot().skills.is_empty());
    assert!(
        catalog
            .snapshot()
            .warnings
            .iter()
            .any(|warning| warning.contains("does not match registered provider"))
    );
}

#[tokio::test]
async fn cancelled_discovery_returns_cancellation_instead_of_partial_catalog() {
    let root = tempfile::tempdir().unwrap();
    let registry = SkillRegistry::new();
    let _guard = registry
        .register(Arc::new(TestProvider::new("test", "description")))
        .unwrap();
    let request = request(root.path());
    request.cancellation.cancel();

    let error = registry.discover(request).await.unwrap_err();

    assert!(error.to_string().contains("cancelled"));
}

#[tokio::test]
async fn discovery_retries_once_on_invalidation_and_then_marks_incomplete() {
    let root = tempfile::tempdir().unwrap();
    let registry = SkillRegistry::new();
    let retrying = Arc::new(
        TestProvider::new("retrying", "stable").invalidating(registry.invalidator(), false),
    );
    let guard = registry.register(retrying.clone()).unwrap();

    let stable = registry.discover(request(root.path())).await.unwrap();
    assert!(stable.snapshot().complete);
    assert_eq!(retrying.calls.load(Ordering::SeqCst), 2);

    drop(guard);
    let always = Arc::new(
        TestProvider::new("always", "unstable").invalidating(registry.invalidator(), true),
    );
    let _guard = registry.register(always.clone()).unwrap();
    let incomplete = registry.discover(request(root.path())).await.unwrap();
    assert!(!incomplete.snapshot().complete);
    assert!(
        incomplete
            .snapshot()
            .warnings
            .iter()
            .any(|warning| warning.contains("changed repeatedly"))
    );
}

#[tokio::test]
async fn user_gestures_honor_policy_boundaries_and_deduplicate() {
    let root = tempfile::tempdir().unwrap();
    write_skill(root.path(), "both", "", "both body");
    write_skill(
        root.path(),
        "model-only",
        "user-invocable: false\n",
        "model body",
    );
    write_skill(
        root.path(),
        "user-only",
        "disable-model-invocation: true\n",
        "user body",
    );
    let catalog = discover_local_skills(
        root.path(),
        &SkillsConfig::default(),
        None,
        CancellationToken::new(),
    )
    .await
    .unwrap();

    let loaded = catalog
        .load_user_invocations_with_selections(
            "$both /model-only /both /unknown /user-only.json /user-only",
            &["user-only".to_string()],
            "turn-1",
            CancellationToken::new(),
        )
        .await
        .unwrap();

    assert_eq!(
        loaded
            .activations
            .iter()
            .map(|activation| activation.name.as_str())
            .collect::<Vec<_>>(),
        ["user-only", "both"]
    );
    let instruction = loaded.instruction.unwrap();
    assert!(instruction.contains("both body"));
    assert!(instruction.contains("user body"));
    assert!(!instruction.contains("model body"));
}

#[tokio::test]
async fn load_reads_latest_body_but_rejects_changed_identity_or_policy() {
    let root = tempfile::tempdir().unwrap();
    write_skill(root.path(), "mutable", "", "first body");
    let catalog = discover_local_skills(
        root.path(),
        &SkillsConfig::default(),
        None,
        CancellationToken::new(),
    )
    .await
    .unwrap();
    write_skill(root.path(), "mutable", "", "second body");
    let latest = catalog
        .load(
            "mutable",
            SkillLoadInvocation::Model,
            CancellationToken::new(),
        )
        .await
        .unwrap();
    assert!(latest.content.contains("second body"));

    write_skill(
        root.path(),
        "mutable",
        "disable-model-invocation: true\n",
        "third body",
    );
    let error = catalog
        .load(
            "mutable",
            SkillLoadInvocation::Model,
            CancellationToken::new(),
        )
        .await
        .unwrap_err();
    assert!(
        error
            .to_string()
            .contains("identity or invocation policy changed")
    );
}
