//! Deterministic name-and-description selection over a frozen Skill catalog.

use std::collections::HashSet;

use super::SkillMetadata;

const MAX_QUERY_BYTES: usize = 4 * 1024;
const MAX_QUERY_TERMS: usize = 64;
const MAX_DOCUMENT_BYTES: usize = 4 * 1024;
const MAX_DOCUMENT_TERMS: usize = 256;
const MAX_CANDIDATES: usize = 1_000;
const MAX_RESULTS: usize = 50;

const STOP_WORDS: &[&str] = &[
    "a", "an", "and", "are", "as", "at", "be", "by", "do", "for", "from", "how", "i", "in", "is",
    "it", "me", "my", "of", "on", "or", "please", "that", "the", "this", "to", "use", "we", "what",
    "when", "where", "which", "with", "you", "your",
];

/// One bounded deterministic Skill selection request.
#[derive(Debug, Clone, Copy)]
pub struct SkillSelectionRequest<'a> {
    /// Natural-language task or search query.
    pub query: &'a str,
    /// Maximum number of results requested by the caller.
    pub limit: usize,
    /// Optional exact category filter applied before scoring.
    pub category: Option<&'a str>,
    /// Names already loaded by direct user invocation.
    pub excluded_names: &'a [String],
    /// Whether model-invocation policy must be enforced.
    pub model_invocable_only: bool,
}

impl<'a> SkillSelectionRequest<'a> {
    /// Creates a model-facing request without category or exclusions.
    pub const fn model(query: &'a str, limit: usize) -> Self {
        Self {
            query,
            limit,
            category: None,
            excluded_names: &[],
            model_invocable_only: true,
        }
    }
}

/// Ordered selection results borrowed from the input catalog.
#[derive(Debug)]
pub struct SkillSelection<'a> {
    /// Positive-score matches in deterministic relevance order.
    pub matches: Vec<&'a SkillMetadata>,
    /// The query exceeded a byte or term bound.
    pub query_truncated: bool,
    /// More candidates existed than the selector inspected.
    pub candidate_set_truncated: bool,
    /// More positive matches existed than the returned limit.
    pub results_truncated: bool,
}

impl SkillSelection<'_> {
    /// Reports whether any selector bound omitted information.
    pub const fn truncated(&self) -> bool {
        self.query_truncated || self.candidate_set_truncated || self.results_truncated
    }
}

/// Cheap deterministic selector for Skill names and descriptions.
#[derive(Debug, Clone, Copy, Default)]
pub struct SkillSelector;

impl SkillSelector {
    /// Selects positive-score Skills without loading their bodies.
    pub fn select<'a>(
        self,
        skills: &'a [SkillMetadata],
        request: SkillSelectionRequest<'_>,
    ) -> SkillSelection<'a> {
        let (query, bytes_truncated) = bounded(request.query, MAX_QUERY_BYTES);
        let query_phrase = normalize_phrase(query);
        let (query_terms, terms_truncated) = query_terms(&query_phrase);
        let query_truncated = bytes_truncated || terms_truncated;
        let candidates = skills
            .iter()
            .filter(|skill| !request.model_invocable_only || skill.invocation.model_invocable)
            .filter(|skill| {
                request.category.is_none_or(|category| {
                    skill
                        .category
                        .as_deref()
                        .is_some_and(|value| value.eq_ignore_ascii_case(category))
                })
            })
            .filter(|skill| {
                !request
                    .excluded_names
                    .iter()
                    .any(|name| skill.name.eq_ignore_ascii_case(name))
            })
            .collect::<Vec<_>>();
        let candidate_set_truncated = candidates.len() > MAX_CANDIDATES;
        if query_terms.is_empty() || request.limit == 0 {
            return SkillSelection {
                matches: Vec::new(),
                query_truncated,
                candidate_set_truncated,
                results_truncated: false,
            };
        }

        let mut scored = candidates
            .into_iter()
            .take(MAX_CANDIDATES)
            .filter_map(|skill| {
                let score = score_skill(&query_phrase, &query_terms, skill);
                (score > 0).then_some((score, skill))
            })
            .collect::<Vec<_>>();
        scored.sort_by(|left, right| {
            right
                .0
                .cmp(&left.0)
                .then_with(|| {
                    left.1
                        .name
                        .to_ascii_lowercase()
                        .cmp(&right.1.name.to_ascii_lowercase())
                })
                .then_with(|| left.1.name.cmp(&right.1.name))
        });
        let limit = request.limit.min(MAX_RESULTS);
        let results_truncated = scored.len() > limit;

        SkillSelection {
            matches: scored
                .into_iter()
                .take(limit)
                .map(|(_, skill)| skill)
                .collect(),
            query_truncated,
            candidate_set_truncated,
            results_truncated,
        }
    }
}

fn score_skill(query_phrase: &str, query_terms: &[&str], skill: &SkillMetadata) -> u32 {
    let name = normalize_bounded(&skill.name);
    let description = normalize_bounded(&skill.description);
    let name_terms = phrase_terms(&name);
    let description_terms = phrase_terms(&description);
    let mut score = 0u32;
    if !name.is_empty() && contains_phrase(query_phrase, &name) {
        score = score.saturating_add(256);
    }

    let mut matched_query_terms = 0u32;
    for query_term in query_terms {
        let mut matched = false;
        if name == *query_term {
            score = score.saturating_add(128);
            matched = true;
        } else if name_terms.contains(query_term) {
            score = score.saturating_add(64);
            matched = true;
        } else if contains_related_term(&name_terms, query_term) {
            score = score.saturating_add(24);
            matched = true;
        }

        if description_terms.contains(query_term) {
            score = score.saturating_add(4);
            matched = true;
        } else if contains_related_term(&description_terms, query_term) {
            score = score.saturating_add(1);
            matched = true;
        }

        if matched {
            matched_query_terms = matched_query_terms.saturating_add(1);
        }
    }

    score.saturating_add(matched_query_terms.saturating_mul(matched_query_terms))
}

fn normalize_bounded(value: &str) -> String {
    normalize_phrase(bounded(value, MAX_DOCUMENT_BYTES).0)
}

fn normalize_phrase(value: &str) -> String {
    let mut normalized = String::with_capacity(value.len());
    let mut previous_was_separator = true;
    for character in value.chars() {
        if character.is_alphanumeric() {
            normalized.extend(character.to_lowercase());
            previous_was_separator = false;
        } else if !previous_was_separator {
            normalized.push(' ');
            previous_was_separator = true;
        }
    }
    if previous_was_separator {
        normalized.pop();
    }
    normalized
}

fn query_terms(query_phrase: &str) -> (Vec<&str>, bool) {
    let mut seen = HashSet::new();
    let mut terms = Vec::new();
    for term in query_phrase
        .split_whitespace()
        .filter(|term| term.chars().count() >= 2 && !STOP_WORDS.contains(term))
    {
        if !seen.insert(term) {
            continue;
        }
        if terms.len() == MAX_QUERY_TERMS {
            return (terms, true);
        }
        terms.push(term);
    }
    (terms, false)
}

fn phrase_terms(phrase: &str) -> HashSet<&str> {
    phrase.split_whitespace().take(MAX_DOCUMENT_TERMS).collect()
}

fn contains_phrase(haystack: &str, needle: &str) -> bool {
    haystack == needle
        || haystack.starts_with(&format!("{needle} "))
        || haystack.ends_with(&format!(" {needle}"))
        || haystack.contains(&format!(" {needle} "))
}

fn contains_related_term(terms: &HashSet<&str>, query_term: &str) -> bool {
    if query_term.chars().count() < 4 {
        return false;
    }
    terms.iter().any(|term| {
        term.chars().count() >= 4 && (term.starts_with(query_term) || query_term.starts_with(*term))
    })
}

fn bounded(value: &str, max_bytes: usize) -> (&str, bool) {
    if value.len() <= max_bytes {
        return (value, false);
    }
    let mut end = max_bytes;
    while !value.is_char_boundary(end) {
        end = end.saturating_sub(1);
    }
    (&value[..end], true)
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;
    use crate::skill::{
        SkillInvocationPolicy, SkillProviderId, SkillResourceBase, SkillSourceKind,
    };

    fn skill(name: &str, description: &str) -> SkillMetadata {
        SkillMetadata {
            name: name.to_string(),
            description: description.to_string(),
            category: Some("development".to_string()),
            platforms: Vec::new(),
            source: SkillSourceKind::Project,
            path: PathBuf::new(),
            provider_id: SkillProviderId::new("test").unwrap(),
            invocation: SkillInvocationPolicy::default(),
            resource_base: SkillResourceBase::Opaque {
                description: "test".to_string(),
            },
        }
    }

    #[test]
    fn exact_name_outranks_description_only_match() {
        let skills = vec![
            skill("release-build", "General diagnostics"),
            skill("diagnostics", "Use for release build failures"),
        ];
        let selected = SkillSelector.select(
            &skills,
            SkillSelectionRequest::model("please use release-build", 10),
        );

        assert_eq!(
            selected
                .matches
                .iter()
                .map(|skill| skill.name.as_str())
                .collect::<Vec<_>>(),
            vec!["release-build", "diagnostics"]
        );
    }

    #[test]
    fn description_prefix_and_coverage_produce_stable_order() {
        let skills = vec![
            skill("beta", "Diagnose Rust release linker failures"),
            skill("alpha", "Diagnose Rust formatting"),
            skill("gamma", "Unrelated slides"),
        ];
        let selected = SkillSelector.select(
            &skills,
            SkillSelectionRequest::model("diagnosing rust release linker", 10),
        );

        assert_eq!(
            selected
                .matches
                .iter()
                .map(|skill| skill.name.as_str())
                .collect::<Vec<_>>(),
            vec!["beta", "alpha"]
        );
    }

    #[test]
    fn filters_category_policy_exclusions_and_zero_scores() {
        let mut hidden = skill("hidden", "Rust release linker");
        hidden.invocation.model_invocable = false;
        let mut other_category = skill("slides", "Rust release linker");
        other_category.category = Some("documents".to_string());
        let skills = vec![
            skill("release", "Rust release linker"),
            hidden,
            other_category,
            skill("unrelated", "Presentation authoring"),
        ];
        let excluded = vec!["release".to_string()];
        let selected = SkillSelector.select(
            &skills,
            SkillSelectionRequest {
                query: "rust release linker",
                limit: 10,
                category: Some("development"),
                excluded_names: &excluded,
                model_invocable_only: true,
            },
        );

        assert!(selected.matches.is_empty());
    }

    #[test]
    fn reports_query_and_result_truncation() {
        let query = (0..=MAX_QUERY_TERMS)
            .map(|index| format!("term{index}"))
            .collect::<Vec<_>>()
            .join(" ");
        let skills = vec![skill("alpha", "term0"), skill("beta", "term0")];
        let selected = SkillSelector.select(&skills, SkillSelectionRequest::model(&query, 1));

        assert!(selected.query_truncated);
        assert!(selected.results_truncated);
        assert!(selected.truncated());
        assert_eq!(selected.matches.len(), 1);
    }

    #[test]
    fn reports_candidate_truncation_and_sorts_equal_scores_by_name() {
        let skills = (0..=MAX_CANDIDATES)
            .map(|index| skill(&format!("skill-{index:04}"), "shared keyword"))
            .collect::<Vec<_>>();
        let selected = SkillSelector.select(
            &skills,
            SkillSelectionRequest::model("shared keyword", MAX_RESULTS),
        );

        assert!(selected.candidate_set_truncated);
        assert!(selected.results_truncated);
        assert_eq!(selected.matches.len(), MAX_RESULTS);
        assert_eq!(selected.matches[0].name, "skill-0000");
        assert_eq!(selected.matches[MAX_RESULTS - 1].name, "skill-0049");
    }

    #[test]
    fn stop_words_and_zero_limit_return_no_matches() {
        let skills = vec![skill("alpha", "the useful skill")];
        assert!(
            SkillSelector
                .select(&skills, SkillSelectionRequest::model("the and to", 10))
                .matches
                .is_empty()
        );
        assert!(
            SkillSelector
                .select(&skills, SkillSelectionRequest::model("useful", 0))
                .matches
                .is_empty()
        );
    }
}
