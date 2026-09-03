use pl_core::{
    ThreadModeId, ThreadModeManager, ThreadModeRegistration, ThreadModeSource, ThreadModeSourceId,
    WorkflowDefinition, WorkflowState, WorkflowStateKind, WorkflowTransition,
};

#[test]
fn upstream_crate_registers_an_owned_mode_through_pl_core_only() {
    let manager = ThreadModeManager::default();
    let mode_id = ThreadModeId::new("mode.upstream-test").expect("valid mode id");
    let source = ThreadModeSource::external(
        ThreadModeSourceId::new("upstream.memory-loader").expect("valid source id"),
    );
    let graph = WorkflowDefinition {
        title: "Upstream delivery".to_string(),
        goal: "Prove the public registration boundary".to_string(),
        initial_state_id: "ready".to_string(),
        states: vec![
            WorkflowState {
                id: "ready".to_string(),
                title: "Ready".to_string(),
                instructions: "Prepare the delivery.".to_string(),
                completion_criteria: vec!["The delivery is prepared.".to_string()],
                kind: WorkflowStateKind::Atomic,
            },
            WorkflowState {
                id: "done".to_string(),
                title: "Done".to_string(),
                instructions: String::new(),
                completion_criteria: Vec::new(),
                kind: WorkflowStateKind::Final,
            },
        ],
        transitions: vec![WorkflowTransition {
            source_state_id: "ready".to_string(),
            target_state_id: "done".to_string(),
            guard: "The delivery is verified.".to_string(),
        }],
    };

    let snapshot = manager
        .replace_source(
            source,
            [ThreadModeRegistration {
                id: mode_id.clone(),
                display_name: "Upstream Test".to_string(),
                description: "Registered entirely through pl-core's public memory API.".to_string(),
                order: 50,
                prompt: "Follow the upstream mode prompt.".to_string(),
                workflow: Some(graph),
            }],
        )
        .expect("public registration succeeds");

    let registered = snapshot.mode(&mode_id).expect("registered mode");
    assert_eq!(registered.prompt(), "Follow the upstream mode prompt.");
    assert_eq!(registered.source().id.as_str(), "upstream.memory-loader");
    assert!(registered.workflow().is_some());
    assert_eq!(snapshot.catalog().modes[0].id, mode_id);
}
