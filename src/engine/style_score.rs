// Composite three-axis style scorecard.
//
// Pure aggregation: AI-likelihood, translationese density, regional density.
// Three orthogonal scores, NEVER collapsed into a single number: the consumer
// chooses which axis to act on.
//
// No new detectors, no scoring-module changes: this module reads
// AiSignatureReport, TranslationeseReport, and the issue list, then emits a
// flat scorecard. Wired by the CLI --detect-style path and the MCP detect_style
// parameter.

use serde::{Deserialize, Serialize};

use crate::engine::ai_score::AiSignatureReport;
use crate::engine::translationese_score::TranslationeseReport;
use crate::rules::ruleset::{Issue, IssueType, Severity};

/// Three orthogonal style scores.  Each axis is `Option<f32>`: `None`
/// means the axis was not computed (e.g. AI detection disabled).  Values
/// are in [0.0, 1.0] but each axis carries its own meaning: they are
/// presented side by side, never averaged.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct StyleScores {
    /// AI-likelihood score.  Higher = more AI-tell signals.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ai: Option<f32>,
    /// Translationese density.  Higher = more 歐化 tells.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub translationese: Option<f32>,
    /// Density of regional vocabulary, not terminology consistency.
    ///
    /// It counts cross-strait and confusable findings per 1000 characters, so
    /// a document that uses PRC forms densely enough, at least one matching
    /// finding per 1000 characters, scores 1.0 while being perfectly
    /// consistent, and the consistency report beside it correctly emits
    /// nothing. Two unrelated quantities shared the name "consistency"
    /// and landed in the same JSON object. Higher = more cross-strait /
    /// confusable issues per 1000 chars.  Capped at 1.0.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub regional_density: Option<f32>,
}

/// Top contributing issues per axis.  Limited to ≤5 per axis to keep
/// payloads small.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TopIssuesPerAxis {
    pub ai: Vec<TopIssue>,
    pub translationese: Vec<TopIssue>,
    pub regional_density: Vec<TopIssue>,
}

/// Lightweight summary of a contributing issue (no full Issue payload).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TopIssue {
    pub line: usize,
    pub col: usize,
    pub found: String,
    pub severity: Severity,
    pub rule_type: IssueType,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub context: Option<String>,
}

impl TopIssue {
    fn from_issue(issue: &Issue) -> Self {
        Self {
            line: issue.line,
            col: issue.col,
            found: issue.found.clone(),
            severity: issue.severity,
            rule_type: issue.rule_type,
            context: issue.context.as_ref().map(|c| c.to_string()),
        }
    }
}

/// Composite scorecard.  Three axes never combined.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct StyleScorecard {
    pub style_scores: StyleScores,
    pub top_issues_per_axis: TopIssuesPerAxis,
}

impl StyleScorecard {
    /// Aggregate scores from existing scanner outputs.  `text_chars` is
    /// the character count of the analyzed text: the regional_density axis
    /// scales by it.  `None` values indicate axes that were not computed
    /// for this run.
    pub fn build(
        ai: Option<&AiSignatureReport>,
        translationese: Option<&TranslationeseReport>,
        issues: &[Issue],
        text_chars: usize,
    ) -> Self {
        let regional_density = compute_regional_density(issues, text_chars);
        Self {
            style_scores: StyleScores {
                ai: ai.map(|s| s.score),
                translationese: translationese.map(|s| s.score),
                regional_density: Some(regional_density),
            },
            top_issues_per_axis: top_issues_per_axis(issues),
        }
    }
}

/// Regional vocabulary density: count of `cross_strait` + `confusable`
/// issues per 1000 chars, capped at 1.0.  Documents that mix 程式/程序 and
/// 記憶體/內存 raise this axis even when individual rules are
/// Info-severity.  This measures how much PRC vocabulary is present, not
/// whether the document is self-consistent about it.
fn compute_regional_density(issues: &[Issue], text_chars: usize) -> f32 {
    if text_chars == 0 {
        return 0.0;
    }
    let count = issues
        .iter()
        .filter(|i| matches!(i.rule_type, IssueType::CrossStrait | IssueType::Confusable))
        .count();
    let per_1000 = (count as f32) * 1000.0 / (text_chars as f32);
    per_1000.min(1.0)
}

/// Top ≤5 issues per axis, ordered by severity (Error > Warning > Info)
/// then by line.  No ranking signal beyond severity: this is a triage
/// list, not a ranking.
fn top_issues_per_axis(issues: &[Issue]) -> TopIssuesPerAxis {
    let mut out = TopIssuesPerAxis::default();
    for issue in issues {
        let bucket = match issue.rule_type {
            // A finding that is not evidence about authorship does not belong
            // in this axis's list, or a document scoring ai: 0.0 would cite
            // 全文混用「你」與「您」 as what earned it. Scoped to the arm it
            // describes, so a future finding carrying one of those families
            // under another type is not dropped silently.
            IssueType::AiStyle
                if issue
                    .structural_family
                    .is_some_and(|f| !f.is_authorship_evidence()) =>
            {
                continue
            }
            IssueType::AiStyle => &mut out.ai,
            IssueType::Translationese => &mut out.translationese,
            IssueType::CrossStrait | IssueType::Confusable => &mut out.regional_density,
            _ => continue,
        };
        bucket.push(TopIssue::from_issue(issue));
    }
    for bucket in [
        &mut out.ai,
        &mut out.translationese,
        &mut out.regional_density,
    ] {
        bucket.sort_by(|a, b| b.severity.cmp(&a.severity).then(a.line.cmp(&b.line)));
        bucket.truncate(5);
    }
    out
}

#[cfg(test)]
#[path = "../../tests/unit/engine/style_score/tests.rs"]
mod tests;
