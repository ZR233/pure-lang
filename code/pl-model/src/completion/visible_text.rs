const COMMENTARY_OPEN_TAG: &str = "<commentary>";
const COMMENTARY_CLOSE_TAG: &str = "</commentary>";
const FINAL_OPEN_TAG: &str = "<final>";
const FINAL_CLOSE_TAG: &str = "</final>";

#[cfg(test)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VisibleTextSegment {
    Untagged(String),
    Commentary(String),
    Final(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VisibleTextKind {
    Commentary,
    Final,
}

impl VisibleTextKind {
    pub fn channel_label(self) -> &'static str {
        match self {
            Self::Commentary => "commentary",
            Self::Final => "final",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VisibleTextEvent {
    Untagged(String),
    Open(VisibleTextKind),
    Delta(VisibleTextKind, String),
    Close(VisibleTextKind),
}

/// Controls whether [`VisibleTextParser::drain_pending`] should finalize the chunk.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DrainMode {
    /// Flush pending content as a partial chunk.
    Partial,
    /// Flush and mark the chunk as final.
    Final,
}

#[cfg(test)]
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct VisibleTextChunk {
    pub segments: Vec<VisibleTextSegment>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct VisibleTextEventChunk {
    pub events: Vec<VisibleTextEvent>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ActiveTag {
    Commentary,
    Final,
}

impl ActiveTag {
    fn close_tag(self) -> &'static str {
        match self {
            Self::Commentary => COMMENTARY_CLOSE_TAG,
            Self::Final => FINAL_CLOSE_TAG,
        }
    }

    fn kind(self) -> VisibleTextKind {
        match self {
            Self::Commentary => VisibleTextKind::Commentary,
            Self::Final => VisibleTextKind::Final,
        }
    }

    #[cfg(test)]
    fn segment(self, text: String) -> VisibleTextSegment {
        match self {
            Self::Commentary => VisibleTextSegment::Commentary(text),
            Self::Final => VisibleTextSegment::Final(text),
        }
    }
}

#[derive(Debug, Clone, Copy)]
struct OpenTag {
    tag: ActiveTag,
    text: &'static str,
}

/// Parser for Codex-style visible output tags emitted by the model.
///
/// The parser accepts arbitrary stream chunk boundaries and removes channel
/// tags from visible text. Untagged text is reported separately so
/// Studio turns can reject provider output that does not follow the channel
/// protocol.
#[derive(Debug)]
pub struct VisibleTextParser {
    active_tag: Option<ActiveTag>,
    pending: String,
}

impl VisibleTextParser {
    pub fn new() -> Self {
        Self {
            active_tag: None,
            pending: String::new(),
        }
    }

    #[cfg(test)]
    pub fn push_str(&mut self, chunk: &str) -> VisibleTextChunk {
        self.pending.push_str(chunk);
        self.drain_pending(DrainMode::Partial)
    }

    #[cfg(test)]
    pub fn finish(&mut self) -> VisibleTextChunk {
        self.drain_pending(DrainMode::Final)
    }

    pub fn push_events(&mut self, chunk: &str) -> VisibleTextEventChunk {
        self.pending.push_str(chunk);
        self.drain_pending_events(DrainMode::Partial)
    }

    pub fn finish_events(&mut self) -> VisibleTextEventChunk {
        self.drain_pending_events(DrainMode::Final)
    }

    #[cfg(test)]
    fn drain_pending(&mut self, mode: DrainMode) -> VisibleTextChunk {
        let mut segments = Vec::new();
        loop {
            if let Some(active_tag) = self.active_tag {
                let close_tag = active_tag.close_tag();
                match self.pending.find(close_tag) {
                    Some(index) => {
                        if index > 0 {
                            segments.push(active_tag.segment(self.pending[..index].to_string()));
                        }
                        self.pending.drain(..index + close_tag.len());
                        self.active_tag = None;
                    }
                    None => {
                        let keep = if matches!(mode, DrainMode::Final) {
                            0
                        } else {
                            suffix_prefix_len(&self.pending, close_tag)
                        };
                        let emit_len = self.pending.len().saturating_sub(keep);
                        if emit_len > 0 {
                            segments.push(active_tag.segment(self.pending[..emit_len].to_string()));
                            self.pending.drain(..emit_len);
                        }
                        if matches!(mode, DrainMode::Final) {
                            if !self.pending.is_empty() {
                                segments.push(active_tag.segment(self.pending.clone()));
                                self.pending.clear();
                            }
                            self.active_tag = None;
                        }
                        break;
                    }
                }
            } else {
                let open_tags = self.open_tags();
                match earliest_open_tag(&self.pending, &open_tags) {
                    Some((index, open_tag)) => {
                        if index > 0 {
                            segments.push(VisibleTextSegment::Untagged(
                                self.pending[..index].to_string(),
                            ));
                        }
                        self.pending.drain(..index + open_tag.text.len());
                        self.active_tag = Some(open_tag.tag);
                    }
                    None => {
                        let keep = if matches!(mode, DrainMode::Final) {
                            0
                        } else {
                            max_suffix_prefix_len(
                                &self.pending,
                                open_tags.iter().map(|tag| tag.text),
                            )
                        };
                        let emit_len = self.pending.len().saturating_sub(keep);
                        if emit_len > 0 {
                            segments.push(VisibleTextSegment::Untagged(
                                self.pending[..emit_len].to_string(),
                            ));
                            self.pending.drain(..emit_len);
                        }
                        if matches!(mode, DrainMode::Final) && !self.pending.is_empty() {
                            segments.push(VisibleTextSegment::Untagged(self.pending.clone()));
                            self.pending.clear();
                        }
                        break;
                    }
                }
            }
        }
        VisibleTextChunk { segments }
    }

    fn drain_pending_events(&mut self, mode: DrainMode) -> VisibleTextEventChunk {
        let mut events = Vec::new();
        loop {
            if let Some(active_tag) = self.active_tag {
                let close_tag = active_tag.close_tag();
                match self.pending.find(close_tag) {
                    Some(index) => {
                        if index > 0 {
                            events.push(VisibleTextEvent::Delta(
                                active_tag.kind(),
                                self.pending[..index].to_string(),
                            ));
                        }
                        self.pending.drain(..index + close_tag.len());
                        self.active_tag = None;
                        events.push(VisibleTextEvent::Close(active_tag.kind()));
                    }
                    None => {
                        let keep = if matches!(mode, DrainMode::Final) {
                            0
                        } else {
                            suffix_prefix_len(&self.pending, close_tag)
                        };
                        let emit_len = self.pending.len().saturating_sub(keep);
                        if emit_len > 0 {
                            events.push(VisibleTextEvent::Delta(
                                active_tag.kind(),
                                self.pending[..emit_len].to_string(),
                            ));
                            self.pending.drain(..emit_len);
                        }
                        if matches!(mode, DrainMode::Final) {
                            if !self.pending.is_empty() {
                                events.push(VisibleTextEvent::Delta(
                                    active_tag.kind(),
                                    self.pending.clone(),
                                ));
                                self.pending.clear();
                            }
                            self.active_tag = None;
                            events.push(VisibleTextEvent::Close(active_tag.kind()));
                        }
                        break;
                    }
                }
            } else {
                let open_tags = self.open_tags();
                match earliest_open_tag(&self.pending, &open_tags) {
                    Some((index, open_tag)) => {
                        if index > 0 {
                            events.push(VisibleTextEvent::Untagged(
                                self.pending[..index].to_string(),
                            ));
                        }
                        self.pending.drain(..index + open_tag.text.len());
                        self.active_tag = Some(open_tag.tag);
                        events.push(VisibleTextEvent::Open(open_tag.tag.kind()));
                    }
                    None => {
                        let keep = if matches!(mode, DrainMode::Final) {
                            0
                        } else {
                            max_suffix_prefix_len(
                                &self.pending,
                                open_tags.iter().map(|tag| tag.text),
                            )
                        };
                        let emit_len = self.pending.len().saturating_sub(keep);
                        if emit_len > 0 {
                            events.push(VisibleTextEvent::Untagged(
                                self.pending[..emit_len].to_string(),
                            ));
                            self.pending.drain(..emit_len);
                        }
                        if matches!(mode, DrainMode::Final) && !self.pending.is_empty() {
                            events.push(VisibleTextEvent::Untagged(self.pending.clone()));
                            self.pending.clear();
                        }
                        break;
                    }
                }
            }
        }
        VisibleTextEventChunk { events }
    }

    fn open_tags(&self) -> Vec<OpenTag> {
        vec![
            OpenTag {
                tag: ActiveTag::Commentary,
                text: COMMENTARY_OPEN_TAG,
            },
            OpenTag {
                tag: ActiveTag::Final,
                text: FINAL_OPEN_TAG,
            },
        ]
    }
}

fn earliest_open_tag(text: &str, open_tags: &[OpenTag]) -> Option<(usize, OpenTag)> {
    open_tags
        .iter()
        .filter_map(|tag| text.find(tag.text).map(|index| (index, *tag)))
        .min_by_key(|(index, _)| *index)
}

fn max_suffix_prefix_len<'a>(text: &str, patterns: impl Iterator<Item = &'a str>) -> usize {
    patterns
        .map(|pattern| suffix_prefix_len(text, pattern))
        .max()
        .unwrap_or(0)
}

fn suffix_prefix_len(text: &str, pattern: &str) -> usize {
    let text = text.as_bytes();
    let pattern = pattern.as_bytes();
    let max = text.len().min(pattern.len().saturating_sub(1));
    for len in (1..=max).rev() {
        if text[text.len() - len..] == pattern[..len] {
            return len;
        }
    }
    0
}

#[cfg(test)]
mod tests {
    use super::{VisibleTextEvent, VisibleTextKind, VisibleTextParser, VisibleTextSegment};
    use pretty_assertions::assert_eq;

    fn collect(chunks: &[&str]) -> Vec<VisibleTextSegment> {
        let mut parser = VisibleTextParser::new();
        let mut segments = Vec::new();
        for chunk in chunks {
            segments.extend(parser.push_str(chunk).segments);
        }
        segments.extend(parser.finish().segments);
        segments
    }

    fn collect_events(chunks: &[&str]) -> Vec<VisibleTextEvent> {
        let mut parser = VisibleTextParser::new();
        let mut events = Vec::new();
        for chunk in chunks {
            events.extend(parser.push_events(chunk).events);
        }
        events.extend(parser.finish_events().events);
        events
    }

    #[test]
    fn extracts_channels_split_across_chunks() {
        assert_eq!(
            collect(&[
                "<comm",
                "entary>checking",
                "</commentary><fi",
                "nal>done</final>"
            ],),
            vec![
                VisibleTextSegment::Commentary("checking".to_string()),
                VisibleTextSegment::Final("done".to_string()),
            ]
        );
    }

    #[test]
    fn leaves_proposed_plan_as_untagged_text() {
        assert_eq!(
            collect(&[
                "before\n<prop",
                "osed_plan>\n- step\n",
                "</proposed_plan>\nafter"
            ]),
            vec![
                VisibleTextSegment::Untagged("before\n<prop".to_string()),
                VisibleTextSegment::Untagged("osed_plan>\n- step\n".to_string()),
                VisibleTextSegment::Untagged("</proposed_plan>\nafter".to_string()),
            ]
        );
    }

    #[test]
    fn closes_unterminated_final_on_finish() {
        assert_eq!(
            collect(&["<final>done"]),
            vec![VisibleTextSegment::Final("done".to_string())]
        );
    }

    #[test]
    fn event_parser_emits_open_delta_close_for_repeated_tags() {
        assert_eq!(
            collect_events(&[
                "<commentary>A</commentary><comm",
                "entary>B</commentary>",
                "<final>C"
            ]),
            vec![
                VisibleTextEvent::Open(VisibleTextKind::Commentary),
                VisibleTextEvent::Delta(VisibleTextKind::Commentary, "A".to_string()),
                VisibleTextEvent::Close(VisibleTextKind::Commentary),
                VisibleTextEvent::Open(VisibleTextKind::Commentary),
                VisibleTextEvent::Delta(VisibleTextKind::Commentary, "B".to_string()),
                VisibleTextEvent::Close(VisibleTextKind::Commentary),
                VisibleTextEvent::Open(VisibleTextKind::Final),
                VisibleTextEvent::Delta(VisibleTextKind::Final, "C".to_string()),
                VisibleTextEvent::Close(VisibleTextKind::Final),
            ]
        );
    }

    #[test]
    fn preserves_untagged_text_without_tags() {
        assert_eq!(
            collect(&["hello ", "world"]),
            vec![
                VisibleTextSegment::Untagged("hello ".to_string()),
                VisibleTextSegment::Untagged("world".to_string()),
            ]
        );
    }
}
