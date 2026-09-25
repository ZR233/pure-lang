mod support;
#[path = "support/tool.rs"]
mod tool_support;

use std::sync::{Arc, Mutex};

use pl_core::context::{ContextContent, ContextSource, OpaquePayload};
use pl_core::model::DynModelSession;
use pl_core::thread::{
    ModelStepLimit, ThreadError, ThreadHandle, TurnOutcome, TurnState, inbox::ThreadMessage,
    input::InputDriverOptions,
};

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

#[tokio::test]
async fn a_durable_message_receipt_wakes_only_its_unconsumed_message() {
    let (model, requests) = ScriptedModel::new(&[]);
    let thread = ThreadHandle::start("parent".into(), DynModelSession::new(model)).unwrap();
    let message = ThreadMessage {
        id: "child-report".into(),
        source_id: "child".into(),
        payload: OpaquePayload::text("finished"),
        context: vec![support::text("finished")],
    };
    let sequence = thread.send_message(message).await.unwrap();
    let options = InputDriverOptions {
        max_model_steps: ModelStepLimit::Limited(2.try_into().unwrap()),
    };
    assert!(matches!(
        thread
            .wake_accepted_message("other", sequence, options)
            .await,
        Err(ThreadError::InvalidIdentity)
    ));
    assert_eq!(thread.snapshot().inbox.len(), 1);
    assert!(
        thread
            .wake_accepted_message("child-report", sequence, options)
            .await
            .unwrap()
    );
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        while thread.snapshot().consumed_messages < sequence {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    assert!(
        !thread
            .wake_accepted_message("child-report", sequence, options)
            .await
            .unwrap()
    );
    assert_eq!(thread.snapshot().inbox_sequence, sequence);
    {
        let requests = requests.lock().unwrap();
        assert_eq!(requests.len(), 1);
        assert!(requests[0].records.iter().any(|record| {
            record.source
                == ContextSource::Runtime {
                    source_id: "child".into(),
                }
        }));
    }
    thread.close().await.unwrap();
}
