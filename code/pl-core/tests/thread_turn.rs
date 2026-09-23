mod support;
#[path = "support/tool.rs"]
mod tool_support;

use std::sync::{Arc, Mutex};

use pl_core::context::{ContextContent, ContextSource};
use pl_core::model::DynModelSession;
use pl_core::thread::{ThreadHandle, TurnOutcome, TurnState};

use support::{ScriptedModel, turn};
use tool_support::tool;

#[tokio::test]
async fn a_turn_commits_user_model_and_tool_facts_in_call_order() {
    let (model, requests) = ScriptedModel::new(&["first", "second"]);
    let thread = ThreadHandle::start("conversation".into(), DynModelSession::new(model)).unwrap();
    let log = Arc::new(Mutex::new(Vec::new()));
    thread
        .register_tools(vec![tool("first", &log), tool("second", &log)])
        .await
        .unwrap();

    let completion = thread.run_turn(turn("question")).await.unwrap();
    assert_eq!(completion.outcome, TurnOutcome::Completed);
    assert_eq!(completion.model_steps, 2);
    assert_eq!(
        *log.lock().unwrap(),
        ["question-first".to_string(), "question-second".to_string()]
    );
    {
        let seen = requests.lock().unwrap();
        assert_eq!(seen.len(), 2);
        let results: Vec<_> = seen[1]
            .records
            .iter()
            .filter_map(|record| match &record.source {
                ContextSource::ToolResult { call_id, .. } => Some(call_id.as_str()),
                _ => None,
            })
            .collect();
        assert_eq!(results, ["question-first", "question-second"]);
        let calls: Vec<_> = seen[1]
            .records
            .iter()
            .flat_map(|record| record.tool_calls.iter())
            .map(|call| call.arguments.content())
            .collect();
        assert_eq!(calls, ["  raw first\n", "  raw second\n"]);
    }

    let deliveries: Vec<_> = thread
        .effects()
        .await
        .unwrap()
        .into_iter()
        .flat_map(|effect| effect.deliveries.to_vec())
        .collect();
    assert_eq!(deliveries.len(), 2);
    assert_eq!(deliveries[0].output.payload().content(), "  raw first\n");
    assert_eq!(deliveries[1].output.payload().content(), "  raw second\n");
    assert!(thread.snapshot().context.records.iter().any(|record| {
        record.source == ContextSource::Assistant
            && record.content.contains(&ContextContent::Text {
                text: Arc::from("Answer completed"),
            })
    }));
    assert!(thread.effects().await.unwrap().iter().any(|effect| {
        effect.turn.as_ref().is_some_and(|turn| {
            turn.turn_id == "question" && turn.state == TurnState::Finished(TurnOutcome::Completed)
        })
    }));
    thread.close().await.unwrap();
}
