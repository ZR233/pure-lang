const COMMENTARY_OPEN_TAG: &str = "<commentary>";
const COMMENTARY_CLOSE_TAG: &str = "</commentary>";
const FINAL_OPEN_TAG: &str = "<final>";
const FINAL_CLOSE_TAG: &str = "</final>";
const PLAN_OPEN_TAG: &str = "<proposed_plan>";
const PLAN_CLOSE_TAG: &str = "</proposed_plan>";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VisibleTextSegment {
    Untagged(String),
    Commentary(String),
    Final(String),
    ProposedPlan(String),
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct VisibleTextChunk {
    pub segments: Vec<VisibleTextSegment>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ActiveTag {
    Commentary,
    Final,
    ProposedPlan,
}

impl ActiveTag {
    fn close_tag(self) -> &'static str {
        match self {
            Self::Commentary => COMMENTARY_CLOSE_TAG,
            Self::Final => FINAL_CLOSE_TAG,
            Self::ProposedPlan => PLAN_CLOSE_TAG,
        }
    }

    fn segment(self, text: String) -> VisibleTextSegment {
        match self {
            Self::Commentary => VisibleTextSegment::Commentary(text),
            Self::Final => VisibleTextSegment::Final(text),
            Self::ProposedPlan => VisibleTextSegment::ProposedPlan(text),
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
/// tags from visible visible text. Untagged text is reported separately so
/// Studio turns can reject provider output that does not follow the channel
/// protocol.
#[derive(Debug)]
pub struct VisibleTextParser {
    allow_plan: bool,
    active_tag: Option<ActiveTag>,
    pending: String,
}

impl VisibleTextParser {
    pub fn new(allow_plan: bool) -> Self {
        Self {
            allow_plan,
            active_tag: None,
            pending: String::new(),
        }
    }

    pub fn push_str(&mut self, chunk: &str) -> VisibleTextChunk {
        self.pending.push_str(chunk);
        self.drain_pending(false)
    }

    pub fn finish(&mut self) -> VisibleTextChunk {
        self.drain_pending(true)
    }

    fn drain_pending(&mut self, finish: bool) -> VisibleTextChunk {
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
                        let keep = if finish {
                            0
                        } else {
                            suffix_prefix_len(&self.pending, close_tag)
                        };
                        let emit_len = self.pending.len().saturating_sub(keep);
                        if emit_len > 0 {
                            segments.push(active_tag.segment(self.pending[..emit_len].to_string()));
                            self.pending.drain(..emit_len);
                        }
                        if finish {
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
                        let keep = if finish {
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
                        if finish && !self.pending.is_empty() {
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

    fn open_tags(&self) -> Vec<OpenTag> {
        let mut tags = vec![
            OpenTag {
                tag: ActiveTag::Commentary,
                text: COMMENTARY_OPEN_TAG,
            },
            OpenTag {
                tag: ActiveTag::Final,
                text: FINAL_OPEN_TAG,
            },
        ];
        if self.allow_plan {
            tags.push(OpenTag {
                tag: ActiveTag::ProposedPlan,
                text: PLAN_OPEN_TAG,
            });
        }
        tags
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
    use super::{VisibleTextParser, VisibleTextSegment};
    use pretty_assertions::assert_eq;

    fn collect(chunks: &[&str], allow_plan: bool) -> Vec<VisibleTextSegment> {
        let mut parser = VisibleTextParser::new(allow_plan);
        let mut segments = Vec::new();
        for chunk in chunks {
            segments.extend(parser.push_str(chunk).segments);
        }
        segments.extend(parser.finish().segments);
        segments
    }

    #[test]
    fn extracts_channels_split_across_chunks() {
        assert_eq!(
            collect(
                &[
                    "<comm",
                    "entary>checking",
                    "</commentary><fi",
                    "nal>done</final>"
                ],
                false,
            ),
            vec![
                VisibleTextSegment::Commentary("checking".to_string()),
                VisibleTextSegment::Final("done".to_string()),
            ]
        );
    }

    #[test]
    fn extracts_plan_split_across_chunks() {
        assert_eq!(
            collect(
                &[
                    "before\n<prop",
                    "osed_plan>\n- step\n",
                    "</proposed_plan>\nafter"
                ],
                true,
            ),
            vec![
                VisibleTextSegment::Untagged("before\n".to_string()),
                VisibleTextSegment::ProposedPlan("\n- step\n".to_string()),
                VisibleTextSegment::Untagged("\nafter".to_string()),
            ]
        );
    }

    #[test]
    fn closes_unterminated_final_on_finish() {
        assert_eq!(
            collect(&["<final>done"], false),
            vec![VisibleTextSegment::Final("done".to_string())]
        );
    }

    #[test]
    fn preserves_untagged_text_without_tags() {
        assert_eq!(
            collect(&["hello ", "world"], false),
            vec![
                VisibleTextSegment::Untagged("hello ".to_string()),
                VisibleTextSegment::Untagged("world".to_string()),
            ]
        );
    }
}
