//! Public-API coverage of the typed live observation port.
//!
//! These cases exercise only what a model adapter and a live projection actually call: appending
//! increments to a shared content block, reading the bytes that were not delivered yet, verifying a
//! consumer's baseline against the shared prefix lineage, advancing a content version, completing
//! late metadata, and the snapshot a watch consumer recovers from.

use std::sync::Arc;

use pl_core::chat::PresentationPart;
use pl_core::model::{
    AggregateChannel, ContentBlock, ContentIncrement, ModelParts, ModelProgress, ModelTextChannel,
    ObservedItemKind, ObservedPart, ObservedPartIdentity, ObservedPartKind, ProviderPartIdentity,
};

fn provider_part(item_id: &str, content_index: u32) -> ObservedPartIdentity {
    ObservedPartIdentity::Provider(ProviderPartIdentity {
        item_id: Arc::from(item_id),
        output_index: Some(0),
        item_kind: ObservedItemKind::Text(ModelTextChannel::Commentary),
        part: ObservedPartKind::OutputText,
        content_index,
    })
}

#[test]
fn appended_blocks_share_prefix_and_read_only_the_missing_suffix() {
    let mut block = ContentBlock::empty();
    assert!(block.is_empty());
    for chunk in ["hello", " ", "world"] {
        block = ContentBlock::append(&block, chunk);
    }
    assert_eq!(block.len(), 11);
    assert_eq!(block.text(), "hello world");
    assert_eq!(block.suffix_since(0).as_deref(), Some("hello world"));
    assert_eq!(block.suffix_since(5).as_deref(), Some(" world"));
    assert_eq!(block.suffix_since(11).as_deref(), Some(""));
}

#[test]
fn suffix_since_reports_a_misaligned_or_out_of_range_offset_instead_of_panicking() {
    // "héllo" carries a two-byte `é`, so byte 2 is not a character boundary.
    let block = ContentBlock::from_text("héllo");
    assert_eq!(block.len(), 6);
    assert_eq!(block.text(), "héllo");
    assert_eq!(block.suffix_since(0).as_deref(), Some("héllo"));
    assert_eq!(block.suffix_since(6).as_deref(), Some(""));
    assert_eq!(block.suffix_since(2), None);
    assert_eq!(block.suffix_since(7), None);
}

#[test]
fn empty_delta_and_empty_text_keep_the_shared_block() {
    let block = ContentBlock::from_text("visible");
    let same = ContentBlock::append(&block, "");
    assert!(Arc::ptr_eq(&block, &same));
    assert!(Arc::ptr_eq(
        &ContentBlock::from_text(""),
        &ContentBlock::empty()
    ));
}

#[test]
fn a_long_shared_chain_drops_without_recursing_per_chunk() {
    // One chunk per delta is the shape a token stream produces; dropping it must not recurse once
    // per chunk or a long response would overflow the stack.
    let mut block = ContentBlock::empty();
    for _ in 0..200_000 {
        block = ContentBlock::append(&block, "x");
    }
    assert_eq!(block.len(), 200_000);
    drop(block);
}

#[test]
fn observation_advances_its_content_version_and_completes_with_provider_text() {
    let observed = ObservedPart::new(provider_part("item-1", 0));
    assert_eq!(observed.version(), 0);
    assert!(observed.is_empty());
    assert_eq!(
        observed.presentation_part(),
        Some(PresentationPart::OutputText(0))
    );

    let streamed = observed.appended("hel").appended("lo");
    assert_eq!(streamed.version(), 2);
    assert_eq!(streamed.text(), "hello");
    assert_eq!(streamed.suffix_since(3).as_deref(), Some("lo"));

    // The provider's authoritative text for the same identity extends what was streamed.
    let completed = streamed.authorized("hello!");
    assert_eq!(completed.version(), 3);
    assert_eq!(completed.text(), "hello!");
    // Repeating the same text is not a new content version.
    assert_eq!(completed.authorized("hello!").version(), 3);
}

#[test]
fn verifiable_baseline_reports_appends_and_a_whole_replacement() {
    let observed = ObservedPart::new(provider_part("item-1", 0)).appended("hello");
    let baseline = observed.baseline();
    assert_eq!(baseline.len(), 5);

    let extended = observed.appended(" world");
    assert_eq!(
        extended.increment_since(&baseline),
        ContentIncrement::Append(" world".to_owned())
    );
    assert_eq!(
        extended.increment_since(&extended.baseline()),
        ContentIncrement::Current
    );
}

#[test]
fn authoritative_replacement_invalidates_a_stale_baseline_instead_of_appending() {
    let observed = ObservedPart::new(provider_part("item-1", 0)).appended("hello");
    let baseline = observed.baseline();

    // An extending authoritative body only appends the missing byte and stays on the lineage.
    let extended = observed.authorized("hello!");
    assert_eq!(extended.text(), "hello!");
    assert_eq!(extended.version(), 2);
    assert_eq!(
        extended.increment_since(&baseline),
        ContentIncrement::Append("!".to_owned())
    );

    // A rewriting authoritative body no longer extends the delivered prefix, so the stale byte
    // count alone would append the new body to the old text; the lineage reports a replacement.
    let rewritten = observed.authorized("HELLO");
    assert_eq!(rewritten.text(), "HELLO");
    assert_eq!(
        rewritten.increment_since(&baseline),
        ContentIncrement::Replace("HELLO".to_owned())
    );
}

#[test]
fn late_provider_metadata_completes_the_part_without_restarting_it() {
    let streamed = ObservedPart::new(ObservedPartIdentity::Provider(ProviderPartIdentity {
        item_id: Arc::from("item-1"),
        output_index: None,
        item_kind: ObservedItemKind::Text(ModelTextChannel::Final),
        part: ObservedPartKind::OutputText,
        content_index: 0,
    }))
    .appended("answer");

    // The item later reports its true output index and channel; only metadata changes.
    let completed = streamed.with_metadata(
        Some(3),
        Some(ObservedItemKind::Text(ModelTextChannel::Commentary)),
    );
    assert_eq!(completed.text(), "answer");
    assert_eq!(completed.version(), streamed.version());
    assert!(Arc::ptr_eq(streamed.content(), completed.content()));
    match completed.identity() {
        ObservedPartIdentity::Provider(identity) => {
            assert_eq!(identity.output_index, Some(3));
            assert_eq!(
                identity.item_kind,
                ObservedItemKind::Text(ModelTextChannel::Commentary)
            );
        }
        ObservedPartIdentity::Aggregate { .. } => panic!("provider identity expected"),
    }

    // Omitting a value keeps what is already known.
    let unchanged = completed.with_metadata(None, None);
    assert!(Arc::ptr_eq(completed.content(), unchanged.content()));
    match unchanged.identity() {
        ObservedPartIdentity::Provider(identity) => {
            assert_eq!(identity.output_index, Some(3));
        }
        ObservedPartIdentity::Aggregate { .. } => panic!("provider identity expected"),
    }

    // An aggregate observation has no provider metadata and keeps its shared block untouched.
    let aggregate = ObservedPart::new(ObservedPartIdentity::Aggregate {
        channel: AggregateChannel::Text,
    })
    .appended("answer");
    let untouched = aggregate.with_metadata(Some(9), None);
    assert!(Arc::ptr_eq(aggregate.content(), untouched.content()));
    assert_eq!(untouched.presentation_part(), None);
}

#[test]
fn raw_and_summary_reasoning_parts_of_one_item_stay_distinct_observations() {
    let summary = ObservedPart::new(ObservedPartIdentity::Provider(ProviderPartIdentity {
        item_id: Arc::from("reason-1"),
        output_index: Some(1),
        item_kind: ObservedItemKind::Reasoning,
        part: ObservedPartKind::SummaryText,
        content_index: 0,
    }))
    .appended("brief summary");
    let raw = ObservedPart::new(ObservedPartIdentity::Provider(ProviderPartIdentity {
        item_id: Arc::from("reason-1"),
        output_index: Some(1),
        item_kind: ObservedItemKind::Reasoning,
        part: ObservedPartKind::ReasoningText,
        content_index: 0,
    }))
    .appended("full thought");
    let progress = ModelProgress::new(4, vec![summary, raw]);

    // The summary and the raw reasoning are two parts of the same item, never the same channel.
    assert_eq!(
        progress
            .observed_part("reason-1", PresentationPart::SummaryText(0))
            .expect("the summary is observed")
            .text(),
        "brief summary"
    );
    assert_eq!(
        progress
            .observed_part("reason-1", PresentationPart::ReasoningText(0))
            .expect("the raw reasoning is observed separately")
            .text(),
        "full thought"
    );
    assert!(progress.channel(AggregateChannel::Reasoning).is_none());
}

#[test]
fn snapshot_reads_parts_by_identity_and_by_channel() {
    let aggregate = ObservedPart::new(ObservedPartIdentity::Aggregate {
        channel: AggregateChannel::Reasoning,
    })
    .appended("thinking");
    let item = ObservedPart::new(provider_part("item-1", 0)).appended("answer");
    let progress = ModelProgress::new(7, vec![aggregate, item]);

    assert_eq!(progress.version(), 7);
    assert_eq!(progress.len(), 2);
    assert!(!progress.is_empty());
    assert_eq!(
        progress
            .channel(AggregateChannel::Reasoning)
            .expect("aggregate channel is observed")
            .text(),
        "thinking"
    );
    assert_eq!(
        progress
            .observed_part("item-1", PresentationPart::OutputText(0))
            .expect("provider part is observed")
            .text(),
        "answer"
    );
    assert!(
        progress
            .observed_part("item-1", PresentationPart::ReasoningText(0))
            .is_none()
    );
}

#[test]
fn snapshot_encoding_materializes_text_once_and_reads_back() {
    let progress = ModelProgress::new(
        3,
        vec![
            ObservedPart::new(ObservedPartIdentity::Aggregate {
                channel: AggregateChannel::Text,
            })
            .appended("streamed"),
            ObservedPart::new(provider_part("item-1", 0)).appended("item text"),
        ],
    );
    let encoded = serde_json::to_string(&progress).expect("snapshot encodes");
    assert!(encoded.contains("streamed"));
    let decoded: ModelProgress = serde_json::from_str(&encoded).expect("snapshot decodes");
    assert_eq!(decoded.version(), 3);
    assert_eq!(decoded.len(), 2);
    assert_eq!(decoded.parts()[0].text(), "streamed");
    assert_eq!(decoded.parts()[1].text(), "item text");
    assert_eq!(decoded.parts()[1].version(), 1);

    let empty = serde_json::to_string(&ModelProgress::default()).expect("empty snapshot encodes");
    let decoded: ModelProgress = serde_json::from_str(&empty).expect("empty snapshot decodes");
    assert!(decoded.is_empty());
    assert_eq!(decoded.version(), 0);
}

/// One live observation per item, so the list can grow past a single shared chunk.
fn provider_part_at(index: usize) -> ObservedPart {
    ObservedPart::new(ObservedPartIdentity::Provider(ProviderPartIdentity {
        item_id: Arc::from(format!("item-{index}").as_str()),
        output_index: Some(u32::try_from(index).expect("the fixture index fits")),
        item_kind: ObservedItemKind::Text(ModelTextChannel::Commentary),
        part: ObservedPartKind::OutputText,
        content_index: 0,
    }))
    .appended(&format!("body-{index}"))
}

fn many_provider_parts(count: usize) -> Vec<ObservedPart> {
    (0..count).map(provider_part_at).collect()
}

#[test]
fn a_long_snapshot_keeps_its_order_and_text_across_chunk_boundaries() {
    // 150 observations span several shared chunks plus a growing tail, so this exercises the
    // chunked representation rather than one flat buffer.
    let progress = ModelProgress::new(9, many_provider_parts(150));
    assert_eq!(progress.len(), 150);
    assert!(!progress.is_empty());
    assert_eq!(progress.parts().len(), 150);
    assert_eq!(progress.parts().iter().count(), 150);
    assert_eq!(progress.parts().iter().rev().count(), 150);
    assert_eq!(progress.parts()[0].text(), "body-0");
    assert_eq!(progress.parts()[63].text(), "body-63");
    assert_eq!(progress.parts()[64].text(), "body-64");
    assert_eq!(progress.parts()[149].text(), "body-149");
    assert_eq!(
        progress
            .parts()
            .iter()
            .next_back()
            .expect("the last part is iterable from the back")
            .text(),
        "body-149"
    );
    assert_eq!(
        progress
            .parts()
            .iter()
            .map(ObservedPart::text)
            .collect::<Vec<_>>(),
        (0..150)
            .map(|index| format!("body-{index}"))
            .collect::<Vec<_>>()
    );

    // The serialized shape is still one ordered array of parts, so a longer snapshot round-trips
    // with the same identities, versions and text in the same positions.
    let encoded = serde_json::to_string(&progress).expect("snapshot encodes");
    let decoded: ModelProgress = serde_json::from_str(&encoded).expect("snapshot decodes");
    assert_eq!(decoded.len(), 150);
    for (before, after) in progress.parts().iter().zip(decoded.parts().iter()) {
        assert_eq!(before.identity(), after.identity());
        assert_eq!(before.version(), after.version());
        assert_eq!(before.text(), after.text());
    }
}

#[test]
fn a_held_part_container_is_not_mutated_by_later_edits_to_filled_chunks_or_the_tail() {
    let mut parts = ModelParts::default();
    for part in many_provider_parts(150) {
        parts.push(part);
    }
    // A consumer that kept this frame must keep exactly it, whatever the producer does next.
    let held = parts.clone();

    // Grow the tail, then edit a part that has already rolled into a filled chunk and the new tail.
    let tail_index = parts.len();
    parts.push(provider_part_at(150));
    let edited = parts[3].appended("!");
    parts[3] = edited;
    let edited_tail = parts[tail_index].appended("?");
    parts[tail_index] = edited_tail;

    assert_eq!(held.len(), 150);
    assert_eq!(held[3].text(), "body-3");
    assert_eq!(held[149].text(), "body-149");
    assert!(
        held.iter().all(|part| !part.text().contains('!')),
        "an edit to a filled chunk never leaks into the held frame"
    );
    assert!(
        held.iter().all(|part| !part.text().contains('?')),
        "an edit to the tail never leaks into the held frame"
    );
    assert_eq!(parts.len(), 151);
    assert_eq!(parts[3].text(), "body-3!");
    assert_eq!(parts[tail_index].text(), "body-150?");
}
