use std::sync::Arc;

use pl_core::{
    chat::{PresentationPart, presentation_item_id, presentation_prefix},
    model::{
        ModelProgress, ModelProgressSender, ModelRecoveryPhase, ModelTextChannel, ObservedItemKind,
        ObservedPart, ObservedPartIdentity, ObservedPartKind, ProviderPartIdentity,
    },
};

fn part(text: &str) -> ObservedPart {
    ObservedPart::new(ObservedPartIdentity::Provider(ProviderPartIdentity {
        item_id: Arc::from("same-provider-id"),
        output_index: Some(0),
        item_kind: ObservedItemKind::Text(ModelTextChannel::Final),
        part: ObservedPartKind::OutputText,
        content_index: 0,
    }))
    .authorized(text)
}

#[test]
fn latest_snapshot_retains_failed_boundary_and_shared_content_for_a_slow_observer() {
    let sender = ModelProgressSender::detached(1024);
    sender.edit_parts(|parts| parts.push(part("failed body")));
    sender.charge_output(11).unwrap();
    let held = sender.latest();
    sender
        .begin_recovery(5, Arc::from("connection interrupted"))
        .unwrap();
    sender.edit_parts(|parts| parts.push(part("successful body")));
    sender.charge_output(26).unwrap();
    sender.finish_recovery(ModelRecoveryPhase::Recovered);
    let latest = sender.latest();
    assert_eq!(latest.observation().generation, 1);
    assert_eq!(latest.observation().failed.len(), 1);
    let failed = &latest.observation().failed[0];
    assert_eq!(failed.parts[0].text(), "failed body");
    assert!(Arc::ptr_eq(
        held.parts()[0].content(),
        failed.parts[0].content()
    ));
    assert_eq!(latest.parts()[0].text(), "successful body");
    assert_eq!(held.parts()[0].text(), "failed body");
    assert_eq!(
        sender.charged_output(),
        26,
        "retry must not reset the shared quota"
    );
    assert_eq!(
        latest.observation().recovery.unwrap().phase,
        ModelRecoveryPhase::Recovered
    );
    let stored: ModelProgress =
        serde_json::from_str(&serde_json::to_string(&latest).unwrap()).unwrap();
    assert_eq!(
        stored.observation().failed[0].parts[0].text(),
        "failed body"
    );
}

#[test]
fn exhausted_observation_policy_never_discards_an_earlier_boundary() {
    let sender = ModelProgressSender::detached(1024);
    for generation in 0..5 {
        sender.edit_parts(|parts| parts.push(part(&format!("generation {generation}"))));
        sender.begin_recovery(5, Arc::from("retry")).unwrap();
    }
    sender.edit_parts(|parts| parts.push(part("last body")));
    assert!(
        sender
            .begin_recovery(5, Arc::from("cannot reset budget"))
            .is_err()
    );
    let latest = sender.latest();
    assert_eq!(latest.observation().failed.len(), 5);
    assert_eq!(latest.observation().generation, 5);
    assert_eq!(latest.parts()[0].text(), "last body");
    assert_eq!(
        latest.observation().failed[0].parts[0].text(),
        "generation 0"
    );
}

#[test]
fn old_v2_observations_default_to_original_generation_and_original_display_identity() {
    let original = ModelProgress::new(7, vec![part("old saved body")]);
    let mut encoded = serde_json::to_value(original).unwrap();
    encoded.as_object_mut().unwrap().remove("observation");
    let old: ModelProgress = serde_json::from_value(encoded).unwrap();
    assert_eq!(old.observation().generation, 0);
    assert!(old.observation().failed.is_empty());
    assert!(old.observation().recovery.is_none());
    assert_eq!(old.parts()[0].text(), "old saved body");
    let part = Some(PresentationPart::OutputText(0));
    assert_eq!(
        presentation_item_id("attempt", 0, "same-provider-id", part),
        "model:7:attempt:presentation:item:16:same-provider-id:text:0"
    );
    let next = presentation_item_id("attempt", 1, "same-provider-id", part);
    assert_ne!(
        next,
        presentation_item_id("attempt", 0, "same-provider-id", part)
    );
    assert!(next.starts_with(&presentation_prefix("attempt")));
}

#[test]
fn live_usage_is_typed_monotonic_and_not_persisted() {
    let sender = ModelProgressSender::detached(1024);
    sender.publish(ModelProgress::live("turn-1", "attempt-1"));
    sender.observe_usage(Some(12), Some(100));
    sender.observe_decode_millis(8);
    let first = sender.latest();
    let usage = first.usage_observation().unwrap();
    assert_eq!(usage.turn_id, "turn-1");
    assert_eq!(usage.attempt_id, "attempt-1");
    assert_eq!(usage.completion_tokens, Some(12));
    assert_eq!(usage.latest_context_tokens, Some(100));
    assert_eq!(usage.decode_millis, Some(8));
    assert_eq!(usage.observation_sequence, 2);

    // Unknown or smaller later reports cannot erase a confirmed live value.
    sender.observe_usage(None, Some(90));
    sender.observe_usage(Some(7), None);
    sender.observe_decode_millis(4);
    assert_eq!(sender.latest().usage_observation(), Some(usage));

    let encoded = serde_json::to_value(&first).unwrap();
    assert!(
        !encoded.as_object().unwrap().contains_key("usage"),
        "live usage must not cross a durable boundary"
    );
    let restored: ModelProgress = serde_json::from_value(encoded).unwrap();
    assert!(restored.usage_observation().is_none());
}
