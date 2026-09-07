// Document-wide terminology consistency report.
//
// Groups scan issues by their english field (natural equivalence class), then
// for each group checks whether the canonical zh-TW form also appears elsewhere
// in the document. Mixed usage produces a Consistency diagnostic alerting the
// author that the same concept is referred to with both regional variants.
//
// TM-suppressed issues are excluded from consistency grouping: those are
// user-approved overrides, not inadvertent inconsistency.

use std::collections::BTreeMap;

use serde::Serialize;

use crate::engine::excluded::{is_excluded, merge_ranges_pub, ByteRange};
use crate::rules::glossary::ProjectGlossary;
use crate::rules::ruleset::{Issue, IssueType, Severity};

/// One occurrence of a calque in the document: used to anchor the
/// consistency diagnostic.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct ConsistencyOccurrence {
    pub offset: usize,
    pub line: usize,
    pub col: usize,
    pub found: String,
}

/// Aggregated consistency record for one equivalence class.  All fields
/// are populated only when both the calque AND a canonical zh-TW form
/// appear in the same document.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct ConsistencyGroup {
    /// English anchor (natural equivalence-class key).
    pub term_group: String,
    /// The TW-preferred form the linter recommends.
    pub preferred: String,
    /// All occurrences of the calque(s) in this group.
    pub occurrences: Vec<ConsistencyOccurrence>,
}

/// Top-level consistency report.  Empty `groups` means no mixed usage.
#[derive(Debug, Clone, Default, Serialize)]
pub struct ConsistencyReport {
    pub groups: Vec<ConsistencyGroup>,
}

impl ConsistencyReport {
    pub fn is_empty(&self) -> bool {
        self.groups.is_empty()
    }
}

/// Build a consistency report from raw scan issues.
///
/// Algorithm:
///   1. Filter to CrossStrait / Confusable issues with non-empty
///      `english`.  Those are the cleanest equivalence-class anchors.
///   2. Skip issues whose severity is Info: TM-suppressed downgrades
///      land at Info; they are user-approved and should not count.
///   3. Group by `english`.  For each group, choose the TW-preferred
///      canonical form from `glossary.preferred` when that preferred
///      form appears outside the group's flagged spans; otherwise fall back
///      to the first suggestion.
///   4. Check whether that canonical form ALSO appears as a substring
///      outside those spans. If yes, both regional variants coexist → emit
///      a group.
pub fn compute_consistency_report(
    text: &str,
    issues: &[Issue],
    glossary: &ProjectGlossary,
) -> ConsistencyReport {
    let mut grouped: BTreeMap<String, Vec<&Issue>> = BTreeMap::new();

    for issue in issues {
        let eligible = matches!(
            issue.rule_type,
            IssueType::CrossStrait | IssueType::Confusable
        ) && issue.severity != Severity::Info;
        if !eligible {
            continue;
        }
        let Some(english) = issue.english.as_deref().filter(|e| !e.is_empty()) else {
            continue;
        };
        grouped.entry(english.to_string()).or_default().push(issue);
    }

    let mut report = ConsistencyReport::default();

    for (english, issues_in_group) in grouped {
        // Normalize once per group so repeated source forms do not require a
        // full issue scan for every candidate occurrence. Public callers may
        // supply overlapping spans or an unsorted issue list.
        let flagged_spans = merge_ranges_pub(
            issues_in_group
                .iter()
                .filter(|issue| issue.length > 0)
                .map(|issue| ByteRange {
                    start: issue.offset,
                    end: issue.offset.saturating_add(issue.length),
                })
                .collect(),
        );
        let canonical =
            preferred_canonical_for_group(text, &issues_in_group, &flagged_spans, glossary);
        let Some(canonical) = canonical else { continue };

        // 厄瓜多 inside 厄瓜多爾 is one usage, not two regional variants.
        if !has_independent_occurrence(text, &canonical, &flagged_spans) {
            continue;
        }

        let occurrences: Vec<ConsistencyOccurrence> = issues_in_group
            .iter()
            .map(|i| ConsistencyOccurrence {
                offset: i.offset,
                line: i.line,
                col: i.col,
                found: i.found.clone(),
            })
            .collect();

        report.groups.push(ConsistencyGroup {
            term_group: english,
            preferred: canonical,
            occurrences,
        });
    }

    report
}

fn has_independent_occurrence(text: &str, canonical: &str, flagged_spans: &[ByteRange]) -> bool {
    if canonical.is_empty() {
        return false;
    }
    let mut search_from = 0;
    while let Some(relative) = text[search_from..].find(canonical) {
        let start = search_from + relative;
        let end = start + canonical.len();
        if !is_excluded(start, end, flagged_spans) {
            return true;
        }

        // Advance one character so a rejected match cannot hide an overlapping
        // match whose full span lies outside the calque.
        search_from = text.ceil_char_boundary(start + 1);
    }
    false
}

fn preferred_canonical_for_group(
    text: &str,
    issues_in_group: &[&Issue],
    flagged_spans: &[ByteRange],
    glossary: &ProjectGlossary,
) -> Option<String> {
    // Prefer project glossary house terms when they also appear in the
    // document, but only when the rule already surfaced that term as a
    // canonical suggestion for this equivalence class. Short zh terms are too
    // collision-prone for edit-distance matching.
    if !glossary.preferred.is_empty() {
        for preferred in &glossary.preferred {
            if preferred.is_empty() {
                continue;
            }
            if glossary_preferred_matches_group(preferred, issues_in_group)
                && has_independent_occurrence(text, preferred, flagged_spans)
            {
                return Some(preferred.clone());
            }
        }
    }

    issues_in_group
        .iter()
        .find_map(|i| i.suggestions.first())
        .filter(|s| !s.is_empty())
        .cloned()
}

fn glossary_preferred_matches_group(preferred: &str, issues_in_group: &[&Issue]) -> bool {
    issues_in_group.iter().any(|issue| {
        issue
            .suggestions
            .iter()
            .any(|suggestion| suggestion == preferred)
    })
}

#[cfg(test)]
#[path = "../../tests/unit/engine/consistency/tests.rs"]
mod tests;
