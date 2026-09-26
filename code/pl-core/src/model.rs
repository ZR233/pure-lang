//! Model invocation ports. Implementations own protocols, encoding, and physical connections.

use std::fmt;
use std::future::Future;
use std::sync::Arc;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicBool, Ordering};

use futures::future::BoxFuture;
use serde::{Deserialize, Serialize};
pub use tokio_util::sync::CancellationToken;

use crate::chat::PresentationPart;
use crate::context::{ContextContent, ContextSnapshot, OpaquePayload};

/// Factory for independent Thread sessions. Clients may share pools, but never mutable history.
pub trait Model: Send + Sync + 'static {
    /// Opens a fresh session; the returned owner must be closed before its Thread is released.
    fn open_session(&self) -> impl Future<Output = Result<DynModelSession, ModelError>> + Send;
}

trait ErasedModel: Send + Sync {
    fn open_session(&self) -> BoxFuture<'_, Result<DynModelSession, ModelError>>;
}
impl<T: Model> ErasedModel for T {
    fn open_session(&self) -> BoxFuture<'_, Result<DynModelSession, ModelError>> {
        Box::pin(Model::open_session(self))
    }
}

/// Shareable factory erasure; each invocation must return an independently owned session.
#[derive(Clone)]
pub struct ModelFactory(Arc<dyn ErasedModel>);
impl ModelFactory {
    /// Erases a model factory without opening a session or starting model work.
    pub fn new(model: impl Model) -> Self {
        Self(Arc::new(model))
    }
    /// Opens a session with panic isolation at the implementation boundary.
    ///
    /// # Errors
    /// Preserves model construction failures and reports panicking factories.
    pub async fn open_session(&self) -> Result<DynModelSession, ModelError> {
        crate::error_record::catch_boundary("model factory", async { self.0.open_session().await })
            .await
            .map_err(ModelError::from_panic)?
    }
}
impl fmt::Debug for ModelFactory {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ModelFactory")
            .finish_non_exhaustive()
    }
}

/// Serial model resource owned by one Thread incarnation.
pub trait ModelSession: Send + 'static {
    /// Freezes implementation selection and request material before dispatch.
    fn prepare(
        &mut self,
        request: ModelRequest,
    ) -> impl Future<Output = Result<PreparedModelCall, ModelError>> + Send;

    /// Stops and waits for owned resources. A failed close retains the implementation for retry.
    fn close(&mut self) -> impl Future<Output = Result<(), ModelError>> + Send;
}

/// Declaration sent alongside context. Core does not interpret its producer-owned schema.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelToolDeclaration {
    pub tool_id: String,
    pub declaration: OpaquePayload,
}

/// Maximum tool-call concurrency admitted by the frozen executor catalog.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolCallMode {
    Sequential,
    Parallel,
}

/// Immutable preparation input. Cancellation remains live while the content stays frozen.
#[derive(Debug, Clone)]
pub struct ModelRequest {
    /// Optional coalescing live observation; never commits canonical content.
    pub progress: Option<ModelProgressSender>,
    pub thread_id: String,
    pub turn_id: String,
    pub attempt_id: String,
    pub context: ContextSnapshot,
    pub tools: Arc<[ModelToolDeclaration]>,
    pub tool_call_mode: ToolCallMode,
    /// Frozen tool IDs that must be the only tool call in a response.
    pub solo_tool_ids: Arc<[String]>,
    pub committed_private_context: Option<OpaquePayload>,
    pub resources: Option<crate::context::ResourceAccess>,
    pub cancellation: CancellationToken,
}

/// Channel the adapter assigned to one assistant-visible text part.
///
/// Core owns this port vocabulary so a live projection can label a provider part without depending
/// on a product protocol; each adapter maps its own channel type into it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ModelTextChannel {
    User,
    Commentary,
    Final,
}

/// Request-scoped aggregate channel used when an adapter reports no provider item boundaries.
///
/// The channel stands in for the whole assistant output of its kind, so it is the stable identity a
/// live projection and the terminal item for that same output share. It never mixes kinds: `Text`
/// carries assistant text, while `Reasoning` carries raw reasoning content. A reasoning *summary* is
/// always one part of a provider item, so it is observed as a provider part rather than through an
/// aggregate channel.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum AggregateChannel {
    Text,
    Reasoning,
}

/// Kind of provider item that owns one observed part.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ObservedItemKind {
    Text(ModelTextChannel),
    Reasoning,
}

/// Stable discriminator of one part inside a provider item.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ObservedPartKind {
    OutputText,
    ReasoningText,
    SummaryText,
}

/// Stable identity of one part of one provider item.
///
/// The provider item id plus the part kind and content index define the row; the output index and
/// provider part ids can arrive after streaming starts and are therefore not part of the identity.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderPartIdentity {
    pub item_id: Arc<str>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output_index: Option<u32>,
    pub item_kind: ObservedItemKind,
    pub part: ObservedPartKind,
    pub content_index: u32,
}

impl ProviderPartIdentity {
    /// The part discriminator [`crate::chat::presentation_item_id`] reserves and finalizes.
    pub fn presentation_part(&self) -> PresentationPart {
        match self.part {
            ObservedPartKind::OutputText => PresentationPart::OutputText(self.content_index),
            ObservedPartKind::ReasoningText => PresentationPart::ReasoningText(self.content_index),
            ObservedPartKind::SummaryText => PresentationPart::SummaryText(self.content_index),
        }
    }
}

/// Stable identity of one live observation in the current preview.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ObservedPartIdentity {
    /// Channel aggregate published before the adapter itemizes the response.
    Aggregate { channel: AggregateChannel },
    /// One part of one provider item.
    Provider(ProviderPartIdentity),
}

/// Immutable, append-only text observed for one live part.
///
/// Every observation shares the chunks observed before it instead of copying the text, so appending
/// one delta costs a single chunk and a new snapshot is a pointer clone. The shared prefix chain is
/// also the lineage a consumer verifies: [`ContentBlock::increment_since`] only reports appended
/// bytes while the consumer's delivered block is still an ancestor, and reports a whole replacement
/// otherwise. Drop walks the prefix iteratively because a per-chunk recursive drop would recurse
/// once per delta and overflow the stack on a long response.
pub struct ContentBlock {
    chunk: Arc<str>,
    prefix: Option<Arc<ContentBlock>>,
    len: usize,
}

/// Increment of an observed part relative to the prefix a consumer already delivered.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ContentIncrement {
    /// The consumer's baseline is already the whole observed text.
    Current,
    /// The observed text still extends the baseline; these bytes are new.
    Append(String),
    /// The observed text no longer extends the baseline; replace the delivered body with this.
    Replace(String),
}

/// Proof that a consumer delivered exactly the prefix held by one observed content block.
///
/// A baseline holds the immutable block the consumer delivered, so a later observation can verify
/// that the consumer is still on its shared prefix chain instead of trusting a stale byte count.
/// After an authoritative replacement the old baseline is no longer an ancestor and the increment
/// reports a whole replacement instead of appending the new body to the old text.
#[derive(Debug, Clone)]
pub struct ContentBaseline {
    block: Arc<ContentBlock>,
}

impl ContentBaseline {
    /// Bytes delivered when this baseline was recorded.
    pub fn len(&self) -> usize {
        self.block.len()
    }

    /// Reports whether the baseline holds no delivered byte.
    pub fn is_empty(&self) -> bool {
        self.block.is_empty()
    }
}

impl ContentBlock {
    /// Shared empty block that every observed text starts from.
    pub fn empty() -> Arc<Self> {
        static EMPTY: OnceLock<Arc<ContentBlock>> = OnceLock::new();
        EMPTY
            .get_or_init(|| {
                Arc::new(Self {
                    chunk: Arc::from(""),
                    prefix: None,
                    len: 0,
                })
            })
            .clone()
    }

    /// Single-chunk block holding exactly `text`.
    pub fn from_text(text: &str) -> Arc<Self> {
        if text.is_empty() {
            return Self::empty();
        }
        Arc::new(Self {
            chunk: Arc::from(text),
            prefix: None,
            len: text.len(),
        })
    }

    /// Single-chunk block that takes over an already shared text without copying it again.
    ///
    /// A producer that rolled its bounded window over already holds the tail as a shared
    /// `Arc<str>`; wrapping it here keeps that one allocation instead of materializing the same
    /// bytes a second time.
    pub fn from_shared(text: Arc<str>) -> Arc<Self> {
        if text.is_empty() {
            return Self::empty();
        }
        let len = text.len();
        Arc::new(Self {
            chunk: text,
            prefix: None,
            len,
        })
    }

    /// Observation of `delta` appended after every byte `block` already observed.
    pub fn append(block: &Arc<Self>, delta: &str) -> Arc<Self> {
        if delta.is_empty() {
            return block.clone();
        }
        Arc::new(Self {
            chunk: Arc::from(delta),
            prefix: Some(block.clone()),
            len: block.len.saturating_add(delta.len()),
        })
    }

    /// Total observed bytes, including every shared prefix.
    pub fn len(&self) -> usize {
        self.len
    }

    /// Reports whether no byte has been observed yet.
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// Complete observed text.
    pub fn text(&self) -> String {
        self.suffix_since(0).unwrap_or_default()
    }

    /// Bytes observed after `delivered_bytes`.
    ///
    /// Returns `None` when `delivered_bytes` exceeds the observed length or is not a UTF-8 boundary
    /// of the observed text, so a misaligned offset is reported instead of panicking. The count
    /// alone does not prove the prefix is still valid; use [`ContentBlock::increment_since`] when a
    /// consumer must verify its baseline.
    pub fn suffix_since(&self, delivered_bytes: usize) -> Option<String> {
        if delivered_bytes > self.len {
            return None;
        }
        let mut chunks: Vec<&str> = Vec::new();
        let mut node = self;
        loop {
            let start = node.prefix.as_deref().map_or(0, ContentBlock::len);
            if delivered_bytes <= start {
                chunks.push(node.chunk.as_ref());
            } else {
                let offset = delivered_bytes - start;
                if offset > node.chunk.len() || !node.chunk.is_char_boundary(offset) {
                    return None;
                }
                chunks.push(&node.chunk[offset..]);
                break;
            }
            match node.prefix.as_deref() {
                Some(prefix) => node = prefix,
                None => break,
            }
        }
        chunks.reverse();
        Some(chunks.concat())
    }

    /// Records the whole observed block as a verifiable baseline for one consumer.
    pub fn baseline(block: &Arc<Self>) -> ContentBaseline {
        ContentBaseline {
            block: block.clone(),
        }
    }

    /// Bytes observed after the shared block `previous`, without materializing the text.
    ///
    /// Returns `Some` with the appended bytes while `previous` is still an ancestor of the shared
    /// prefix chain (an empty string means the observation did not change), and `None` when the
    /// chain no longer contains `previous` — for example after [`ObservedPart::authorized`] replaced
    /// the body, so a consumer must replace its whole copy instead of appending to a stale prefix.
    pub fn appended_since(block: &Arc<Self>, previous: &Arc<Self>) -> Option<String> {
        let mut node = block.clone();
        loop {
            if Arc::ptr_eq(&node, previous) {
                return block.suffix_since(previous.len);
            }
            node = node.prefix.clone()?;
        }
    }

    /// Byte-for-byte equality of two observed texts without materializing either.
    ///
    /// Two observations that share a block are equal by pointer. Otherwise the walk compares the
    /// non-empty chunks in order, so a semantic check on a cold path never allocates the whole text.
    pub fn text_eq(left: &Arc<Self>, right: &Arc<Self>) -> bool {
        if Arc::ptr_eq(left, right) {
            return true;
        }
        Self::chain_eq(left, right)
    }

    /// Non-empty chunks of the observed text in content order (root -> tip).
    ///
    /// The walk collects only the shared chunk pointers, so it never copies the text itself.
    fn chunks_in_order(&self) -> Vec<&str> {
        let mut chunks: Vec<&str> = Vec::new();
        let mut node = self;
        loop {
            if !node.chunk.is_empty() {
                chunks.push(node.chunk.as_ref());
            }
            match node.prefix.as_deref() {
                Some(prefix) => node = prefix,
                None => break,
            }
        }
        chunks.reverse();
        chunks
    }

    /// Byte-for-byte equality of two observed texts without materializing either.
    ///
    /// The two observations may have different chunk boundaries (one long chunk versus several
    /// appended ones), so the walk streams both byte runs and compares them in place. Nothing is
    /// allocated beyond the chunk pointers, keeping even a cold semantic comparison off the
    /// full-text copy path.
    fn chain_eq(left: &Self, right: &Self) -> bool {
        if left.len != right.len {
            return false;
        }
        let left_chunks = left.chunks_in_order();
        let right_chunks = right.chunks_in_order();
        let mut left_index = 0usize;
        let mut left_offset = 0usize;
        let mut right_index = 0usize;
        let mut right_offset = 0usize;
        loop {
            while left_index < left_chunks.len() && left_offset == left_chunks[left_index].len() {
                left_index += 1;
                left_offset = 0;
            }
            while right_index < right_chunks.len()
                && right_offset == right_chunks[right_index].len()
            {
                right_index += 1;
                right_offset = 0;
            }
            if left_index == left_chunks.len() || right_index == right_chunks.len() {
                return left_index == left_chunks.len() && right_index == right_chunks.len();
            }
            let left_run = &left_chunks[left_index][left_offset..];
            let right_run = &right_chunks[right_index][right_offset..];
            let take = left_run.len().min(right_run.len());
            if left_run.as_bytes()[..take] != right_run.as_bytes()[..take] {
                return false;
            }
            left_offset += take;
            right_offset += take;
        }
    }

    /// The tail of the observed text, at most `max_bytes` bytes, starting on a UTF-8 boundary.
    ///
    /// Only the chunks nearest the tip are read, so a live "latest bytes" projection never copies a
    /// long response.
    pub fn suffix(&self, max_bytes: usize) -> String {
        let want = max_bytes.min(self.len);
        if want == 0 {
            return String::new();
        }
        let mut chunks: Vec<&str> = Vec::new();
        let mut collected = 0usize;
        let mut node = self;
        loop {
            let chunk = node.chunk.as_ref();
            if !chunk.is_empty() {
                chunks.push(chunk);
                collected += chunk.len();
            }
            if collected >= want {
                break;
            }
            match node.prefix.as_deref() {
                Some(prefix) => node = prefix,
                None => break,
            }
        }
        chunks.reverse();
        let text: String = chunks.into_iter().collect();
        let mut start = text.len().saturating_sub(want);
        while start < text.len() && !text.is_char_boundary(start) {
            start += 1;
        }
        text[start..].to_owned()
    }

    /// The last non-empty line of the observed text, bounded to `max_bytes`.
    ///
    /// Only the tail chunks are read and a longer line is truncated on a UTF-8 boundary, so a
    /// latest-line label never copies the whole observed text.
    pub fn last_line(&self, max_bytes: usize) -> Option<String> {
        let tail = self.suffix(max_bytes);
        tail.lines()
            .rev()
            .find(|line| !line.trim().is_empty())
            .map(str::to_owned)
    }

    /// Increment to deliver relative to `baseline`, verified against the shared prefix chain.
    ///
    /// The baseline's own block must still be an ancestor of `block`; walking that chain proves the
    /// prefix was neither replaced nor reset, so a consumer never appends to a body that changed
    /// identity under the same item.
    pub fn increment_since(block: &Arc<Self>, baseline: &ContentBaseline) -> ContentIncrement {
        let mut node = block.clone();
        let mut chunks: Vec<Arc<str>> = Vec::new();
        loop {
            if Arc::ptr_eq(&node, &baseline.block) {
                break;
            }
            chunks.push(node.chunk.clone());
            match node.prefix.clone() {
                Some(prefix) => node = prefix,
                None => return ContentIncrement::Replace(block.text()),
            }
        }
        if chunks.is_empty() {
            return ContentIncrement::Current;
        }
        let mut text = String::with_capacity(chunks.iter().map(|chunk| chunk.len()).sum::<usize>());
        for chunk in chunks.iter().rev() {
            text.push_str(chunk.as_ref());
        }
        ContentIncrement::Append(text)
    }

    /// Existing shared block holding exactly the first `max_bytes` of the observed text.
    ///
    /// A block already within budget is returned unchanged. Otherwise the walk picks the deepest
    /// shared node whose observed length fits the budget and appends only the one partial chunk that
    /// follows it, so bounding a preview reuses the shared prefix instead of re-materializing the
    /// whole text. The returned count is the bytes kept, always ending on a UTF-8 boundary.
    pub fn prefix_at(block: &Arc<Self>, max_bytes: usize) -> (Arc<Self>, usize) {
        if block.len <= max_bytes {
            return (block.clone(), block.len);
        }
        let mut chain: Vec<Arc<ContentBlock>> = Vec::new();
        let mut node = block.clone();
        loop {
            let next = node.prefix.clone();
            chain.push(node);
            match next {
                Some(prefix) => node = prefix,
                None => break,
            }
        }
        // `chain[0]` is the tip and `chain.last()` is the root; content runs root -> tip.
        let mut base_index = chain.len();
        for (index, candidate) in chain.iter().enumerate() {
            if candidate.len <= max_bytes {
                base_index = index;
                break;
            }
        }
        if base_index == chain.len() {
            // The budget is smaller than the first chunk: keep a prefix of the root chunk alone.
            let root = &chain[chain.len() - 1];
            let end = bounded_char_len(&root.chunk, max_bytes);
            return (
                ContentBlock::append(&ContentBlock::empty(), &root.chunk[..end]),
                end,
            );
        }
        let base = &chain[base_index];
        if base.len == max_bytes {
            return (base.clone(), max_bytes);
        }
        // The chunk that follows `base` is held by its child, one step back toward the tip.
        let child = &chain[base_index - 1];
        let end = bounded_char_len(&child.chunk, max_bytes - base.len);
        let kept = base.len + end;
        (ContentBlock::append(base, &child.chunk[..end]), kept)
    }

    /// Length of the observed prefix when `text` starts with exactly the whole observed text.
    ///
    /// Comparison walks the shared chain instead of materializing the observed text, so re-sending
    /// the provider's authoritative body does not copy the text that was already observed.
    fn extended_prefix(&self, text: &str) -> Option<usize> {
        let observed = self.len;
        if text.len() < observed {
            return None;
        }
        let mut node = self;
        loop {
            let start = node.prefix.as_deref().map_or(0, ContentBlock::len);
            if text.get(start..node.len) != Some(&*node.chunk) {
                return None;
            }
            match node.prefix.as_deref() {
                Some(prefix) => node = prefix,
                None => break,
            }
        }
        Some(observed)
    }
}

/// Largest byte length no greater than `max_bytes` that ends on a UTF-8 boundary of `text`.
fn bounded_char_len(text: &str, max_bytes: usize) -> usize {
    let mut end = max_bytes.min(text.len());
    while end > 0 && !text.is_char_boundary(end) {
        end -= 1;
    }
    end
}

impl Drop for ContentBlock {
    fn drop(&mut self) {
        // Break the shared prefix chain iteratively; a recursive drop would recurse once per
        // observed delta and overflow the stack for a long response.
        let mut next = self.prefix.take();
        while let Some(node) = next {
            match Arc::try_unwrap(node) {
                Ok(mut node) => next = node.prefix.take(),
                Err(_shared) => break,
            }
        }
    }
}

impl PartialEq for ContentBlock {
    fn eq(&self, other: &Self) -> bool {
        // Compare through the shared prefix chain and never through `text()`: this runs on hot
        // validation paths (for example an identical revision re-published), and materializing both
        // full bodies would reintroduce the per-token full copy this representation removes.
        self.len == other.len && (std::ptr::eq(self, other) || Self::chain_eq(self, other))
    }
}

impl Eq for ContentBlock {}

impl fmt::Debug for ContentBlock {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ContentBlock")
            .field("len", &self.len)
            .finish_non_exhaustive()
    }
}

impl Serialize for ContentBlock {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.text())
    }
}

impl<'de> Deserialize<'de> for ContentBlock {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let text = String::deserialize(deserializer)?;
        Ok(Self {
            chunk: Arc::from(text.as_str()),
            prefix: None,
            len: text.len(),
        })
    }
}

/// One live observation: a stable identity with a monotonic content version and a shared block.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ObservedPart {
    identity: ObservedPartIdentity,
    version: u64,
    content: Arc<ContentBlock>,
}

impl ObservedPart {
    /// First observation of one identity; content starts empty and the version starts at zero.
    pub fn new(identity: ObservedPartIdentity) -> Self {
        Self {
            identity,
            version: 0,
            content: ContentBlock::empty(),
        }
    }

    /// Stable identity this observation belongs to.
    pub fn identity(&self) -> &ObservedPartIdentity {
        &self.identity
    }

    /// Content version, advanced by every observation of this identity.
    pub fn version(&self) -> u64 {
        self.version
    }

    /// Shared immutable content block; cloning it is a pointer clone.
    pub fn content(&self) -> &Arc<ContentBlock> {
        &self.content
    }

    /// Bytes currently observed for this identity.
    pub fn len(&self) -> usize {
        self.content.len()
    }

    /// Reports whether no content has been observed for this identity yet.
    pub fn is_empty(&self) -> bool {
        self.content.is_empty()
    }

    /// Complete text observed for this identity.
    pub fn text(&self) -> String {
        self.content.text()
    }

    /// Bytes observed after `delivered_bytes` of this identity's text.
    ///
    /// Returns `None` for an out-of-range or non-boundary offset. The count alone does not prove the
    /// prefix still belongs to this body; use [`ObservedPart::increment_since`] to verify a
    /// consumer's baseline.
    pub fn suffix_since(&self, delivered_bytes: usize) -> Option<String> {
        self.content.suffix_since(delivered_bytes)
    }

    /// Records the whole currently observed text as a verifiable consumer baseline.
    pub fn baseline(&self) -> ContentBaseline {
        ContentBlock::baseline(&self.content)
    }

    /// Increment to deliver relative to `baseline`, verified against the shared prefix chain.
    ///
    /// A baseline whose block is no longer an ancestor — for example after
    /// [`ObservedPart::authorized`] replaced the body under the same identity — reports
    /// [`ContentIncrement::Replace`] instead of appending new bytes to the stale text.
    pub fn increment_since(&self, baseline: &ContentBaseline) -> ContentIncrement {
        ContentBlock::increment_since(&self.content, baseline)
    }

    /// Part discriminator of a provider item observation; `None` for an aggregate observation.
    pub fn presentation_part(&self) -> Option<PresentationPart> {
        match &self.identity {
            ObservedPartIdentity::Aggregate { .. } => None,
            ObservedPartIdentity::Provider(identity) => Some(identity.presentation_part()),
        }
    }

    /// Same identity observing `delta` appended; the content version advances by one.
    pub fn appended(&self, delta: &str) -> Self {
        if delta.is_empty() {
            return self.clone();
        }
        Self {
            identity: self.identity.clone(),
            version: self.version.saturating_add(1),
            content: ContentBlock::append(&self.content, delta),
        }
    }

    /// Same identity whose content becomes the provider's authoritative text for this part.
    ///
    /// Text that extends what was already observed keeps the shared prefix and appends only the
    /// missing bytes; anything else replaces the observed text instead of keeping two sources. The
    /// comparison walks the shared chain, so this boundary never materializes the observed text just
    /// to compare it, and a caller that repeats the same authoritative body is a no-op.
    pub fn authorized(&self, text: &str) -> Self {
        match self.content.extended_prefix(text) {
            Some(observed) => {
                let missing = &text[observed..];
                if missing.is_empty() {
                    return self.clone();
                }
                Self {
                    identity: self.identity.clone(),
                    version: self.version.saturating_add(1),
                    content: ContentBlock::append(&self.content, missing),
                }
            }
            None => Self {
                identity: self.identity.clone(),
                version: self.version.saturating_add(1),
                content: ContentBlock::from_text(text),
            },
        }
    }

    /// Same identity and content carrying provider metadata learned after streaming started.
    ///
    /// The output index and item kind are not part of the identity, so a late value only completes
    /// the metadata of the existing observation and never starts a second part or rewrites the
    /// observed body. A `None` argument keeps the value already known.
    pub fn with_metadata(
        &self,
        output_index: Option<u32>,
        item_kind: Option<ObservedItemKind>,
    ) -> Self {
        let ObservedPartIdentity::Provider(identity) = &self.identity else {
            return self.clone();
        };
        let output_index = output_index.or(identity.output_index);
        let item_kind = item_kind.unwrap_or(identity.item_kind);
        if output_index == identity.output_index && item_kind == identity.item_kind {
            return self.clone();
        }
        Self {
            identity: ObservedPartIdentity::Provider(ProviderPartIdentity {
                output_index,
                item_kind,
                ..identity.clone()
            }),
            version: self.version,
            content: self.content.clone(),
        }
    }
}

/// Number of live observations held in one shared chunk of [`ModelParts`].
///
/// A chunk is the unit one live edit reproduces: an edit copies at most the chunk its identity lands
/// in, so keeping a held snapshot immutable no longer costs the whole list.
const MODEL_PARTS_CHUNK: usize = 64;

/// The live observations of one request, shared in immutable fixed-size chunks.
///
/// The list keeps its observations in chunks a snapshot shares, and holds the one chunk still being
/// appended to on its own. Appending or updating one observation therefore reproduces at most the
/// chunk it lands in, plus the small chunk index when the edit lands in an already-filled chunk (or
/// when the still-growing chunk rolls over into one) — so a consumer that still holds an earlier
/// frame keeps exactly the observations it saw without the producer re-copying every part on each
/// event. A chunk an edit does not touch is never reproduced, and the chunk index is a pointer array
/// with one entry per chunk, far smaller than the observations it indexes.
///
/// This is the container handed to [`ModelProgressSender::edit_parts`] and read back through
/// [`ModelProgress::parts`]. It is an implementation detail of the snapshot: iteration, indexing,
/// length and the serialized shape are those of a plain ordered list of [`ObservedPart`].
#[derive(Debug, Clone, Default)]
pub struct ModelParts {
    /// Full chunks of exactly one chunk's worth of observations, in order.
    filled: Arc<Vec<Arc<[ObservedPart]>>>,
    /// The observations after `filled`.
    ///
    /// Never longer than one chunk; once it holds a full chunk's worth, the next append moves it
    /// into `filled` and starts a new tail.
    tail: Arc<Vec<ObservedPart>>,
}

impl ModelParts {
    /// Splits one owned part list into shared chunks.
    fn from_vec(parts: Vec<ObservedPart>) -> Self {
        let mut filled = Vec::with_capacity(parts.len() / MODEL_PARTS_CHUNK);
        let mut tail = Vec::with_capacity(MODEL_PARTS_CHUNK);
        for part in parts {
            tail.push(part);
            if tail.len() == MODEL_PARTS_CHUNK {
                let chunk = std::mem::take(&mut tail).into_boxed_slice();
                filled.push(Arc::<[ObservedPart]>::from(chunk));
            }
        }
        Self {
            filled: Arc::new(filled),
            tail: Arc::new(tail),
        }
    }

    /// Number of observations currently in the list.
    pub fn len(&self) -> usize {
        self.filled.len() * MODEL_PARTS_CHUNK + self.tail.len()
    }

    /// Reports whether the list holds no observation.
    pub fn is_empty(&self) -> bool {
        self.filled.is_empty() && self.tail.is_empty()
    }

    /// The observation at `index`, or `None` when the list is shorter.
    pub fn get(&self, index: usize) -> Option<&ObservedPart> {
        let filled = self.filled.len() * MODEL_PARTS_CHUNK;
        if index < filled {
            self.filled
                .get(index / MODEL_PARTS_CHUNK)?
                .get(index % MODEL_PARTS_CHUNK)
        } else {
            self.tail.get(index - filled)
        }
    }

    /// Iterates the observations in list order; the iterator is double-ended.
    pub fn iter(&self) -> ModelPartsIter<'_> {
        ModelPartsIter {
            parts: self,
            front: 0,
            back: self.len(),
        }
    }

    /// Appends one observation to the end of the list.
    ///
    /// Only this method grows the list, so the indices already handed out stay stable. It touches
    /// the tail chunk, and the chunk index only when the tail fills and becomes a shared chunk.
    pub fn push(&mut self, part: ObservedPart) {
        if self.tail.len() == MODEL_PARTS_CHUNK {
            let full = std::mem::take(Arc::make_mut(&mut self.tail)).into_boxed_slice();
            Arc::make_mut(&mut self.filled).push(Arc::<[ObservedPart]>::from(full));
        }
        Arc::make_mut(&mut self.tail).push(part);
    }

    /// Keeps only the observations the predicate accepts, preserving list order.
    pub fn retain(&mut self, mut keep: impl FnMut(&ObservedPart) -> bool) {
        let kept = self.iter().filter(|part| keep(part)).cloned().collect();
        *self = Self::from_vec(kept);
    }

    /// The observation at `index` for in-place editing, or `None` when the list is shorter.
    ///
    /// Editing one observation reproduces only the chunk it belongs to, plus the chunk index when it
    /// belongs to an already-filled chunk; every other chunk stays shared with the snapshots that
    /// already hold it.
    fn get_mut(&mut self, index: usize) -> Option<&mut ObservedPart> {
        let filled = self.filled.len() * MODEL_PARTS_CHUNK;
        if index < filled {
            let chunk = Arc::make_mut(&mut self.filled).get_mut(index / MODEL_PARTS_CHUNK)?;
            Arc::make_mut(chunk).get_mut(index % MODEL_PARTS_CHUNK)
        } else {
            Arc::make_mut(&mut self.tail).get_mut(index - filled)
        }
    }
}

/// Double-ended iterator over the observations of a [`ModelParts`].
#[derive(Debug, Clone)]
pub struct ModelPartsIter<'a> {
    parts: &'a ModelParts,
    front: usize,
    back: usize,
}

impl<'a> Iterator for ModelPartsIter<'a> {
    type Item = &'a ObservedPart;

    fn next(&mut self) -> Option<Self::Item> {
        if self.front == self.back {
            return None;
        }
        let parts = self.parts;
        let index = self.front;
        self.front += 1;
        parts.get(index)
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        let remaining = self.back - self.front;
        (remaining, Some(remaining))
    }
}

impl DoubleEndedIterator for ModelPartsIter<'_> {
    fn next_back(&mut self) -> Option<Self::Item> {
        if self.front == self.back {
            return None;
        }
        let parts = self.parts;
        self.back -= 1;
        parts.get(self.back)
    }
}

impl ExactSizeIterator for ModelPartsIter<'_> {}

impl<'a> IntoIterator for &'a ModelParts {
    type Item = &'a ObservedPart;
    type IntoIter = ModelPartsIter<'a>;

    fn into_iter(self) -> Self::IntoIter {
        self.iter()
    }
}

impl std::ops::Index<usize> for ModelParts {
    type Output = ObservedPart;

    fn index(&self, index: usize) -> &Self::Output {
        self.get(index).expect("model part index out of range")
    }
}

impl std::ops::IndexMut<usize> for ModelParts {
    fn index_mut(&mut self, index: usize) -> &mut Self::Output {
        self.get_mut(index).expect("model part index out of range")
    }
}

impl Serialize for ModelParts {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_seq(self.iter())
    }
}

impl<'de> Deserialize<'de> for ModelParts {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        Vec::<ObservedPart>::deserialize(deserializer).map(Self::from_vec)
    }
}

/// Bounded authoritative live observation of the current request, separate from committed output.
///
/// The snapshot holds at most one current entry per reserved observation identity, and every entry
/// shares its immutable content block with the observations before it. A slow observer coalesces
/// through the watch channel and still reads the newest authoritative text: it recovers the part of
/// the text it has not delivered yet from the block — verified with
/// [`ObservedPart::increment_since`] against the shared prefix lineage — instead of replaying a
/// delta queue, and it never has to re-encode the text while the response is streaming.
/// Materializing the text happens once, at the boundary that encodes a failure receipt.
///
/// The part list lives in the shared chunked container [`ModelParts`]: the producer owns the only
/// live snapshot and edits it in place while the watch still holds its only reference, and the
/// moment an observer has cloned a snapshot the producer only reproduces the chunk the edit lands in
/// (plus the chunk index when that chunk is already filled), so that snapshot keeps the parts it saw
/// without the whole list being copied.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelProgress {
    /// Monotonic version of the whole observation; the newest published snapshot wins.
    #[serde(default)]
    version: u64,
    #[serde(default, skip_serializing_if = "ModelParts::is_empty")]
    parts: ModelParts,
}

impl ModelProgress {
    /// Snapshot of the current observations at one monotonic version.
    pub fn new(version: u64, parts: Vec<ObservedPart>) -> Self {
        Self {
            version,
            parts: ModelParts::from_vec(parts),
        }
    }

    /// Monotonic version of this observation snapshot.
    pub fn version(&self) -> u64 {
        self.version
    }

    /// Current observations, at most one entry per identity.
    ///
    /// The returned container shares its unchanged chunks with every snapshot that has not edited
    /// them, so cloning a snapshot is a pointer copy rather than a copy of every part.
    pub fn parts(&self) -> &ModelParts {
        &self.parts
    }

    /// Reports whether no part has been observed yet.
    pub fn is_empty(&self) -> bool {
        self.parts.is_empty()
    }

    /// Number of currently observed identities.
    pub fn len(&self) -> usize {
        self.parts.len()
    }

    /// Current observation of one provider item part, if it is still in this preview.
    pub fn observed_part(&self, item_id: &str, part: PresentationPart) -> Option<&ObservedPart> {
        self.parts
            .iter()
            .find(|observed| match observed.identity() {
                ObservedPartIdentity::Provider(identity) => {
                    identity.item_id.as_ref() == item_id && identity.presentation_part() == part
                }
                ObservedPartIdentity::Aggregate { .. } => false,
            })
    }

    /// Current aggregate observation of one channel, if the adapter streams without item boundaries.
    pub fn channel(&self, channel: AggregateChannel) -> Option<&ObservedPart> {
        self.parts.iter().find(|observed| {
            matches!(
                observed.identity(),
                ObservedPartIdentity::Aggregate { channel: observed } if *observed == channel
            )
        })
    }
}

/// Largest live output one running operation may keep observed.
///
/// It bounds the resident live window, not the operation's canonical result: a producer that
/// reaches this ceiling rolls its window over and reports the shift as an explicit replacement
/// instead of silently dropping the head of an append-only body.
pub const MAX_TOOL_PROGRESS_BYTES: u64 = 64 * 1024;

/// Largest number of live output identities one running operation may keep observed.
pub const MAX_TOOL_PROGRESS_PARTS: usize = 64;

/// One live observed output part of a running operation, under a producer-chosen identity.
///
/// The identity is opaque to core: the producer names its own output domains (for example the
/// merged command stream) and core never parses the key, matches it against a tool's structure or
/// infers it from the text. Every observation of one identity shares the chunks observed before it,
/// so appending a delta costs one chunk and a new snapshot is a pointer clone. The shared prefix
/// chain is also the lineage a consumer verifies with
/// [`ToolProgressPart::increment_since`]: an append keeps the baseline an ancestor and reports only
/// the missing bytes, while a bounded window that dropped its head no longer contains the baseline
/// and reports a whole replacement instead.
#[derive(Debug, Clone)]
pub struct ToolProgressPart {
    id: Arc<str>,
    version: u64,
    content: Arc<ContentBlock>,
}

impl ToolProgressPart {
    /// First observation of `id`; content starts empty and the version starts at zero.
    pub fn new(id: impl Into<Arc<str>>) -> Self {
        Self {
            id: id.into(),
            version: 0,
            content: ContentBlock::empty(),
        }
    }

    /// Stable producer-chosen identity of this output part.
    pub fn id(&self) -> &str {
        &self.id
    }

    /// Content version, advanced by every accepted observation of this part.
    pub fn version(&self) -> u64 {
        self.version
    }

    /// Shared immutable content block; cloning it is a pointer clone.
    pub fn content(&self) -> &Arc<ContentBlock> {
        &self.content
    }

    /// Bytes currently observed for this part.
    pub fn len(&self) -> usize {
        self.content.len()
    }

    /// Reports whether no byte has been observed for this part yet.
    pub fn is_empty(&self) -> bool {
        self.content.is_empty()
    }

    /// Complete text observed for this part.
    pub fn text(&self) -> String {
        self.content.text()
    }

    /// Records the whole currently observed text as a verifiable consumer baseline.
    pub fn baseline(&self) -> ContentBaseline {
        ContentBlock::baseline(&self.content)
    }

    /// Increment to deliver relative to `baseline`, verified against the shared prefix chain.
    ///
    /// A baseline whose block is no longer an ancestor — for example after a bounded window was
    /// replaced — reports [`ContentIncrement::Replace`] instead of appending to a body that changed
    /// identity under the same part.
    pub fn increment_since(&self, baseline: &ContentBaseline) -> ContentIncrement {
        ContentBlock::increment_since(&self.content, baseline)
    }

    /// Same identity observing `update`; the content version advances by one real change.
    fn observed(&self, update: &ToolProgressUpdate) -> Self {
        match update {
            ToolProgressUpdate::Append { chunk, .. } => {
                if chunk.is_empty() {
                    return self.clone();
                }
                Self {
                    id: self.id.clone(),
                    version: self.version.saturating_add(1),
                    content: ContentBlock::append(&self.content, chunk),
                }
            }
            ToolProgressUpdate::Replace { text, .. } => {
                // Re-sending the same bounded window is not a new version. The comparison walks the
                // shared chain, so a genuinely different body never materializes the observed text
                // just to be compared.
                if text.len() == self.content.len()
                    && ContentBlock::text_eq(
                        &self.content,
                        &ContentBlock::from_shared(text.clone()),
                    )
                {
                    return self.clone();
                }
                Self {
                    id: self.id.clone(),
                    version: self.version.saturating_add(1),
                    content: ContentBlock::from_shared(text.clone()),
                }
            }
        }
    }
}

/// One increment a producer reports for its own live output.
///
/// The producer keeps the shared prefix chain and sends only what changed, so an append carries one
/// chunk instead of the whole accumulated text. `Replace` is how a bounded window states that its
/// body no longer extends the previous one: the consumer must take the whole text instead of
/// appending onto a prefix that changed identity.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ToolProgressUpdate {
    /// Bytes appended to `part`, keeping every byte already observed for it.
    Append { part: Arc<str>, chunk: Arc<str> },
    /// `part`'s authoritative live body becomes exactly `text`.
    Replace { part: Arc<str>, text: Arc<str> },
}

impl ToolProgressUpdate {
    /// Append `chunk` to the output part `part`.
    pub fn append(part: impl Into<Arc<str>>, chunk: impl Into<Arc<str>>) -> Self {
        Self::Append {
            part: part.into(),
            chunk: chunk.into(),
        }
    }

    /// Replace the whole live body of the output part `part` with `text`.
    pub fn replace(part: impl Into<Arc<str>>, text: impl Into<Arc<str>>) -> Self {
        Self::Replace {
            part: part.into(),
            text: text.into(),
        }
    }

    /// Output identity this increment belongs to.
    pub fn part(&self) -> &str {
        match self {
            Self::Append { part, .. } | Self::Replace { part, .. } => part,
        }
    }
}

/// Bounded authoritative live output of one running operation.
///
/// The snapshot holds at most one current entry per output identity, and every entry shares its
/// immutable content block with the observations before it. A slow consumer therefore recovers the
/// newest authoritative body from the block — verified with
/// [`ToolProgressPart::increment_since`] against the shared prefix lineage — instead of replaying a
/// delta queue, and it never has to re-encode the output while the operation is streaming. Text is
/// materialized once, at the boundary that encodes a canonical result or a durable receipt, and the
/// resident bytes are bounded by [`MAX_TOOL_PROGRESS_BYTES`].
#[derive(Debug, Clone, Default)]
pub struct ToolProgress {
    version: u64,
    parts: Arc<[ToolProgressPart]>,
}

impl ToolProgress {
    /// Snapshot of the current parts at one monotonic version.
    pub fn new(version: u64, parts: Vec<ToolProgressPart>) -> Self {
        Self {
            version,
            parts: parts.into(),
        }
    }

    /// Monotonic version of this observation snapshot.
    pub fn version(&self) -> u64 {
        self.version
    }

    /// Current output observations, at most one entry per identity.
    pub fn parts(&self) -> &[ToolProgressPart] {
        &self.parts
    }

    /// Reports whether no output identity is observed yet.
    pub fn is_empty(&self) -> bool {
        self.parts.is_empty()
    }

    /// Number of currently observed output identities.
    pub fn len(&self) -> usize {
        self.parts.len()
    }

    /// Current observation of one output identity.
    pub fn part(&self, id: &str) -> Option<&ToolProgressPart> {
        self.parts.iter().find(|part| part.id() == id)
    }

    /// Total accepted bytes across every part, without materializing any text.
    pub fn bytes(&self) -> u64 {
        self.parts
            .iter()
            .fold(0u64, |total, part| total.saturating_add(part.len() as u64))
    }

    /// Total accepted bytes after `update` would be applied, without applying it.
    ///
    /// The owner charges this total against the operation's reliable quota **before** it accepts
    /// the increment, so a refusal leaves the accepted parts exactly as they were.
    pub fn bytes_after(&self, update: &ToolProgressUpdate) -> u64 {
        match update {
            ToolProgressUpdate::Append { chunk, .. } => {
                self.bytes().saturating_add(chunk.len() as u64)
            }
            ToolProgressUpdate::Replace { text, .. } => {
                let current = self.part(update.part()).map_or(0, |part| part.len() as u64);
                self.bytes()
                    .saturating_sub(current)
                    .saturating_add(text.len() as u64)
            }
        }
    }

    /// Number of parts after `update` would be applied, without applying it.
    pub fn parts_after(&self, update: &ToolProgressUpdate) -> usize {
        if self.part(update.part()).is_some() {
            self.parts.len()
        } else {
            self.parts.len().saturating_add(1)
        }
    }

    /// Applies one accepted increment and advances the snapshot version by one real change.
    ///
    /// An update that does not change the observed body — an empty append, or the same bounded
    /// window reported twice — keeps this snapshot's version, so a consumer that compares versions
    /// never re-projects a body that did not move.
    pub fn applied(&self, update: &ToolProgressUpdate) -> Self {
        let id = update.part();
        let mut parts = self.parts.to_vec();
        let changed = match parts.iter_mut().find(|part| part.id() == id) {
            Some(part) => {
                let next = part.observed(update);
                let changed = next.version() != part.version();
                *part = next;
                changed
            }
            None => {
                let part = ToolProgressPart::new(id).observed(update);
                if part.is_empty() {
                    return self.clone();
                }
                parts.push(part);
                true
            }
        };
        if !changed {
            return self.clone();
        }
        Self {
            version: self.version.saturating_add(1),
            parts: parts.into(),
        }
    }

    /// Shared content block of the operation's whole live output, without materializing it.
    ///
    /// A single-part observation — the shape a command producer publishes — returns the part's
    /// block directly, so a live projection shares the same allocation instead of copying the text.
    /// A multi-part observation is concatenated once here; producers that publish one merged part
    /// never take that path.
    pub fn content(&self) -> Arc<ContentBlock> {
        match self.parts.as_ref() {
            [] => ContentBlock::empty(),
            [part] => part.content().clone(),
            parts => {
                let mut text =
                    String::with_capacity(parts.iter().map(ToolProgressPart::len).sum::<usize>());
                for part in parts {
                    text.push_str(&part.text());
                }
                ContentBlock::from_shared(Arc::from(text.as_str()))
            }
        }
    }
}

/// A bounded latest-value observer. Slow observers coalesce previews without blocking execution.
#[derive(Debug, Clone)]
pub struct ModelProgressSender {
    sender: tokio::sync::watch::Sender<ModelProgress>,
    reservation: Option<ModelProgressReservation>,
    output: Option<Arc<OutputBudget>>,
}

#[derive(Debug, Clone)]
struct ModelProgressReservation {
    store: crate::thread::cold::ColdStoreHandle,
    thread_id: String,
    attempt_id: String,
}

/// Reliable retention quota shared by one in-flight model call and its producer.
///
/// The quota is granted by the attached cold store out of the same budget the reliable save path
/// uses, so a call only starts when its streamed result could really be kept. The producer charges
/// what it has *already accepted*; a charge the quota cannot hold is refused before the increment is
/// applied, which is what lets the adapter cancel the call with the bytes it already has instead of
/// accumulating output this process could not retain. It is deliberately not the provider's own
/// output limit: the provider may report far more than the process can keep.
///
/// The accepted bytes are the real resident content of the live observation, which is also what the
/// terminal failure receipt materializes, so the receipt that reports a cancelled call is itself
/// covered by the reservation instead of being dropped by the same full budget.
#[derive(Debug)]
pub(crate) struct OutputBudget {
    store: Option<crate::thread::cold::ColdStoreHandle>,
    thread_id: String,
    operation_id: String,
    limit: u64,
    charged: std::sync::Mutex<u64>,
    refused: std::sync::Mutex<Option<(u64, u64)>>,
    /// Owner-owned wakeup the first refusal is reported through.
    ///
    /// The producer charges its output on the streaming task, not on the owner's stack, so the
    /// refusal cannot latch the typed fault directly. Reporting it here lets the owner observe the
    /// truncation the moment it happens through a channel it already owns, instead of only when the
    /// call returns — a tool that keeps running after its preview was refused would otherwise leave
    /// the fault and the admission block invisible for as long as it takes to wrap up. The owner is
    /// the single reader and publisher of the fault, so this is not a second source of truth.
    refusal: Option<OutputRefusalNotice>,
}

/// Timely, owner-owned notification that one in-flight operation exceeded its reliable output quota.
///
/// The value carries exactly the identity and the accepted/limit pair the owner latches, so the
/// owner never reconstructs the refusal from error text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct OutputRefusal {
    pub(crate) operation_id: String,
    pub(crate) accepted: u64,
    pub(crate) limit: u64,
}

/// Cloneable sender half of the owner's output-refusal wakeup.
///
/// The owner keeps the receiver and latches the typed fault from it; one clone rides every in-flight
/// operation's budget. A send with no receiver left is a no-op: the owner that would have acted on
/// it is gone, and the budget's own `refusal()` state is what the release path still reads.
#[derive(Debug, Clone)]
pub(crate) struct OutputRefusalNotice {
    sender: tokio::sync::watch::Sender<Option<OutputRefusal>>,
}

impl OutputRefusalNotice {
    pub(crate) fn new(sender: tokio::sync::watch::Sender<Option<OutputRefusal>>) -> Self {
        Self { sender }
    }

    fn report(&self, operation_id: &str, accepted: u64, limit: u64) {
        let _ = self.sender.send(Some(OutputRefusal {
            operation_id: operation_id.to_owned(),
            accepted,
            limit,
        }));
    }
}

impl OutputBudget {
    /// Reserves one operation's live-output quota from the backend's reliable budget.
    ///
    /// The ceiling is the smaller of the request and what the backend can really fund; nothing is
    /// charged until the producer reports accepted bytes, and the granted ceiling is charged at the
    /// first report. A backend without a reliable budget grants the request unchanged, and the
    /// ceiling itself still bounds the operation.
    pub(crate) fn reserve(
        store: Option<crate::thread::cold::ColdStoreHandle>,
        thread_id: &str,
        operation_id: &str,
        max_bytes: u64,
        refusal: Option<OutputRefusalNotice>,
    ) -> Result<Self, crate::thread::cold::ColdStoreError> {
        let limit = match &store {
            Some(store) => store.reserve_operation_output(thread_id, operation_id, max_bytes)?,
            None => max_bytes,
        };
        Ok(Self {
            store,
            thread_id: thread_id.to_owned(),
            operation_id: operation_id.to_owned(),
            limit: limit.min(max_bytes),
            charged: std::sync::Mutex::new(0),
            refused: std::sync::Mutex::new(None),
            refusal,
        })
    }

    /// Grants the producer's view of this quota.
    pub(crate) fn limit(&self) -> u64 {
        self.limit
    }

    /// Charges the total bytes the producer has accepted so far.
    ///
    /// Monotonic: a repeated or smaller report is a no-op, so a coalescing producer can report its
    /// current total without double charging. A refusal is remembered so the owner can report the
    /// exact accepted/limit pair that truncated the call, and it is returned to the producer as the
    /// typed reason to stop.
    pub(crate) fn charge(
        &self,
        accepted_bytes: u64,
    ) -> Result<(), crate::thread::cold::ColdStoreError> {
        let mut charged = self
            .charged
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if accepted_bytes <= *charged {
            return Ok(());
        }
        if accepted_bytes > self.limit {
            self.note_refusal(*charged);
            return Err(self.budget_error(*charged, accepted_bytes - *charged));
        }
        if let Some(store) = &self.store
            && let Err(error) =
                store.charge_operation_output(&self.thread_id, &self.operation_id, accepted_bytes)
        {
            self.note_refusal(*charged);
            return Err(error);
        }
        *charged = accepted_bytes;
        Ok(())
    }

    /// Reports the accepted/limit pair of a refused charge, if the operation was truncated.
    pub(crate) fn refusal(&self) -> Option<(u64, u64)> {
        *self
            .refused
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// Gives the remaining quota back once the call ended and its result was handed over.
    pub(crate) fn release(&self) {
        if let Some(store) = &self.store {
            store.release_operation_output(&self.thread_id, &self.operation_id);
        }
    }

    /// Remembers the first refusal so the owner reports the exact pair that truncated the call.
    fn note_refusal(&self, accepted: u64) {
        let mut refused = self
            .refused
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if refused.is_some() {
            return;
        }
        *refused = Some((accepted, self.limit));
        drop(refused);
        if let Some(refusal) = &self.refusal {
            refusal.report(&self.operation_id, accepted, self.limit);
        }
    }

    /// Typed refusal naming what was retained and what was rejected instead of retained.
    fn budget_error(&self, retained: u64, rejected: u64) -> crate::thread::cold::ColdStoreError {
        crate::thread::cold::ColdStoreError {
            source: Box::new(std::io::Error::other(format!(
                "model output exceeded the reliable budget: retained {retained} bytes of {}; the next {rejected} bytes were not retained",
                self.limit,
            ))),
        }
    }
}

impl ModelProgressSender {
    /// Creates the observation channel for one call that already holds its reliable output quota.
    pub(crate) fn channel(
        store: Option<crate::thread::cold::ColdStoreHandle>,
        thread_id: &str,
        attempt_id: &str,
        output: Arc<OutputBudget>,
    ) -> (Self, tokio::sync::watch::Receiver<ModelProgress>) {
        let (sender, receiver) = tokio::sync::watch::channel(ModelProgress::default());
        let reservation = store.map(|store| ModelProgressReservation {
            store,
            thread_id: thread_id.to_owned(),
            attempt_id: attempt_id.to_owned(),
        });
        (
            Self {
                sender,
                reservation,
                output: Some(output),
            },
            receiver,
        )
    }

    /// Charges the live observation's accepted bytes against the call's reserved reliable quota.
    ///
    /// The producer calls this before applying an increment, so a refusal leaves the accepted parts
    /// exactly as they were and the call can be cancelled with what it already had.
    ///
    /// # Errors
    /// Returns the typed storage refusal when the quota cannot hold the reported total.
    pub fn charge_output(
        &self,
        accepted_bytes: u64,
    ) -> Result<(), crate::thread::cold::ColdStoreError> {
        match &self.output {
            Some(output) => output.charge(accepted_bytes),
            None => Ok(()),
        }
    }

    /// Reserved ceiling of this call's live output, `None` when no quota was granted.
    pub fn output_limit(&self) -> Option<u64> {
        self.output.as_ref().map(|output| output.limit())
    }

    /// Reserves one raw presentation identity before its preview is coalesced or truncated.
    ///
    /// # Errors
    /// Returns the attached cold store's synchronous reservation failure.
    pub fn reserve_observed_item(
        &self,
        provider_item_id: &str,
        part: Option<crate::chat::PresentationPart>,
    ) -> Result<(), crate::thread::cold::ColdStoreError> {
        if let Some(reservation) = &self.reservation {
            let item_id =
                crate::chat::presentation_item_id(&reservation.attempt_id, provider_item_id, part);
            reservation
                .store
                .reserve_observed_item(&reservation.thread_id, &item_id)?;
        }
        Ok(())
    }
    /// Publishes the newest observation snapshot, replacing the previous one.
    ///
    /// A slow consumer therefore observes the latest authoritative text rather than a replayed
    /// delta queue, and publishing cannot advance a Thread context or grant execution authority.
    pub fn publish(&self, progress: ModelProgress) {
        self.sender.send_replace(progress);
    }

    /// Applies one in-place edit to the newest observation snapshot, notifying observers once.
    ///
    /// The producer owns the only live snapshot, so this edits the stored part list in place while
    /// the watch still holds its only reference. The moment a consumer has cloned a snapshot the
    /// list is shared, so the edit reproduces only the chunk it changes with copy-on-write — plus the
    /// small chunk index when it changes an already-filled chunk — and the cloned snapshot keeps
    /// exactly the parts it saw. This removes the per-event copy of the *whole* list, not all
    /// copying: a consumer that keeps a clone of every frame can still force a copy of the chunk its
    /// frame shares, and nothing here promises a measured performance.
    ///
    /// # Contract
    /// `edit` runs on the caller's task **inside the watch's write lock**, so it must not borrow the
    /// same watch again: calling [`Self::publish`], [`Self::latest`], [`Self::edit_parts`] or
    /// anything else that reads or writes this watch from within the closure re-enters the lock and
    /// deadlocks. It must stay synchronous and bounded — no blocking, no I/O, no waiting on another
    /// task. Whatever the closure writes to the list is exactly what observers see.
    ///
    /// The snapshot version advances by one and observers are notified once for *every* call,
    /// including a call whose closure refuses the change (for example a charge that rejects the next
    /// bytes): the accepted content is whatever the closure left in place, but the version and the
    /// notification are not tied to an accepted change.
    pub fn edit_parts<R>(&self, edit: impl FnOnce(&mut ModelParts) -> R) -> R {
        let mut result = None;
        self.sender.send_modify(|progress| {
            progress.version = progress.version.saturating_add(1);
            result = Some(edit(&mut progress.parts));
        });
        result.expect("the edit runs exactly once")
    }

    /// Captures the last published preview for a terminal failure receipt.
    pub fn latest(&self) -> ModelProgress {
        self.sender.borrow().clone()
    }
}

/// Current request identity supplied by the framework with its last observed preview.
#[derive(Debug, Clone)]
pub struct ActiveModelProgress {
    pub attempt_id: String,
    pub progress: ModelProgress,
}

/// An executable local call normalized by the model implementation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelToolCall {
    pub call_id: String,
    pub tool_id: String,
    pub arguments: OpaquePayload,
}

/// A proposed step. The Thread commits its content and private context together.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelStepOutput {
    pub attempt_id: String,
    pub base_context_revision: u64,
    pub content: Vec<ContextContent>,
    pub tool_calls: Vec<ModelToolCall>,
    pub private_context: Option<OpaquePayload>,
    pub usage: ModelUsage,
}

/// Service-reported counters. Cache values are subsets of total input, not additional input.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelUsage {
    pub input_tokens: Option<u64>,
    pub cache_read_tokens: Option<u64>,
    pub cache_write_tokens: Option<u64>,
    pub output_tokens: Option<u64>,
    pub reasoning_tokens: Option<u64>,
}

/// Portable failure classes. Provider-specific details are retained as the error source.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ModelFailureKind {
    Cancelled,
    UnsupportedContent,
    IncompatibleContext,
    ContextLimit,
    Unavailable,
    InvalidResponse,
    ImplementationPanicked,
}

/// Invocation failure with usage observed before termination.
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelError {
    /// Producer-owned facts observed on failure, including optional accounting or protocol material.
    #[serde(default)]
    pub details: Option<Box<OpaquePayload>>,
    pub kind: ModelFailureKind,
    pub usage: ModelUsage,
    /// Formats without an explicit null (TOML state checkpoints) omit an absent source, so the
    /// field must default instead of requiring it during decode.
    #[serde(default)]
    #[serde(with = "crate::error_record::optional")]
    pub source: Option<Box<dyn std::error::Error + Send + Sync>>,
}

impl std::fmt::Display for ModelError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "model call failed: {:?}", self.kind)?;
        if let Some(source) = &self.source {
            write!(formatter, ": {source}")?;
        }
        Ok(())
    }
}

impl std::error::Error for ModelError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        self.source.as_deref().map(|error| error as _)
    }
}

impl ModelError {
    fn from_panic(source: crate::error_record::BoundaryPanic) -> Self {
        Self {
            kind: ModelFailureKind::ImplementationPanicked,
            details: None,
            usage: ModelUsage::default(),
            source: Some(Box::new(source)),
        }
    }

    /// Diagnostic source-chain text of this failure, exactly as serialization records it.
    ///
    /// The persisted form of a `ModelError` keeps only this text — never the provider or plugin type
    /// that produced it — so two failures with equal text serialize identically.
    pub(crate) fn source_chain(&self) -> Vec<String> {
        match &self.source {
            Some(source) => crate::error_record::chain_text(source.as_ref()),
            None => Vec::new(),
        }
    }

    /// Copy of this failure with replaced `details` and `source`, preserving kind and usage.
    ///
    /// `ModelError` owns a `Box<dyn Error>` and is deliberately not `Clone`; a copy is rebuilt from
    /// the two things the persisted form keeps, so the copy persists and reports exactly like the
    /// original while the live owner keeps the untouched value. Callers pass the source as its
    /// portable chain (see [`crate::error_record::chain_source`]) instead of the original type.
    pub(crate) fn with_parts(
        &self,
        details: Option<Box<OpaquePayload>>,
        source: Option<Box<dyn std::error::Error + Send + Sync>>,
    ) -> Self {
        Self {
            details,
            kind: self.kind,
            usage: self.usage.clone(),
            source,
        }
    }
}

/// Accuracy declared by the model adapter's token estimator.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum EstimateAccuracy {
    Exact,
    Approximate,
}

/// Model-supplied input token estimate. Absence remains unknown, not zero.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TokenEstimate {
    pub tokens: u64,
    pub accuracy: EstimateAccuracy,
}

#[derive(Default)]
struct SessionHealth {
    poisoned: AtomicBool,
    closing: AtomicBool,
}
impl SessionHealth {
    fn ensure_available(&self) -> Result<(), ModelError> {
        if self.poisoned.load(Ordering::Acquire) || self.closing.load(Ordering::Acquire) {
            Err(ModelError {
                details: None,
                kind: ModelFailureKind::Unavailable,
                usage: ModelUsage::default(),
                source: Some(Box::new(std::io::Error::other(
                    "model session requires replacement or is closing",
                ))),
            })
        } else {
            Ok(())
        }
    }
}

/// One-shot invocation bound to the implementation that prepared its request.
///
/// Preparation cannot start unowned model work. Executing consumes this handle; a second
/// dispatch requires a separately identified attempt. Dropping an unstarted call drops its future.
pub struct PreparedModelCall {
    request_metadata: Option<OpaquePayload>,
    tool_projection: Option<OpaquePayload>,
    session_health: Option<Arc<SessionHealth>>,
    input_estimate: Option<TokenEstimate>,
    future: BoxFuture<'static, Result<ModelStepOutput, ModelError>>,
}

impl PreparedModelCall {
    /// Wraps a prepared, cancellation-aware operation without polling it.
    pub fn new(
        future: impl Future<Output = Result<ModelStepOutput, ModelError>> + Send + 'static,
    ) -> Self {
        Self {
            future: Box::pin(future),
            input_estimate: None,
            session_health: None,
            tool_projection: None,
            request_metadata: None,
        }
    }

    /// Freezes adapter-owned request provenance without exposing provider types to core.
    pub fn with_request_metadata(mut self, metadata: OpaquePayload) -> Self {
        self.request_metadata = Some(metadata);
        self
    }

    /// Returns the metadata of this exact prepared request.
    pub fn request_metadata(&self) -> Option<&OpaquePayload> {
        self.request_metadata.as_ref()
    }

    /// Freezes model-owned materials for tools produced by this invocation. Core does not decode them.
    pub fn with_tool_projection(mut self, projection: OpaquePayload) -> Self {
        self.tool_projection = Some(projection);
        self
    }

    /// Returns the exact tool projection materials associated with this prepared adapter.
    pub fn tool_projection(&self) -> Option<&OpaquePayload> {
        self.tool_projection.as_ref()
    }

    /// Attaches the estimate for this exact prepared input, not a separately reconstructed request.
    pub fn with_input_estimate(mut self, estimate: TokenEstimate) -> Self {
        self.input_estimate = Some(estimate);
        self
    }

    /// Returns the adapter's estimate and declared precision for the frozen input.
    pub fn input_estimate(&self) -> Option<TokenEstimate> {
        self.input_estimate
    }

    /// Dispatches this attempt exactly once.
    ///
    /// # Errors
    /// Returns the implementation failure with any observed usage.
    pub async fn execute(self) -> Result<ModelStepOutput, ModelError> {
        if let Some(health) = &self.session_health {
            health.ensure_available()?;
        }
        match crate::error_record::catch_boundary("model execution", self.future).await {
            Ok(result) => result,
            Err(source) => {
                if let Some(health) = self.session_health {
                    health.poisoned.store(true, Ordering::Release);
                }
                Err(ModelError::from_panic(source))
            }
        }
    }
}

impl fmt::Debug for PreparedModelCall {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("PreparedModelCall")
            .finish_non_exhaustive()
    }
}

trait ErasedModelSession: Send {
    fn prepare(
        &mut self,
        request: ModelRequest,
    ) -> BoxFuture<'_, Result<PreparedModelCall, ModelError>>;
    fn close(&mut self) -> BoxFuture<'_, Result<(), ModelError>>;
}

impl<T: ModelSession> ErasedModelSession for T {
    fn prepare(
        &mut self,
        request: ModelRequest,
    ) -> BoxFuture<'_, Result<PreparedModelCall, ModelError>> {
        Box::pin(ModelSession::prepare(self, request))
    }

    fn close(&mut self) -> BoxFuture<'_, Result<(), ModelError>> {
        Box::pin(ModelSession::close(self))
    }
}

/// Owned erasure for one serial model session, deliberately not Clone or Sync.
pub struct DynModelSession {
    health: Arc<SessionHealth>,
    inner: Box<dyn ErasedModelSession>,
}

impl DynModelSession {
    /// Takes ownership of a Thread-local implementation.
    pub fn new(session: impl ModelSession) -> Self {
        Self {
            inner: Box::new(session),
            health: Arc::new(SessionHealth::default()),
        }
    }

    pub(crate) fn is_available(&self) -> bool {
        self.health.ensure_available().is_ok()
    }

    /// Prepares the next immutable attempt.
    ///
    /// # Errors
    /// Returns invalid input, incompatible context, or preparation failure.
    pub async fn prepare(
        &mut self,
        request: ModelRequest,
    ) -> Result<PreparedModelCall, ModelError> {
        self.health.ensure_available()?;
        match crate::error_record::catch_boundary("model preparation", async {
            self.inner.prepare(request).await
        })
        .await
        {
            Ok(result) => result.map(|mut call| {
                call.session_health = Some(self.health.clone());
                call
            }),
            Err(source) => {
                self.health.poisoned.store(true, Ordering::Release);
                Err(ModelError::from_panic(source))
            }
        }
    }

    /// Closes resources before the Thread releases its model session.
    ///
    /// # Errors
    /// A failed close retains this owner so the caller can retry.
    pub async fn close(&mut self) -> Result<(), ModelError> {
        self.health.closing.store(true, Ordering::Release);
        crate::error_record::catch_boundary("model close", async { self.inner.close().await })
            .await
            .map_err(ModelError::from_panic)?
    }
}

impl fmt::Debug for DynModelSession {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("DynModelSession")
            .finish_non_exhaustive()
    }
}
