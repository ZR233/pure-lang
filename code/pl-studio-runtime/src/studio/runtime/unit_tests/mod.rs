use pl_model::{ModelInfo, ProviderEndpoint};

use super::*;
use crate::config::{
    ConfigPaths, ModelRouteConfig, ProviderId, ReasoningEffort, StudioConfig, StudioRole,
};
use crate::studio::task_coordinator::CreateTaskRun;
use crate::{
    ConfigStore, StudioHostKind, StudioMode, StudioRuntimeOptions, StudioRuntimeStateKind,
    StudioTaskState,
};

fn test_config(base_url: String) -> StudioConfig {
    let mut model = ModelInfo::fallback("local-responses");
    model.transport = pl_model::ModelTransportProfile::responses_http();
    model.parameters = vec![crate::ModelParameter {
        name: "effort".to_string(),
        label: None,
        candidates: vec!["none".to_string()],
        wire: std::collections::BTreeMap::new(),
    }];
    let info = ProviderEndpoint::openai(Some(base_url));
    let provider = crate::ProviderConfig::from_explicit_models(info, vec![model]);
    let provider_id = ProviderId::new("local").unwrap();
    let route = ModelRouteConfig {
        provider: provider_id.clone(),
        model: "local-responses".to_string(),
        effort: Some(ReasoningEffort::new("none")),
    };
    test_product_config(provider_id, provider, route)
}

fn test_product_config(
    provider_id: ProviderId,
    provider: crate::ProviderConfig,
    route: ModelRouteConfig,
) -> StudioConfig {
    let mut config = StudioConfig::default_config();
    config.models = crate::AgentModelConfig {
        providers: std::collections::BTreeMap::from([(provider_id, provider)]),
        routes: StudioRole::all()
            .into_iter()
            .map(|role| (role.id(), route.clone()))
            .collect(),
    };
    config
}

#[tokio::test]
async fn system_skills_are_rebuilt_and_discovered_from_studio_home_after_restart() {
    let root = tempfile::tempdir().unwrap();
    let home = root.path().join("home");
    let workspace = root.path().join("workspace");
    let user_skills = root.path().join("separate-user-skills");
    tokio::fs::create_dir_all(&workspace).await.unwrap();
    let config_store = ConfigStore::new(ConfigPaths::from_config_dir(&home));
    let mut config = test_config("http://127.0.0.1:9".to_string());
    config.skills.user_dir = user_skills.to_string_lossy().into_owned();
    config_store.save(&config).unwrap();
    let options = StudioRuntimeOptions {
        studio_home: Some(home.clone()),
        host: StudioHostKind::Test,
    };
    let system_dir = home.join("studio").join("skills").join(".system");

    let first = StudioRuntime::with_options(options.clone()).await.unwrap();
    first.start_runtime().await.unwrap();
    assert!(system_dir.join("studio-config").join("SKILL.md").is_file());
    let catalog = first
        .skills
        .discover("project", &workspace, &config.skills)
        .await
        .unwrap();
    assert_eq!(
        catalog
            .state
            .value()
            .unwrap()
            .catalog
            .find("studio-config")
            .unwrap()
            .source,
        pl_core::skill::SkillSourceKind::System
    );
    std::fs::write(system_dir.join("stale"), "remove on restart").unwrap();
    first.shutdown_runtime().await.unwrap();
    drop(first);

    let reopened = StudioRuntime::with_options(options).await.unwrap();
    reopened.start_runtime().await.unwrap();

    assert!(!system_dir.join("stale").exists());
    assert!(system_dir.join("skill-creator").join("SKILL.md").is_file());
    assert!(!user_skills.join(".system").exists());
    reopened.shutdown_runtime().await.unwrap();
}

#[tokio::test]
async fn unsafe_system_skills_target_prevents_runtime_from_becoming_ready() {
    let root = tempfile::tempdir().unwrap();
    let home = root.path().join("home");
    let system_dir = home.join("studio").join("skills").join(".system");
    std::fs::create_dir_all(system_dir.parent().unwrap()).unwrap();
    std::fs::write(&system_dir, "not a managed directory").unwrap();
    let runtime = StudioRuntime::with_options(StudioRuntimeOptions {
        studio_home: Some(home),
        host: StudioHostKind::Test,
    })
    .await
    .unwrap();

    let error = format!("{:#}", runtime.start_runtime().await.unwrap_err());

    assert!(error.contains("failed to remove system Skills"));
    assert_eq!(
        runtime.runtime_snapshot().await.unwrap().state.kind(),
        StudioRuntimeStateKind::Failed
    );
    assert_eq!(
        std::fs::read_to_string(system_dir).unwrap(),
        "not a managed directory"
    );
}

#[tokio::test]
async fn start_new_thread_accepts_first_prompt_before_publishing_thread() {
    let root = tempfile::tempdir().unwrap();
    let home = root.path().join("home");
    let workspace = root.path().join("workspace");
    let database = root.path().join("studio.sqlite");
    tokio::fs::create_dir_all(&workspace).await.unwrap();
    let config_store = ConfigStore::new(ConfigPaths::from_home(&home));
    config_store
        .save(&test_config("http://127.0.0.1:9".to_string()))
        .unwrap();
    let store = StudioStore::open(&database).await.unwrap();
    let runtime = StudioRuntime::new(store, config_store).unwrap();
    let project = runtime.open_project(&workspace).await.unwrap();

    let response = runtime
        .start_new_thread(StudioStartNewThreadRequest {
            project_id: project.id.clone(),
            title: "First prompt".to_string(),
            prompt: "Inspect the temporary project.".to_string(),
            attachment_ids: Vec::new(),
            mode: StudioMode::Simple,
            options: StudioSubmitPromptOptions::default(),
        })
        .await
        .unwrap();

    assert_eq!(response.thread.project_id, project.id);
    assert_eq!(response.thread.mode, StudioMode::Simple);
    assert_eq!(response.submission.thread_id, response.thread.id);
    assert!(
        runtime
            .agent_facility
            .product_events
            .thread_snapshot(&response.thread.id)
            .is_some(),
        "accepted Thread must become visible only after its first prompt is queued"
    );
    assert!(runtime.thread_snapshot(&response.thread.id).await.is_ok());

    runtime.shutdown_runtime().await.unwrap();
}

#[tokio::test]
async fn start_new_task_thread_uses_hot_root_before_write_behind_persists_it() {
    let root = tempfile::tempdir().unwrap();
    let home = root.path().join("home");
    let workspace = root.path().join("workspace");
    let database = root.path().join("studio.sqlite");
    tokio::fs::create_dir_all(&workspace).await.unwrap();
    let config_store = ConfigStore::new(ConfigPaths::from_home(&home));
    config_store
        .save(&test_config("http://127.0.0.1:9".to_string()))
        .unwrap();
    let store = StudioStore::open(&database).await.unwrap();
    let runtime = StudioRuntime::new(store, config_store).unwrap();
    let project = runtime.open_project(&workspace).await.unwrap();

    let response = runtime
        .start_new_thread(StudioStartNewThreadRequest {
            project_id: project.id.clone(),
            title: "First task prompt".to_string(),
            prompt: "Create notes.txt with the requested marker.".to_string(),
            attachment_ids: Vec::new(),
            mode: StudioMode::Task,
            options: StudioSubmitPromptOptions::default(),
        })
        .await
        .expect("Task creation must consume the canonical hot root Thread");

    assert_eq!(response.thread.project_id, project.id);
    assert_eq!(response.thread.mode, StudioMode::Task);
    let task = runtime
        .thread_task_view(&response.thread.id)
        .await
        .unwrap()
        .expect("accepted Task must own a hot aggregate");
    assert!(matches!(task.state, StudioTaskState::Planning(_)));

    runtime.shutdown_runtime().await.unwrap();
}

#[tokio::test]
async fn task_hot_record_is_committed_before_runtime_readiness_is_checked() {
    let root = tempfile::tempdir().unwrap();
    let home = root.path().join("home");
    let workspace = root.path().join("workspace");
    tokio::fs::create_dir_all(&workspace).await.unwrap();
    let config_store = ConfigStore::new(ConfigPaths::from_home(&home));
    config_store
        .save(&test_config("http://127.0.0.1:9".to_string()))
        .unwrap();
    let runtime = StudioRuntime::with_options(StudioRuntimeOptions {
        studio_home: Some(home),
        host: StudioHostKind::Test,
    })
    .await
    .unwrap();
    let project = runtime.open_project(&workspace).await.unwrap();
    let thread = runtime
        .create_thread(&project.id, "Persist before runtime")
        .await
        .unwrap();
    runtime
        .set_thread_mode(&thread.id, StudioMode::Task)
        .await
        .unwrap();

    let error = runtime
        .submit_prompt(StudioSubmitPromptRequest {
            thread_id: thread.id.clone(),
            prompt: "deliver the requested feature".to_string(),
            attachment_ids: Vec::new(),
            options: StudioSubmitPromptOptions::default(),
        })
        .await
        .unwrap_err();

    assert!(error.to_string().contains("runtime is not ready"));
    let task = runtime
        .thread_task_view(&thread.id)
        .await
        .unwrap()
        .expect("Task hot record must survive model/runtime readiness failure");
    assert!(matches!(task.state, StudioTaskState::Planning(_)));
    assert_eq!(task.revision, 0);
    assert_eq!(task.generation, 0);
}

#[tokio::test]
async fn cold_start_restores_plan_confirmation_after_loading_memory_directories() {
    let root = tempfile::tempdir().unwrap();
    let home = root.path().join("home");
    let workspace = root.path().join("workspace");
    tokio::fs::create_dir_all(&workspace).await.unwrap();
    let config_store = ConfigStore::new(ConfigPaths::from_home(&home));
    config_store
        .save(&test_config("http://127.0.0.1:9".to_string()))
        .unwrap();
    let options = StudioRuntimeOptions {
        studio_home: Some(home.clone()),
        host: StudioHostKind::Test,
    };
    let setup = StudioRuntime::with_options(options.clone()).await.unwrap();
    let project = setup.open_project(&workspace).await.unwrap();
    let thread = setup
        .create_thread(&project.id, "Pending plan confirmation")
        .await
        .unwrap();
    setup
        .set_thread_mode(&thread.id, StudioMode::Task)
        .await
        .unwrap();
    setup
        .store
        .create_task_run(CreateTaskRun {
            project_id: project.id,
            root_thread_id: thread.id.clone(),
            request: "deliver the requested feature".to_string(),
            workspace_root: workspace.to_string_lossy().into_owned(),
        })
        .await
        .unwrap();
    let (_, interaction) = setup
        .store
        .submit_task_plan(&thread.id, "implementation plan", "plan-call", 0, 0)
        .await
        .unwrap();
    drop(setup);

    let runtime = StudioRuntime::with_options(options).await.unwrap();
    let snapshot = runtime.start_runtime().await.unwrap();

    assert!(snapshot.state.is_ready());
    let task = runtime
        .thread_task_view(&thread.id)
        .await
        .unwrap()
        .expect("pending Task must be restored into TaskRuntime");
    assert!(matches!(
        task.state,
        StudioTaskState::PendingConfirmation(_)
    ));
    let thread_snapshot = runtime.thread_snapshot(&thread.id).await.unwrap();
    assert!(
        thread_snapshot
            .interactions
            .iter()
            .any(|candidate| { candidate.interaction_id == interaction.interaction_id })
    );

    runtime.shutdown_runtime().await.unwrap();
}

mod deepseek_cache;
mod openai_cache;

#[tokio::test]
async fn project_cleanup_cancels_pending_interaction_for_nonresident_thread() {
    let root = tempfile::tempdir().unwrap();
    let home = root.path().join("home");
    let workspace = root.path().join("workspace");
    tokio::fs::create_dir_all(&workspace).await.unwrap();
    let config_store = ConfigStore::new(ConfigPaths::from_home(&home));
    config_store.save(&StudioConfig::default_config()).unwrap();
    let store = StudioStore::open_memory().await.unwrap();
    let project = store.upsert_project(&workspace).await.unwrap();
    let thread = store
        .create_thread(
            &project.id,
            "Nonresident pending interaction",
            StudioMode::Simple,
        )
        .await
        .unwrap();
    let interaction = crate::InteractionRequest::user_input(
        "cleanup-pending",
        pl_protocol::InteractionScope {
            thread_id: thread.id.clone(),
            turn_id: "turn-cleanup-pending".to_string(),
            item_id: Some("item-cleanup-pending".to_string()),
            tool_id: Some("ask_user".to_string()),
            agent_path: Some(thread.agent_path.clone()),
        },
        Vec::new(),
        1,
    );
    store.upsert_interaction(&interaction).await.unwrap();
    let runtime = StudioRuntime::new(store.clone(), config_store).unwrap();
    let preview = runtime.preview_project_cleanup(&project.id).await.unwrap();

    runtime
        .cleanup_project(&project.id, &preview.expected_revision)
        .await
        .unwrap();

    // 目录事实经 write-behind 异步落库；断言前先排空 writer。
    runtime
        .persistence_repository()
        .await
        .unwrap()
        .writer()
        .flush()
        .await
        .unwrap();
    assert!(store.list_projects().await.unwrap().is_empty());
    assert!(
        store
            .list_threads_for_root(&thread.root_thread_id)
            .await
            .unwrap()
            .is_empty()
    );
    assert!(
        store
            .list_pending_interactions(&thread.id)
            .await
            .unwrap()
            .is_empty()
    );
    assert_eq!(
        store
            .read_interaction(&interaction.interaction_id)
            .await
            .unwrap()
            .unwrap()
            .status(),
        crate::InteractionStatus::Cancelled
    );
}

#[tokio::test]
async fn thread_directory_facts_survive_sqlite_write_failure_as_degraded() {
    let root = tempfile::tempdir().unwrap();
    let home = root.path().join("home");
    let workspace = root.path().join("workspace");
    tokio::fs::create_dir_all(&workspace).await.unwrap();
    let config_store = ConfigStore::new(ConfigPaths::from_home(&home));
    config_store
        .save(&test_config("http://127.0.0.1:9".to_string()))
        .unwrap();
    let runtime = StudioRuntime::with_options(StudioRuntimeOptions {
        studio_home: Some(home),
        host: StudioHostKind::Test,
    })
    .await
    .unwrap();
    let project = runtime.open_project(&workspace).await.unwrap();
    // 排空项目目录 delta，保证触发器只影响后续 Thread 落库。
    runtime
        .persistence_repository()
        .await
        .unwrap()
        .writer()
        .flush()
        .await
        .unwrap();
    sea_orm::ConnectionTrait::execute_unprepared(
        runtime.store.database(),
        "CREATE TRIGGER fail_thread_directory_insert \
             BEFORE INSERT ON threads \
             BEGIN SELECT RAISE(FAIL, 'forced directory insert failure'); END",
    )
    .await
    .unwrap();

    // 目录命令内存先行：SQLite 失败进入 Degraded，不回滚已发布的内存事实。
    let thread = runtime
        .create_thread(&project.id, "Degraded directory")
        .await
        .expect("directory command must not fail on SQLite errors");

    let hot = runtime
        .agent_facility
        .product_events
        .thread_snapshot(&thread.id)
        .expect("hot thread fact must survive");
    assert_eq!(hot.title, "Degraded directory");

    // 触发器产生的是确定性 FAIL（Blocked 而非 Degraded）；两者都必须
    // 表达"持久化不健康 + 新工作门禁关闭"，而不是命令回滚。
    let mut unhealthy = false;
    for _ in 0..100 {
        if !runtime
            .agent_facility
            .product_events
            .persistence_state()
            .state
            .accepts_new_work()
        {
            unhealthy = true;
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }
    assert!(
        unhealthy,
        "persistence must report an unhealthy state after SQLite failure"
    );
    // Degraded 暂停新生命周期：后续目录命令被新工作门禁拒绝。
    assert!(
        runtime
            .create_thread(&project.id, "Blocked by gate")
            .await
            .is_err()
    );
}
