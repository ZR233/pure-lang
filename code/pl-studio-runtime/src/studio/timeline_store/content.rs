//! Bounded previews and chunked display content for oversized timeline items.
//!
//! A page never decodes a whole large item: the durable slot row stores only a bounded preview plus
//! a [`ContentRef`], while the full **current-format display JSON** is split into fixed-size chunk
//! rows. [`TimelineReader::read_content`] then fetches only the chunks an offset window touches.
//! The stored bytes are the whole serialized item, so a caller can still reassemble and verify the
//! digest, and the model-visible context is never affected.

use super::TimelineStoreError;
use super::read::{TimelineReader, statement, unsigned};
use super::schema::{TABLE_CONTENT, TABLE_CONTENT_META};
use sea_orm::ConnectionTrait;
use sha2::{Digest, Sha256};

/// Maximum bytes kept inline in a slot row as a bounded preview.
pub(crate) const PREVIEW_BYTES: usize = 4 * 1024;
/// Fixed byte size of one content chunk row.
pub(crate) const CONTENT_CHUNK_BYTES: usize = 8 * 1024;

/// Identity and integrity metadata of one chunked display payload.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ContentRef {
    pub ref_id: String,
    pub digest: String,
    pub total_bytes: u64,
    pub revision: u64,
}

/// One bounded slice of a chunked display payload, always starting on a UTF-8 boundary.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ContentChunk {
    pub bytes: Vec<u8>,
    pub offset: u64,
    pub next_offset: Option<u64>,
    pub digest: String,
    pub total_bytes: u64,
}

/// How one item's display JSON is split for storage.
pub(super) struct ContentPlan {
    pub preview: String,
    pub truncated: bool,
    pub reference: Option<ContentRef>,
    pub chunks: Vec<Vec<u8>>,
}

/// Splits one item's display JSON into a bounded preview and, when oversized, chunk rows.
pub(super) fn plan_content(
    thread_id: &str,
    slot_key: &str,
    revision: u64,
    content: &[u8],
) -> Result<ContentPlan, TimelineStoreError> {
    let text = std::str::from_utf8(content)
        .map_err(|_| TimelineStoreError::Corrupt("display content is not UTF-8".into()))?;
    let digest = digest_hex(content);
    let total_bytes = content.len() as u64;
    if content.len() <= PREVIEW_BYTES {
        return Ok(ContentPlan {
            preview: text.to_owned(),
            truncated: false,
            reference: None,
            chunks: Vec::new(),
        });
    }
    let boundary = clamp_to_boundary(content, PREVIEW_BYTES);
    Ok(ContentPlan {
        preview: text[..boundary].to_owned(),
        truncated: true,
        reference: Some(ContentRef {
            ref_id: content_ref_id(thread_id, slot_key, revision, &digest),
            digest,
            total_bytes,
            revision,
        }),
        chunks: split_chunks(content)
            .into_iter()
            .map(<[u8]>::to_vec)
            .collect(),
    })
}

/// SHA-256 digest of the encoded bytes, as lowercase hex.
pub(super) fn digest_hex(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    let digest = hasher.finalize();
    let mut hex = String::with_capacity(digest.len() * 2);
    for byte in digest {
        hex.push(char::from_digit((byte >> 4) as u32, 16).unwrap_or('0'));
        hex.push(char::from_digit((byte & 0x0f) as u32, 16).unwrap_or('0'));
    }
    hex
}

/// Content-addressed reference id unique to one `(Thread, slot, revision, payload)`.
fn content_ref_id(thread_id: &str, slot_key: &str, revision: u64, digest: &str) -> String {
    let mut hasher = Sha256::new();
    for part in [
        thread_id.as_bytes(),
        b"\0",
        slot_key.as_bytes(),
        b"\0",
        revision.to_string().as_bytes(),
        b"\0",
        digest.as_bytes(),
    ] {
        hasher.update(part);
    }
    let digest = hasher.finalize();
    let mut hex = String::with_capacity(digest.len() * 2);
    for byte in digest {
        hex.push(char::from_digit((byte >> 4) as u32, 16).unwrap_or('0'));
        hex.push(char::from_digit((byte & 0x0f) as u32, 16).unwrap_or('0'));
    }
    hex
}

/// Fixed-size byte chunks; a chunk may end mid-character and is reassembled before decoding.
pub(super) fn split_chunks(content: &[u8]) -> Vec<&[u8]> {
    content.chunks(CONTENT_CHUNK_BYTES).collect()
}

/// The largest character-boundary index not greater than `limit`.
///
/// `limit` may be `>= content.len()`, in which case the whole payload end (a boundary) is returned.
pub(super) fn clamp_to_boundary(content: &[u8], limit: usize) -> usize {
    let mut index = limit.min(content.len());
    while index > 0 && index < content.len() && (content[index] & 0xc0) == 0x80 {
        index -= 1;
    }
    index
}

/// Whether `offset` is a UTF-8 character boundary in `content`.
pub(super) fn is_boundary(content: &[u8], offset: usize) -> bool {
    offset == 0 || offset >= content.len() || (content[offset] & 0xc0) != 0x80
}

/// The next character boundary strictly greater than `offset`, for guaranteed forward progress.
pub(super) fn next_boundary(content: &[u8], offset: usize) -> usize {
    let mut index = offset + 1;
    while index < content.len() && (content[index] & 0xc0) == 0x80 {
        index += 1;
    }
    index
}

impl TimelineReader {
    /// Reads one bounded UTF-8-safe slice of a chunked display payload.
    ///
    /// # Errors
    /// Fails for an unknown reference, an offset past the payload, an offset that is not a UTF-8
    /// character boundary, a zero-length request, or a corrupt chunk chain.
    pub(crate) async fn read_content(
        &self,
        thread_id: &str,
        ref_id: &str,
        offset: u64,
        max_bytes: usize,
    ) -> Result<ContentChunk, TimelineStoreError> {
        if max_bytes == 0 {
            return Err(TimelineStoreError::InvalidRequest(
                "content read must request at least one byte".into(),
            ));
        }
        let meta_sql = format!(
            "SELECT digest, total_bytes FROM {TABLE_CONTENT_META} WHERE thread_id=? AND ref_id=?"
        );
        let meta = self
            .db
            .query_one_raw(statement(&meta_sql, vec![thread_id.into(), ref_id.into()]))
            .await?;
        let meta = meta.ok_or_else(|| TimelineStoreError::UnknownContentRef {
            ref_id: ref_id.to_owned(),
        })?;
        let digest: String = meta.try_get("", "digest")?;
        let total = unsigned(meta.try_get::<i64>("", "total_bytes")?)?;
        if offset > total {
            return Err(TimelineStoreError::InvalidOffset { offset, total });
        }
        if offset == total {
            return Ok(ContentChunk {
                bytes: Vec::new(),
                offset,
                next_offset: None,
                digest,
                total_bytes: total,
            });
        }
        let end = offset.saturating_add(max_bytes as u64).min(total);
        let chunk_bytes = CONTENT_CHUNK_BYTES as u64;
        let start_chunk = (offset / chunk_bytes) as i64;
        let last_chunk = (end / chunk_bytes) as i64;
        let chunk_sql = format!(
            "SELECT chunk, bytes FROM {TABLE_CONTENT} WHERE thread_id=? AND ref_id=? AND chunk BETWEEN ? AND ? ORDER BY chunk ASC"
        );
        let rows = self
            .db
            .query_all_raw(statement(
                &chunk_sql,
                vec![
                    thread_id.into(),
                    ref_id.into(),
                    start_chunk.into(),
                    last_chunk.into(),
                ],
            ))
            .await?;
        let mut buffer = Vec::new();
        for (expected, row) in (start_chunk..).zip(rows) {
            let chunk: i64 = row.try_get("", "chunk")?;
            if chunk != expected {
                return Err(TimelineStoreError::Corrupt(format!(
                    "timeline content {ref_id} is missing chunk {expected}"
                )));
            }
            let bytes: Vec<u8> = row.try_get("", "bytes")?;
            buffer.extend_from_slice(&bytes);
        }
        let base = start_chunk as u64 * chunk_bytes;
        let rel = (offset - base) as usize;
        if rel >= buffer.len() || !is_boundary(&buffer, rel) {
            return Err(TimelineStoreError::NotUtf8Boundary { offset });
        }
        let mut cut = ((end - base) as usize).min(buffer.len());
        while cut > rel && !is_boundary(&buffer, cut) {
            cut -= 1;
        }
        if cut <= rel {
            // No whole character fits in the requested window: advance one character to guarantee
            // forward progress instead of returning a sticky empty slice.
            cut = next_boundary(&buffer, rel).min(buffer.len());
        }
        let absolute = base + cut as u64;
        Ok(ContentChunk {
            bytes: buffer[rel..cut].to_vec(),
            offset,
            next_offset: if absolute >= total {
                None
            } else {
                Some(absolute)
            },
            digest,
            total_bytes: total,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::studio::timeline_store::fixture;
    use crate::studio::timeline_store::{TimelineBudget, TimelinePageQuery, TimelineReader};
    use pretty_assertions::assert_eq;

    fn multibyte(len: usize) -> Vec<u8> {
        "行号测试内容abc\n"
            .repeat(len / "行号测试内容abc\n".len() + 1)
            .into_bytes()
    }

    #[test]
    fn clamp_to_boundary_handles_tail_empty_and_oversized_limits() {
        let content = multibyte(64);
        // limit >= len returns the whole payload end without reading past it.
        assert_eq!(clamp_to_boundary(&content, content.len()), content.len());
        assert_eq!(
            clamp_to_boundary(&content, content.len() + 100),
            content.len()
        );
        assert_eq!(clamp_to_boundary(&content, usize::MAX), content.len());
        // Empty input is a boundary and must never index out of bounds.
        assert_eq!(clamp_to_boundary(&[], 0), 0);
        assert_eq!(clamp_to_boundary(&[], 64), 0);
        // A mid-character limit is stepped back to a real boundary.
        let mid = content.len() / 2;
        let boundary = clamp_to_boundary(&content, mid);
        assert!(std::str::from_utf8(&content[..boundary]).is_ok());
        assert!(boundary <= mid);
        assert!(mid - boundary < 4, "must step back at most one character");
    }

    #[test]
    fn split_chunks_round_trips_tail_sized_and_empty_payloads() {
        let exact = multibyte(CONTENT_CHUNK_BYTES * 3);
        let tail = multibyte(CONTENT_CHUNK_BYTES * 2 + 7);
        for payload in [Vec::new(), vec![b'a'], exact, tail] {
            let chunks = split_chunks(&payload);
            let expected_chunks = payload.len().div_ceil(CONTENT_CHUNK_BYTES);
            assert_eq!(chunks.len(), expected_chunks);
            let reassembled: Vec<u8> = chunks.concat();
            assert_eq!(reassembled, payload, "chunk reassembly must be exact");
            assert_eq!(digest_hex(&reassembled), digest_hex(&payload));
        }
    }

    #[test]
    fn plan_content_marks_small_items_complete_and_large_items_truncated() {
        let small = b"{\"id\":\"x\"}";
        let plan = plan_content("thread", "slot", 1, small).unwrap();
        assert!(!plan.truncated);
        assert!(plan.reference.is_none());
        assert_eq!(plan.preview.as_bytes(), small);

        let exact = vec![b'a'; PREVIEW_BYTES];
        let plan = plan_content("thread", "slot", 1, &exact).unwrap();
        assert!(
            !plan.truncated,
            "an item exactly at the budget stays complete"
        );

        let large = multibyte(PREVIEW_BYTES * 3);
        let plan = plan_content("thread", "slot", 1, &large).unwrap();
        assert!(plan.truncated);
        assert!(plan.preview.len() <= PREVIEW_BYTES);
        assert!(std::str::from_utf8(plan.preview.as_bytes()).is_ok());
        let reference = plan.reference.as_ref().unwrap();
        assert_eq!(reference.total_bytes as usize, large.len());
        assert_eq!(reference.digest, digest_hex(&large));
        assert_eq!(plan.chunks.concat(), large);
    }

    async fn seeded_large_item() -> (
        tempfile::TempDir,
        std::path::PathBuf,
        crate::studio::thread_projection::engine::ProjectionState,
    ) {
        let dir = tempfile::tempdir().unwrap();
        let path = fixture::session_path(&dir);
        let large = "行号测试内容\n".repeat(4_000);
        let mut journal = vec![{
            let mut first = fixture::commit(1);
            first.turn = Some(fixture::turn(None));
            first
        }];
        let mut second = fixture::commit(2);
        second.attempt = Some(fixture::committed("a1", &large));
        journal.push(second);
        let state = fixture::indexed(&path, "thread", &journal).await;
        (dir, path, state)
    }

    #[tokio::test]
    async fn chunked_content_reassembles_utf8_exactly_with_a_matching_digest() {
        let (_dir, path, state) = seeded_large_item().await;
        let text_item = state
            .materialize()
            .into_iter()
            .find(|item| item.text().is_some())
            .expect("a text item exists");
        let expected = serde_json::to_vec(&text_item).unwrap();
        assert!(
            expected.len() > CONTENT_CHUNK_BYTES * 2,
            "fixture must span chunks: {}",
            expected.len()
        );

        let reader = TimelineReader::open(&path).await.unwrap();
        let window = reader
            .page(
                "thread",
                &TimelinePageQuery::Latest,
                &TimelineBudget::default(),
            )
            .await
            .unwrap();
        let entry = window
            .entries
            .iter()
            .find(|entry| entry.item_id == text_item.id)
            .expect("the large item is on the page");
        assert!(entry.preview.decode_item().is_none());
        let reference = entry
            .content
            .as_ref()
            .expect("large item carries a content ref");
        assert_eq!(reference.total_bytes as usize, expected.len());
        assert_eq!(reference.digest, digest_hex(&expected));

        let mut reassembled = Vec::new();
        let mut offset = 0u64;
        loop {
            let chunk = reader
                .read_content("thread", &reference.ref_id, offset, 3_000)
                .await
                .unwrap();
            assert!(chunk.bytes.len() <= 3_000);
            reassembled.extend_from_slice(&chunk.bytes);
            match chunk.next_offset {
                Some(next) => offset = next,
                None => break,
            }
        }
        assert_eq!(reassembled, expected);
        assert_eq!(digest_hex(&reassembled), reference.digest);

        let missing = reader
            .read_content("thread", "nope", 0, 16)
            .await
            .unwrap_err();
        assert!(matches!(
            missing,
            TimelineStoreError::UnknownContentRef { .. }
        ));
        reader.close().await.unwrap();
    }

    #[tokio::test]
    async fn content_offsets_outside_the_payload_or_on_a_character_boundary_are_rejected() {
        let (_dir, path, state) = seeded_large_item().await;
        let text_item = state
            .materialize()
            .into_iter()
            .find(|item| item.text().is_some())
            .unwrap();
        let expected = serde_json::to_vec(&text_item).unwrap();
        let mid_char = (1..expected.len())
            .find(|index| expected[*index] & 0xc0 == 0x80)
            .expect("a multibyte continuation byte exists");

        let reader = TimelineReader::open(&path).await.unwrap();
        let window = reader
            .page(
                "thread",
                &TimelinePageQuery::Latest,
                &TimelineBudget::default(),
            )
            .await
            .unwrap();
        let reference = window
            .entries
            .iter()
            .find_map(|entry| entry.content.clone())
            .unwrap();

        let past_end = reader
            .read_content("thread", &reference.ref_id, reference.total_bytes + 5, 32)
            .await
            .unwrap_err();
        assert!(matches!(past_end, TimelineStoreError::InvalidOffset { .. }));

        let split = reader
            .read_content("thread", &reference.ref_id, mid_char as u64, 32)
            .await
            .unwrap_err();
        assert!(matches!(split, TimelineStoreError::NotUtf8Boundary { .. }));

        let empty = reader
            .read_content("thread", &reference.ref_id, 0, 0)
            .await
            .unwrap_err();
        assert!(matches!(empty, TimelineStoreError::InvalidRequest(_)));
        reader.close().await.unwrap();
    }
}
