use std::collections::HashMap;

use pl_protocol::{
    InteractionChangedEvent, InteractionKind, InteractionPayload, InteractionRequest,
    InteractionResolution, InteractionScope, InteractionStatus, StudioEventEnvelope,
    StudioEventKind, StudioMessage, StudioMessageRole, StudioMessageStatus, StudioPart,
    StudioPartDelta, StudioPartDeltaField, StudioPartStatus, StudioPartType, StudioTextChannel,
    StudioToolPart, ToolApprovalResolution, UserInputAnswer, UserQuestion, UserQuestionOption,
};
use pretty_assertions::assert_eq;

fn message(message_id: &str, role: StudioMessageRole, created_at: i64) -> StudioMessage {
    StudioMessage {
        message_id: message_id.to_string(),
        session_id: "session-golden".to_string(),
        turn_id: "turn-golden-1".to_string(),
        role,
        status: StudioMessageStatus::Streaming,
        created_at,
        updated_at: created_at,
        completed_at: None,
        error: None,
        metadata: serde_json::json!({}),
    }
}

fn text_part(
    part_id: &str,
    message_id: &str,
    order: u64,
    text_channel: StudioTextChannel,
) -> StudioPart {
    StudioPart {
        part_id: part_id.to_string(),
        message_id: message_id.to_string(),
        session_id: "session-golden".to_string(),
        turn_id: "turn-golden-1".to_string(),
        part_type: StudioPartType::Text,
        order,
        revision: 0,
        status: StudioPartStatus::Streaming,
        created_at: order as i64,
        updated_at: order as i64,
        completed_at: None,
        error: None,
        text_channel: Some(text_channel),
        text: String::new(),
        attachments: vec![],
        tool: None,
        agent: None,
        inference: None,
        plan: None,
        file: None,
        usage: None,
        synthetic: false,
        ignored: false,
    }
}

fn reasoning_part(part_id: &str, order: u64, text: &str, status: StudioPartStatus) -> StudioPart {
    StudioPart {
        part_id: part_id.to_string(),
        message_id: "turn-golden-1:assistant".to_string(),
        session_id: "session-golden".to_string(),
        turn_id: "turn-golden-1".to_string(),
        part_type: StudioPartType::Reasoning,
        order,
        revision: 0,
        status,
        created_at: order as i64,
        updated_at: order as i64,
        completed_at: None,
        error: None,
        text_channel: None,
        text: text.to_string(),
        attachments: vec![],
        tool: None,
        agent: None,
        inference: None,
        plan: None,
        file: None,
        usage: None,
        synthetic: false,
        ignored: false,
    }
}

fn tool_part(part_id: &str, status: StudioPartStatus) -> StudioPart {
    StudioPart {
        part_id: part_id.to_string(),
        message_id: "turn-golden-1:assistant".to_string(),
        session_id: "session-golden".to_string(),
        turn_id: "turn-golden-1".to_string(),
        part_type: StudioPartType::Tool,
        order: 4,
        revision: 0,
        status,
        created_at: 4,
        updated_at: 4,
        completed_at: None,
        error: None,
        text_channel: None,
        text: String::new(),
        attachments: vec![],
        tool: Some(StudioToolPart {
            tool_call_id: part_id.to_string(),
            call_id: None,
            provider_item_id: None,
            name: "bash".to_string(),
            arguments: r#"{"command":"pwd"}"#.to_string(),
            result: None,
            exit_code: None,
            timed_out: false,
            working_directory: Some("D:/repo".to_string()),
            denial_reason: None,
        }),
        agent: None,
        inference: None,
        plan: None,
        file: None,
        usage: None,
        synthetic: false,
        ignored: false,
    }
}

fn envelope(sequence: u64, kind: StudioEventKind) -> StudioEventEnvelope {
    StudioEventEnvelope {
        event_id: format!("event-{sequence}"),
        project_id: None,
        session_id: Some("session-golden".to_string()),
        turn_id: Some("turn-golden-1".to_string()),
        sequence,
        created_at: sequence as i64,
        kind,
    }
}

#[test]
fn opencode_timeline_events_keep_wire_shape_for_parts_and_deltas() {
    let events = vec![
        envelope(
            1,
            StudioEventKind::MessageUpdated {
                message: Box::new(message("turn-golden-1:user", StudioMessageRole::User, 1)),
            },
        ),
        envelope(
            2,
            StudioEventKind::MessagePartUpdated {
                part: Box::new(text_part(
                    "turn-golden-1:user:text",
                    "turn-golden-1:user",
                    1,
                    StudioTextChannel::User,
                )),
            },
        ),
        envelope(
            3,
            StudioEventKind::MessageUpdated {
                message: Box::new(message(
                    "turn-golden-1:assistant",
                    StudioMessageRole::Assistant,
                    2,
                )),
            },
        ),
        envelope(
            4,
            StudioEventKind::MessagePartUpdated {
                part: Box::new(reasoning_part(
                    "reason-a",
                    2,
                    "",
                    StudioPartStatus::Streaming,
                )),
            },
        ),
        envelope(
            5,
            StudioEventKind::MessagePartDelta {
                delta: StudioPartDelta {
                    part_id: "reason-a".to_string(),
                    revision: 1,
                    field: StudioPartDeltaField::ReasoningSummary,
                    delta: "thinking".to_string(),
                    chunk_index: Some(0),
                },
            },
        ),
        envelope(
            6,
            StudioEventKind::MessagePartUpdated {
                part: Box::new(tool_part("tool-call-1", StudioPartStatus::AwaitingApproval)),
            },
        ),
        envelope(
            7,
            StudioEventKind::MessagePartDelta {
                delta: StudioPartDelta {
                    part_id: "tool-call-1".to_string(),
                    revision: 1,
                    field: StudioPartDeltaField::ToolResult,
                    delta: "D:/repo\n".to_string(),
                    chunk_index: None,
                },
            },
        ),
    ];

    assert_eq!(
        serde_json::to_value(events).unwrap(),
        serde_json::json!([
            {
                "eventId": "event-1",
                "sessionId": "session-golden",
                "turnId": "turn-golden-1",
                "sequence": 1,
                "createdAt": 1,
                "kind": {
                    "type": "messageUpdated",
                    "message": {
                        "messageId": "turn-golden-1:user",
                        "sessionId": "session-golden",
                        "turnId": "turn-golden-1",
                        "role": "user",
                        "status": "streaming",
                        "createdAt": 1,
                        "updatedAt": 1
                    }
                }
            },
            {
                "eventId": "event-2",
                "sessionId": "session-golden",
                "turnId": "turn-golden-1",
                "sequence": 2,
                "createdAt": 2,
                "kind": {
                    "type": "messagePartUpdated",
                    "part": {
                        "partId": "turn-golden-1:user:text",
                        "messageId": "turn-golden-1:user",
                        "sessionId": "session-golden",
                        "turnId": "turn-golden-1",
                        "partType": "text",
                        "order": 1,
                        "revision": 0,
                        "status": "streaming",
                        "createdAt": 1,
                        "updatedAt": 1,
                        "textChannel": "user"
                    }
                }
            },
            {
                "eventId": "event-3",
                "sessionId": "session-golden",
                "turnId": "turn-golden-1",
                "sequence": 3,
                "createdAt": 3,
                "kind": {
                    "type": "messageUpdated",
                    "message": {
                        "messageId": "turn-golden-1:assistant",
                        "sessionId": "session-golden",
                        "turnId": "turn-golden-1",
                        "role": "assistant",
                        "status": "streaming",
                        "createdAt": 2,
                        "updatedAt": 2
                    }
                }
            },
            {
                "eventId": "event-4",
                "sessionId": "session-golden",
                "turnId": "turn-golden-1",
                "sequence": 4,
                "createdAt": 4,
                "kind": {
                    "type": "messagePartUpdated",
                    "part": {
                        "partId": "reason-a",
                        "messageId": "turn-golden-1:assistant",
                        "sessionId": "session-golden",
                        "turnId": "turn-golden-1",
                        "partType": "reasoning",
                        "order": 2,
                        "revision": 0,
                        "status": "streaming",
                        "createdAt": 2,
                        "updatedAt": 2
                    }
                }
            },
            {
                "eventId": "event-5",
                "sessionId": "session-golden",
                "turnId": "turn-golden-1",
                "sequence": 5,
                "createdAt": 5,
                "kind": {
                    "type": "messagePartDelta",
                    "delta": {
                        "partId": "reason-a",
                        "revision": 1,
                        "field": "reasoning.summary",
                        "delta": "thinking",
                        "chunkIndex": 0
                    }
                }
            },
            {
                "eventId": "event-6",
                "sessionId": "session-golden",
                "turnId": "turn-golden-1",
                "sequence": 6,
                "createdAt": 6,
                "kind": {
                    "type": "messagePartUpdated",
                    "part": {
                        "partId": "tool-call-1",
                        "messageId": "turn-golden-1:assistant",
                        "sessionId": "session-golden",
                        "turnId": "turn-golden-1",
                        "partType": "tool",
                        "order": 4,
                        "revision": 0,
                        "status": "awaitingApproval",
                        "createdAt": 4,
                        "updatedAt": 4,
                        "tool": {
                            "toolCallId": "tool-call-1",
                            "name": "bash",
                            "arguments": "{\"command\":\"pwd\"}",
                            "timedOut": false,
                            "workingDirectory": "D:/repo"
                        }
                    }
                }
            },
            {
                "eventId": "event-7",
                "sessionId": "session-golden",
                "turnId": "turn-golden-1",
                "sequence": 7,
                "createdAt": 7,
                "kind": {
                    "type": "messagePartDelta",
                    "delta": {
                        "partId": "tool-call-1",
                        "revision": 1,
                        "field": "tool.result",
                        "delta": "D:/repo\n"
                    }
                }
            }
        ])
    );
}

#[test]
fn user_input_interaction_fixture_covers_options_other_secret_and_resolution() {
    let interaction = InteractionRequest {
        interaction_id: "interaction-user-input-1".to_string(),
        kind: InteractionKind::UserInput,
        status: InteractionStatus::Resolved,
        scope: InteractionScope {
            session_id: "session-golden".to_string(),
            turn_id: "turn-golden-1".to_string(),
            item_id: None,
            tool_id: None,
            agent_path: None,
        },
        payload: InteractionPayload::UserInput {
            questions: vec![
                UserQuestion {
                    id: "style".to_string(),
                    header: "Style".to_string(),
                    question: "Choose the response style.".to_string(),
                    is_other: false,
                    is_secret: false,
                    options: Some(vec![UserQuestionOption {
                        label: "Concise".to_string(),
                        description: "Keep the answer short.".to_string(),
                    }]),
                },
                UserQuestion {
                    id: "notes".to_string(),
                    header: "Notes".to_string(),
                    question: "Add any extra constraints.".to_string(),
                    is_other: true,
                    is_secret: false,
                    options: Some(vec![UserQuestionOption {
                        label: "Include risks".to_string(),
                        description: "Mention possible regressions.".to_string(),
                    }]),
                },
                UserQuestion {
                    id: "token".to_string(),
                    header: "Token".to_string(),
                    question: "Paste the secret token.".to_string(),
                    is_other: false,
                    is_secret: true,
                    options: None,
                },
            ],
        },
        created_at: 10,
        updated_at: 11,
        resolved_at: Some(11),
        resolution: Some(InteractionResolution::UserInput {
            answers: HashMap::from([
                (
                    "style".to_string(),
                    UserInputAnswer {
                        answers: vec!["Concise".to_string()],
                    },
                ),
                (
                    "notes".to_string(),
                    UserInputAnswer {
                        answers: vec!["Include risks".to_string(), "add rollback plan".to_string()],
                    },
                ),
                (
                    "token".to_string(),
                    UserInputAnswer {
                        answers: vec!["sk-test-secret".to_string()],
                    },
                ),
            ]),
        }),
    };

    assert_eq!(
        serde_json::to_value(StudioEventKind::InteractionChanged {
            event: Box::new(InteractionChangedEvent { interaction }),
        })
        .unwrap(),
        serde_json::json!({
            "type": "interactionChanged",
            "event": {
                "interaction": {
                    "interactionId": "interaction-user-input-1",
                    "kind": "userInput",
                    "status": "resolved",
                    "scope": {
                        "sessionId": "session-golden",
                        "turnId": "turn-golden-1"
                    },
                    "payload": {
                        "type": "userInput",
                        "questions": [
                            {
                                "id": "style",
                                "header": "Style",
                                "question": "Choose the response style.",
                                "isOther": false,
                                "isSecret": false,
                                "options": [
                                    {
                                        "label": "Concise",
                                        "description": "Keep the answer short."
                                    }
                                ]
                            },
                            {
                                "id": "notes",
                                "header": "Notes",
                                "question": "Add any extra constraints.",
                                "isOther": true,
                                "isSecret": false,
                                "options": [
                                    {
                                        "label": "Include risks",
                                        "description": "Mention possible regressions."
                                    }
                                ]
                            },
                            {
                                "id": "token",
                                "header": "Token",
                                "question": "Paste the secret token.",
                                "isOther": false,
                                "isSecret": true
                            }
                        ]
                    },
                    "createdAt": 10,
                    "updatedAt": 11,
                    "resolvedAt": 11,
                    "resolution": {
                        "type": "userInput",
                        "answers": {
                            "notes": {
                                "answers": ["Include risks", "add rollback plan"]
                            },
                            "style": {
                                "answers": ["Concise"]
                            },
                            "token": {
                                "answers": ["sk-test-secret"]
                            }
                        }
                    }
                }
            }
        })
    );
}

#[test]
fn tool_approval_interaction_serializes_decision_and_reason() {
    let event = StudioEventKind::InteractionChanged {
        event: Box::new(InteractionChangedEvent {
            interaction: InteractionRequest {
                interaction_id: "interaction-tool-approval-1".to_string(),
                kind: InteractionKind::ToolApproval,
                status: InteractionStatus::Resolved,
                scope: InteractionScope {
                    session_id: "session-golden".to_string(),
                    turn_id: "turn-golden-1".to_string(),
                    item_id: None,
                    tool_id: Some("tool-call-1".to_string()),
                    agent_path: None,
                },
                payload: InteractionPayload::ToolApproval {
                    name: "bash".to_string(),
                    arguments: serde_json::json!({ "command": "pwd" }),
                    working_directory: Some("D:/repo".to_string()),
                    parent_agent_id: None,
                },
                created_at: 20,
                updated_at: 21,
                resolved_at: Some(21),
                resolution: Some(InteractionResolution::ToolApproval {
                    decision: ToolApprovalResolution::Approved,
                    reason: None,
                }),
            },
        }),
    };

    assert_eq!(
        serde_json::to_value(event).unwrap(),
        serde_json::json!({
            "type": "interactionChanged",
            "event": {
                "interaction": {
                    "interactionId": "interaction-tool-approval-1",
                    "kind": "toolApproval",
                    "status": "resolved",
                    "scope": {
                        "sessionId": "session-golden",
                        "turnId": "turn-golden-1",
                        "toolId": "tool-call-1"
                    },
                    "payload": {
                        "type": "toolApproval",
                        "name": "bash",
                        "arguments": { "command": "pwd" },
                        "workingDirectory": "D:/repo"
                    },
                    "createdAt": 20,
                    "updatedAt": 21,
                    "resolvedAt": 21,
                    "resolution": {
                        "type": "toolApproval",
                        "decision": "approved"
                    }
                }
            }
        })
    );
}
