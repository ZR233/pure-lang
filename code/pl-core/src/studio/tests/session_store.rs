use super::*;
use pretty_assertions::assert_eq;

#[tokio::test]
async fn session_crud_and_message_restore() {
    let store = StudioStore::open_memory().await.unwrap();
    let project = store.upsert_project("C:/work/alpha").await.unwrap();
    let session = store
        .create_session(&project.id, "Build app", CompileMode::Auto)
        .await
        .unwrap();
    let message = Message {
        role: MessageRole::User,
        content: MessageContent::Text("hello".to_string()),
        reasoning_content: None,
        metadata: HashMap::new(),
    };

    store.append_message(&session.id, &message).await.unwrap();
    let restored = store.load_core_session(&session.id).await.unwrap();

    assert_eq!(restored.len(), 1);
    assert_eq!(restored.messages()[0].role, MessageRole::User);
    match &restored.messages()[0].content {
        MessageContent::Text(text) => assert_eq!(text, "hello"),
        MessageContent::MultiPart(_) => panic!("expected text message"),
    }
}

#[tokio::test]
async fn message_storage_round_trips_image_attachment_parts() {
    let store = StudioStore::open_memory().await.unwrap();
    let project = store.upsert_project("C:/work/alpha").await.unwrap();
    let session = store
        .create_session(&project.id, "Vision", CompileMode::Auto)
        .await
        .unwrap();
    let message = Message {
        role: MessageRole::User,
        content: MessageContent::MultiPart(vec![
            ContentPart::Text {
                text: "what is this?".to_string(),
            },
            ContentPart::Image {
                source: ImageSource::Attachment {
                    attachment_id: "attachment-1".to_string(),
                },
                media_type: "image/png".to_string(),
                filename: Some("image.png".to_string()),
            },
        ]),
        reasoning_content: None,
        metadata: HashMap::new(),
    };

    store.append_message(&session.id, &message).await.unwrap();

    assert_eq!(
        store.load_messages(&session.id).await.unwrap(),
        vec![message]
    );
}

#[tokio::test]
async fn archive_session_hides_it_from_session_list() {
    let store = StudioStore::open_memory().await.unwrap();
    let project = store.upsert_project("C:/work/alpha").await.unwrap();
    let session = store
        .create_session(&project.id, "Build app", CompileMode::Auto)
        .await
        .unwrap();
    let message = Message {
        role: MessageRole::User,
        content: MessageContent::Text("hello".to_string()),
        reasoning_content: None,
        metadata: HashMap::new(),
    };

    store.append_message(&session.id, &message).await.unwrap();
    let archived = store.archive_session(&session.id).await.unwrap().unwrap();
    let sessions = store.list_sessions(&project.id).await.unwrap();
    let restored = store.load_messages(&session.id).await.unwrap();

    assert_eq!(archived.id, session.id);
    assert_eq!(sessions, Vec::<SessionRecord>::new());
    assert_eq!(restored, vec![message]);
}

#[tokio::test]
async fn replace_session_messages_rewrites_history() {
    let store = StudioStore::open_memory().await.unwrap();
    let project = store.upsert_project("C:/work/alpha").await.unwrap();
    let session = store
        .create_session(&project.id, "Build app", CompileMode::Auto)
        .await
        .unwrap();
    let first = Message {
        role: MessageRole::User,
        content: MessageContent::Text("first".to_string()),
        reasoning_content: None,
        metadata: HashMap::new(),
    };
    let second = Message {
        role: MessageRole::User,
        content: MessageContent::Text("second".to_string()),
        reasoning_content: None,
        metadata: HashMap::new(),
    };

    store.append_message(&session.id, &first).await.unwrap();
    store
        .replace_session_messages(&session.id, std::slice::from_ref(&second))
        .await
        .unwrap();
    let restored = store.load_messages(&session.id).await.unwrap();

    assert_eq!(restored.len(), 1);
    assert_eq!(restored[0], second);
}

#[tokio::test]
async fn set_session_mode_persists_mode_label() {
    let store = StudioStore::open_memory().await.unwrap();
    let project = store.upsert_project("C:/work/alpha").await.unwrap();
    let session = store
        .create_session(&project.id, "Plan work", CompileMode::Auto)
        .await
        .unwrap();

    store
        .set_session_mode(&session.id, CompileMode::Plan)
        .await
        .unwrap();
    let updated = store.read_session(&session.id).await.unwrap().unwrap();

    assert_eq!(updated.mode, "plan");
}

#[tokio::test]
async fn instruction_snapshot_round_trips_with_session() {
    let store = StudioStore::open_memory().await.unwrap();
    let project = store.upsert_project("C:/work/alpha").await.unwrap();
    let session = store
        .create_session(&project.id, "Build app", CompileMode::Auto)
        .await
        .unwrap();
    let snapshot = InstructionSnapshot {
        base: InstructionBlock {
            source: InstructionSource {
                kind: InstructionSourceKind::BuiltInBase,
                label: "base".to_string(),
                path: None,
            },
            content: "base".to_string(),
        },
        developer: vec![InstructionBlock {
            source: InstructionSource {
                kind: InstructionSourceKind::Mode,
                label: "mode".to_string(),
                path: None,
            },
            content: "developer".to_string(),
        }],
        user: vec![InstructionBlock {
            source: InstructionSource {
                kind: InstructionSourceKind::ProjectDoc,
                label: "AGENTS.md".to_string(),
                path: Some("C:/work/alpha/AGENTS.md".to_string()),
            },
            content: "project".to_string(),
        }],
    };

    assert_eq!(session.instruction_snapshot, None);
    let saved = store
        .save_instruction_snapshot(&session.id, &snapshot)
        .await
        .unwrap()
        .unwrap();
    let read = store.read_session(&session.id).await.unwrap().unwrap();
    let listed = store.list_sessions(&project.id).await.unwrap();

    assert_eq!(saved.instruction_snapshot, Some(snapshot.clone()));
    assert_eq!(read.instruction_snapshot, Some(snapshot.clone()));
    assert_eq!(listed[0].instruction_snapshot, Some(snapshot));
}
