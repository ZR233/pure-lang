use super::*;
use pl_protocol::{
    InteractionKind, InteractionPayload, InteractionRequest, InteractionScope, InteractionStatus,
    PlanLifecycleState, RuntimeCostAmount,
};
use pretty_assertions::assert_eq;

#[test]
fn serializes_enabled_tools_trace_event_as_camel_case() {
    let event = TraceEventKind::EnabledToolsRecorded {
        event: EnabledToolsEvent {
            turn_id: "turn-1".to_string(),
            tools: vec!["bash".to_string(), "lsp_query".to_string()],
        },
    };

    assert_eq!(
        serde_json::to_value(event).unwrap(),
        serde_json::json!({
            "type": "enabledToolsRecorded",
            "event": {
                "turnId": "turn-1",
                "tools": ["bash", "lsp_query"]
            }
        })
    );
}

#[test]
fn serializes_interaction_changed_as_camel_case() {
    let event = AgentEvent::InteractionChanged {
        event: InteractionChangedEvent {
            interaction: InteractionRequest {
                interaction_id: "call-1".to_string(),
                kind: InteractionKind::ToolApproval,
                status: InteractionStatus::Pending,
                scope: InteractionScope {
                    session_id: "session-1".to_string(),
                    turn_id: "turn-1".to_string(),
                    item_id: Some("call-1".to_string()),
                    tool_id: Some("call-1".to_string()),
                    agent_path: None,
                },
                payload: InteractionPayload::ToolApproval {
                    name: "bash".to_string(),
                    arguments: serde_json::json!({"command": "echo hi"}),
                    working_directory: Some("C:/project".to_string()),
                    parent_agent_id: None,
                },
                created_at: 1_779_688_800,
                updated_at: 1_779_688_800,
                resolved_at: None,
                resolution: None,
            },
        },
    };

    assert_eq!(
        serde_json::to_value(event).unwrap(),
        serde_json::json!({
            "interactionChanged": {
                "event": {
                    "interaction": {
                        "interactionId": "call-1",
                        "kind": "toolApproval",
                        "status": "pending",
                        "scope": {
                            "sessionId": "session-1",
                            "turnId": "turn-1",
                            "itemId": "call-1",
                            "toolId": "call-1"
                        },
                        "payload": {
                            "type": "toolApproval",
                            "name": "bash",
                            "arguments": {"command": "echo hi"},
                            "workingDirectory": "C:/project"
                        },
                        "createdAt": 1779688800,
                        "updatedAt": 1779688800
                    }
                }
            }
        })
    );
}

fn trace_text_part() -> TracePart {
    TracePart::text(
        "turn-1",
        "item-1",
        0,
        TraceTextChannel::Final,
        "hello",
        TracePartStatus::Completed,
        1_779_688_800,
    )
}

#[test]
fn serializes_trace_part_started_as_camel_case() {
    let event = TraceEvent {
        session_id: "sess-1".to_string(),
        sequence: 0,
        timestamp: 1_779_688_800,
        kind: TraceEventKind::TracePartStarted {
            item: trace_text_part(),
        },
    };

    assert_eq!(
        serde_json::to_value(event).unwrap(),
        serde_json::json!({
            "sessionId": "sess-1",
            "sequence": 0,
            "timestamp": 1779688800,
            "kind": {
                "type": "tracePartStarted",
                "item": {
                    "turnId": "turn-1",
                    "itemId": "item-1",
                    "startedSequence": 0,
                    "revision": 0,
                    "kind": "text",
                    "status": "completed",
                    "createdAt": 1779688800,
                    "updatedAt": 1779688800,
                    "textChannel": "final",
                    "content": "hello"
                }
            }
        })
    );
}

#[test]
fn serializes_turn_budget_limited_as_camel_case() {
    let event = AgentEvent::TurnBudgetLimited {
        reason: "budget limited".to_string(),
        limit_kind: BudgetLimitKind::ToolCall,
        usage: BudgetUsage {
            model_steps: 3,
            tool_calls: 121,
            wait_calls: 2,
            elapsed_ms: 42,
        },
    };

    assert_eq!(
        serde_json::to_value(event).unwrap(),
        serde_json::json!({
            "turnBudgetLimited": {
                "reason": "budget limited",
                "limitKind": "toolCall",
                "usage": {
                    "modelSteps": 3,
                    "toolCalls": 121,
                    "waitCalls": 2,
                    "elapsedMs": 42
                }
            }
        })
    );
}

#[test]
fn serializes_agent_runtime_updated_as_camel_case() {
    let event = AgentEvent::AgentRuntimeUpdated {
        delta: AgentRuntimeDelta {
            inference_id: "inf-1".to_string(),
            agent_id: "agent-1".to_string(),
            path: "/root/research".to_string(),
            parent_path: Some("/root".to_string()),
            role: "explorer".to_string(),
            model: "deepseek-v4-flash".to_string(),
            context_window: Some(1_000_000),
            usage: TokenUsageSnapshot {
                prompt_tokens: 100,
                completion_tokens: 20,
                cached_prompt_tokens: 40,
                total_tokens: 120,
            },
            estimated_costs: vec![RuntimeCostAmount {
                currency: "CNY".to_string(),
                amount: 0.001,
            }],
            has_unpriced_usage: false,
            updated_at: 1_779_688_800,
        },
    };

    assert_eq!(
        serde_json::to_value(event).unwrap(),
        serde_json::json!({
            "agentRuntimeUpdated": {
                "delta": {
                    "inferenceId": "inf-1",
                    "agentId": "agent-1",
                    "path": "/root/research",
                    "parentPath": "/root",
                    "role": "explorer",
                    "model": "deepseek-v4-flash",
                    "contextWindow": 1000000,
                    "usage": {
                        "promptTokens": 100,
                        "completionTokens": 20,
                        "cachedPromptTokens": 40,
                        "totalTokens": 120
                    },
                    "estimatedCosts": [
                        {
                            "currency": "CNY",
                            "amount": 0.001
                        }
                    ],
                    "hasUnpricedUsage": false,
                    "updatedAt": 1779688800
                }
            }
        })
    );
}

#[test]
fn serializes_skill_activation_events_as_camel_case() {
    let activation = SkillActivation {
        name: "rust-flow".to_string(),
        source: "project".to_string(),
        path: "skills/rust-flow".to_string(),
        turn_id: "turn-1".to_string(),
        tool_call_id: "turn-1-call-1".to_string(),
        activated_at: 1_779_688_800,
    };

    assert_eq!(
        serde_json::to_value(AgentEvent::SkillActivated {
            activation: activation.clone()
        })
        .unwrap(),
        serde_json::json!({
            "skillActivated": {
                "activation": {
                    "name": "rust-flow",
                    "source": "project",
                    "path": "skills/rust-flow",
                    "turnId": "turn-1",
                    "toolCallId": "turn-1-call-1",
                    "activatedAt": 1779688800
                }
            }
        })
    );
    assert_eq!(
        serde_json::to_value(TraceEventKind::SkillActivated { activation }).unwrap(),
        serde_json::json!({
            "type": "skillActivated",
            "activation": {
                "name": "rust-flow",
                "source": "project",
                "path": "skills/rust-flow",
                "turnId": "turn-1",
                "toolCallId": "turn-1-call-1",
                "activatedAt": 1779688800
            }
        })
    );
}

#[test]
fn serializes_trace_delta_as_camel_case() {
    let event = AgentEvent::TracePartDelta {
        event: TracePartDeltaEvent {
            turn_id: "turn-1".to_string(),
            item_id: "item-1".to_string(),
            started_sequence: 2,
            revision: 3,
            kind: TracePartKind::Thinking,
            status: TracePartStatus::Streaming,
            created_at: 1_779_688_800,
            updated_at: 1_779_688_801,
            delta: TraceDelta::Thinking {
                chunk_index: 1,
                delta: "thinking".to_string(),
            },
        },
    };

    assert_eq!(
        serde_json::to_value(event).unwrap(),
        serde_json::json!({
            "tracePartDelta": {
                "event": {
                    "turnId": "turn-1",
                    "itemId": "item-1",
                    "startedSequence": 2,
                    "revision": 3,
                    "kind": "thinking",
                    "status": "streaming",
                    "createdAt": 1779688800,
                    "updatedAt": 1779688801,
                    "delta": {
                        "type": "thinking",
                        "chunkIndex": 1,
                        "delta": "thinking"
                    }
                }
            }
        })
    );
}

#[test]
fn deserializes_legacy_timeline_sequence_as_started_sequence() {
    let item = serde_json::from_value::<TracePart>(serde_json::json!({
        "turnId": "turn-1",
        "itemId": "turn-1-plan",
        "sequence": 7,
        "kind": "plan",
        "status": "streaming",
        "createdAt": 1779688800,
        "updatedAt": 1779688800,
        "content": "# Plan"
    }))
    .unwrap();

    assert_eq!(item.started_sequence, 7);
    assert_eq!(
        serde_json::to_value(item).unwrap()["startedSequence"],
        serde_json::json!(7)
    );
}

#[test]
fn serializes_plan_lifecycle_trace_event_as_camel_case() {
    let event = TraceEvent {
        session_id: "sess-1".to_string(),
        sequence: 3,
        timestamp: 1_779_688_802,
        kind: TraceEventKind::PlanLifecycleChanged {
            event: PlanLifecycleEvent {
                plan_id: "turn-1-plan".to_string(),
                state: PlanLifecycleState::ImplementationFailed,
                turn_id: Some("turn-2".to_string()),
                reason: Some("provider error".to_string()),
                updated_at: 1_779_688_802,
            },
        },
    };

    assert_eq!(
        serde_json::to_value(event).unwrap(),
        serde_json::json!({
            "sessionId": "sess-1",
            "sequence": 3,
            "timestamp": 1779688802,
            "kind": {
                "type": "planLifecycleChanged",
                "event": {
                    "planId": "turn-1-plan",
                    "state": "implementationFailed",
                    "turnId": "turn-2",
                    "reason": "provider error",
                    "updatedAt": 1779688802
                }
            }
        })
    );
}
