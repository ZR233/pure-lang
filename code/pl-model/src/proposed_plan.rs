const OPEN_TAG: &str = "<proposed_plan>";
const CLOSE_TAG: &str = "</proposed_plan>";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProposedPlanSegment {
    Normal(String),
    ProposedPlanStart,
    ProposedPlanDelta(String),
    ProposedPlanEnd,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ProposedPlanChunk {
    pub segments: Vec<ProposedPlanSegment>,
}

/// Parser for `<proposed_plan>` blocks emitted in Plan Mode.
///
/// The parser accepts arbitrary stream chunk boundaries and removes plan
/// blocks from normal assistant text while preserving the extracted plan text.
#[derive(Debug, Default)]
pub struct ProposedPlanParser {
    inside_plan: bool,
    pending: String,
}

impl ProposedPlanParser {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn push_str(&mut self, chunk: &str) -> ProposedPlanChunk {
        self.pending.push_str(chunk);
        self.drain_pending(false)
    }

    pub fn finish(&mut self) -> ProposedPlanChunk {
        self.drain_pending(true)
    }

    fn drain_pending(&mut self, finish: bool) -> ProposedPlanChunk {
        let mut segments = Vec::new();
        loop {
            if self.inside_plan {
                match self.pending.find(CLOSE_TAG) {
                    Some(index) => {
                        if index > 0 {
                            segments.push(ProposedPlanSegment::ProposedPlanDelta(
                                self.pending[..index].to_string(),
                            ));
                        }
                        self.pending.drain(..index + CLOSE_TAG.len());
                        self.inside_plan = false;
                        segments.push(ProposedPlanSegment::ProposedPlanEnd);
                    }
                    None => {
                        let keep = if finish {
                            0
                        } else {
                            suffix_prefix_len(&self.pending, CLOSE_TAG)
                        };
                        let emit_len = self.pending.len().saturating_sub(keep);
                        if emit_len > 0 {
                            segments.push(ProposedPlanSegment::ProposedPlanDelta(
                                self.pending[..emit_len].to_string(),
                            ));
                            self.pending.drain(..emit_len);
                        }
                        if finish {
                            if !self.pending.is_empty() {
                                segments.push(ProposedPlanSegment::ProposedPlanDelta(
                                    self.pending.clone(),
                                ));
                                self.pending.clear();
                            }
                            self.inside_plan = false;
                            segments.push(ProposedPlanSegment::ProposedPlanEnd);
                        }
                        break;
                    }
                }
            } else {
                match self.pending.find(OPEN_TAG) {
                    Some(index) => {
                        if index > 0 {
                            segments.push(ProposedPlanSegment::Normal(
                                self.pending[..index].to_string(),
                            ));
                        }
                        self.pending.drain(..index + OPEN_TAG.len());
                        self.inside_plan = true;
                        segments.push(ProposedPlanSegment::ProposedPlanStart);
                    }
                    None => {
                        let keep = if finish {
                            0
                        } else {
                            suffix_prefix_len(&self.pending, OPEN_TAG)
                        };
                        let emit_len = self.pending.len().saturating_sub(keep);
                        if emit_len > 0 {
                            segments.push(ProposedPlanSegment::Normal(
                                self.pending[..emit_len].to_string(),
                            ));
                            self.pending.drain(..emit_len);
                        }
                        if finish && !self.pending.is_empty() {
                            segments.push(ProposedPlanSegment::Normal(self.pending.clone()));
                            self.pending.clear();
                        }
                        break;
                    }
                }
            }
        }
        ProposedPlanChunk { segments }
    }
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
    use super::{ProposedPlanParser, ProposedPlanSegment};
    use pretty_assertions::assert_eq;

    fn collect(chunks: &[&str]) -> Vec<ProposedPlanSegment> {
        let mut parser = ProposedPlanParser::new();
        let mut segments = Vec::new();
        for chunk in chunks {
            segments.extend(parser.push_str(chunk).segments);
        }
        segments.extend(parser.finish().segments);
        segments
    }

    #[test]
    fn extracts_plan_split_across_chunks() {
        assert_eq!(
            collect(&[
                "before\n<prop",
                "osed_plan>\n- step\n",
                "</proposed_plan>\nafter"
            ]),
            vec![
                ProposedPlanSegment::Normal("before\n".to_string()),
                ProposedPlanSegment::ProposedPlanStart,
                ProposedPlanSegment::ProposedPlanDelta("\n- step\n".to_string()),
                ProposedPlanSegment::ProposedPlanEnd,
                ProposedPlanSegment::Normal("\nafter".to_string()),
            ]
        );
    }

    #[test]
    fn closes_unterminated_plan_block_on_finish() {
        assert_eq!(
            collect(&["<proposed_plan>\n- step\n"]),
            vec![
                ProposedPlanSegment::ProposedPlanStart,
                ProposedPlanSegment::ProposedPlanDelta("\n- step\n".to_string()),
                ProposedPlanSegment::ProposedPlanEnd,
            ]
        );
    }

    #[test]
    fn preserves_normal_text_without_tags() {
        assert_eq!(
            collect(&["hello ", "world"]),
            vec![
                ProposedPlanSegment::Normal("hello ".to_string()),
                ProposedPlanSegment::Normal("world".to_string()),
            ]
        );
    }
}
