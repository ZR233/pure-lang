use pl_core::context::OpaquePayload;
use pl_tool::{
    ask_user, finish_turn, session_note,
    task_control::{TaskControlKind, TaskControlTool},
};
use pretty_assertions::assert_eq;

#[test]
fn downstream_can_select_public_tools_with_explicit_registration_authority() {
    let selected = [
        ask_user::registration(OpaquePayload::text("questions")).unwrap(),
        finish_turn::registration(OpaquePayload::text("finish_turn")).unwrap(),
        session_note::registration(
            session_note::SessionNoteToolKind::Read,
            OpaquePayload::text("note"),
        )
        .unwrap(),
        TaskControlTool::new(TaskControlKind::Wait)
            .registration(OpaquePayload::text("wait"))
            .unwrap(),
    ];
    assert_eq!(
        selected
            .iter()
            .map(|tool| tool.tool_id())
            .collect::<Vec<_>>(),
        vec![
            "request_user_input",
            "finish_turn",
            "read_session_note",
            "wait"
        ]
    );
}
