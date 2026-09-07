// Tier 2 local disambiguation scorer.
//
// Sits between deterministic rule matching (Tier 1) and LLM sampling (Tier 3).
// Scores each unresolved issue using three local strategies:
//
//   1. Neighbor-word heuristic: AC match of context_clues in ±40-char window.
//   2. Profile-based priors: preferred-term tables per profile.
//   3. Fixed collocation mapping: compound technical terms resolve deterministically.
//
// Issues scoring above the decided threshold are resolved locally. Issues
// scoring below the ambiguous threshold are suppressed (false positive). Issues
// in the gray zone proceed to Tier 3 (LLM).

use std::sync::Arc;

use crate::rules::ruleset::{Issue, IssueType, Profile, Severity, Tier2Outcome};

// AnchorKind: classifies calibration outcomes as hard or soft

/// Whether a calibration anchor terminates resolution (Hard) or merely
/// contributes evidence to Tier 2 scoring (Soft).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AnchorKind {
    /// ExactAnchor or AnchorAndContext: translation confirmed the english
    /// anchor with high confidence.  Terminates resolution: Tier 2/3 are
    /// skipped entirely.
    Hard,
    /// SynonymAnchor or ContextOnly: weaker signal that contributes to the
    /// Tier 2 score but does not bypass it.  A wrong soft anchor must remain
    /// overridable by Tier 2 evidence or Tier 3 judgment.
    Soft,
}

/// Classify an issue's anchor_match into AnchorKind.
///
/// - `anchor_match == Some(true)` with single suggestion → Hard
///   (unambiguous confirmation).
/// - `anchor_match == Some(true)` with multiple suggestions → Soft
///   (confirmed domain but still ambiguous which suggestion to pick).
/// - `anchor_match == Some(false)` → not an anchor at all (potential
///   false positive).
/// - `anchor_match == None` → no signal.
pub fn classify_anchor(issue: &Issue) -> Option<AnchorKind> {
    match issue.anchor_match {
        Some(true) if issue.suggestions.len() == 1 => Some(AnchorKind::Hard),
        Some(true) if issue.suggestions.len() > 1 => Some(AnchorKind::Soft),
        _ => None, // 0 suggestions or no anchor signal
    }
}

// AmbiguityScore: the Tier 2 output

/// How an issue was resolved (or left unresolved) by Tier 2.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Resolution {
    /// Hard anchor terminated resolution before Tier 2 ran.
    HardAnchor,
    /// Tier 2 resolved via context clue matching.
    ContextClue,
    /// Tier 2 resolved via profile-based preferred-term prior.
    ProfilePrior,
    /// Tier 2 resolved via fixed collocation mapping.
    Collocation,
    /// Combined Tier 2 evidence exceeded threshold.
    Combined,
    /// Score is in the gray zone: needs Tier 3 LLM.
    GrayZone,
    /// Score is below ambiguous threshold: likely false positive, suppress.
    Suppressed,
}

impl Resolution {
    /// Text for the `tier2:` annotation attached to the issue.
    ///
    /// Exhaustive on purpose: a new variant must fail to compile here
    /// rather than reach a wildcard arm and panic at runtime.
    fn label(self, score: f32) -> String {
        match self {
            Resolution::HardAnchor => "hard anchor".to_string(),
            Resolution::ContextClue => format!("context clue (score={score:.2})"),
            Resolution::ProfilePrior => format!("profile prior (score={score:.2})"),
            Resolution::Collocation => format!("collocation (score={score:.2})"),
            Resolution::Combined => format!("combined (score={score:.2})"),
            Resolution::GrayZone => format!("gray zone (score={score:.2})"),
            Resolution::Suppressed => format!("suppressed (score={score:.2})"),
        }
    }
}

/// Tier 2 disambiguation result for a single issue.
#[derive(Debug, Clone)]
pub struct AmbiguityScore {
    /// Composite score in [0.0, 1.0]. Higher = more confident the issue is
    /// real.
    pub score: f32,
    /// If resolved locally, the chosen replacement term.
    pub resolved: Option<String>,
    /// How the score was determined.
    pub resolution: Resolution,
}

// Collocation table: compound terms that resolve deterministically

/// A fixed collocation entry: when `trigger` appears within ±window of an
/// ambiguous `from` term, resolve to `resolved_to`.
struct Collocation {
    /// The ambiguous source term (rule 'from' field).
    from: &'static str,
    /// A compound or neighbor that disambiguates.
    trigger: &'static str,
    /// The resolved replacement.
    resolved_to: &'static str,
}

/// Fixed collocation table for high-frequency polysemous terms.
/// These are compound technical terms where the surrounding word makes
/// the meaning unambiguous.
const COLLOCATIONS: &[Collocation] = &[
    // 進程 vs 行程 vs 處理程序: "process" in different domains
    Collocation {
        from: "進程",
        trigger: "排程",
        resolved_to: "行程",
    },
    Collocation {
        from: "進程",
        trigger: "PID",
        resolved_to: "行程",
    },
    Collocation {
        from: "進程",
        trigger: "行程表",
        resolved_to: "行程",
    },
    Collocation {
        from: "進程",
        trigger: "殺掉",
        resolved_to: "行程",
    },
    Collocation {
        from: "進程",
        trigger: "前景",
        resolved_to: "行程",
    },
    Collocation {
        from: "進程",
        trigger: "背景",
        resolved_to: "行程",
    },
    Collocation {
        from: "進程",
        trigger: "調度",
        resolved_to: "行程",
    },
    Collocation {
        from: "進程",
        trigger: "fork",
        resolved_to: "行程",
    },
    Collocation {
        from: "進程",
        trigger: "exec",
        resolved_to: "行程",
    },
    Collocation {
        from: "進程",
        trigger: "daemon",
        resolved_to: "行程",
    },
    // 程序: "program" vs "procedure"
    Collocation {
        from: "程序",
        trigger: "編譯",
        resolved_to: "程式",
    },
    Collocation {
        from: "程序",
        trigger: "原始碼",
        resolved_to: "程式",
    },
    Collocation {
        from: "程序",
        trigger: "執行檔",
        resolved_to: "程式",
    },
    Collocation {
        from: "程序",
        trigger: "除錯",
        resolved_to: "程式",
    },
    Collocation {
        from: "程序",
        trigger: "偵錯",
        resolved_to: "程式",
    },
    Collocation {
        from: "程序",
        trigger: "compiler",
        resolved_to: "程式",
    },
    Collocation {
        from: "程序",
        trigger: "binary",
        resolved_to: "程式",
    },
    // 線程 → 執行緒 (thread)
    Collocation {
        from: "線程",
        trigger: "多執行緒",
        resolved_to: "執行緒",
    },
    Collocation {
        from: "線程",
        trigger: "mutex",
        resolved_to: "執行緒",
    },
    Collocation {
        from: "線程",
        trigger: "鎖",
        resolved_to: "執行緒",
    },
    Collocation {
        from: "線程",
        trigger: "並行",
        resolved_to: "執行緒",
    },
    Collocation {
        from: "線程",
        trigger: "並發",
        resolved_to: "執行緒",
    },
    Collocation {
        from: "線程",
        trigger: "同步",
        resolved_to: "執行緒",
    },
    // 內存 → 記憶體 (memory)
    Collocation {
        from: "內存",
        trigger: "分配",
        resolved_to: "記憶體",
    },
    Collocation {
        from: "內存",
        trigger: "配置",
        resolved_to: "記憶體",
    },
    Collocation {
        from: "內存",
        trigger: "洩漏",
        resolved_to: "記憶體",
    },
    Collocation {
        from: "內存",
        trigger: "堆疊",
        resolved_to: "記憶體",
    },
    Collocation {
        from: "內存",
        trigger: "heap",
        resolved_to: "記憶體",
    },
    Collocation {
        from: "內存",
        trigger: "stack",
        resolved_to: "記憶體",
    },
    Collocation {
        from: "內存",
        trigger: "malloc",
        resolved_to: "記憶體",
    },
    Collocation {
        from: "內存",
        trigger: "RAM",
        resolved_to: "記憶體",
    },
    // 軟件 → 軟體 (software)
    Collocation {
        from: "軟件",
        trigger: "安裝",
        resolved_to: "軟體",
    },
    Collocation {
        from: "軟件",
        trigger: "更新",
        resolved_to: "軟體",
    },
    Collocation {
        from: "軟件",
        trigger: "版本",
        resolved_to: "軟體",
    },
    Collocation {
        from: "軟件",
        trigger: "下載",
        resolved_to: "軟體",
    },
    // 硬件 → 硬體 (hardware)
    Collocation {
        from: "硬件",
        trigger: "驅動",
        resolved_to: "硬體",
    },
    Collocation {
        from: "硬件",
        trigger: "晶片",
        resolved_to: "硬體",
    },
    Collocation {
        from: "硬件",
        trigger: "主機板",
        resolved_to: "硬體",
    },
    // 接口 → 介面 (interface)
    Collocation {
        from: "接口",
        trigger: "API",
        resolved_to: "介面",
    },
    Collocation {
        from: "接口",
        trigger: "使用者",
        resolved_to: "介面",
    },
    Collocation {
        from: "接口",
        trigger: "圖形",
        resolved_to: "介面",
    },
    Collocation {
        from: "接口",
        trigger: "USB",
        resolved_to: "介面",
    },
    // 服務器 → 伺服器 (server)
    Collocation {
        from: "服務器",
        trigger: "HTTP",
        resolved_to: "伺服器",
    },
    Collocation {
        from: "服務器",
        trigger: "網路",
        resolved_to: "伺服器",
    },
    Collocation {
        from: "服務器",
        trigger: "部署",
        resolved_to: "伺服器",
    },
    Collocation {
        from: "服務器",
        trigger: "雲端",
        resolved_to: "伺服器",
    },
    // 鏈接 → 連結 (link)
    Collocation {
        from: "鏈接",
        trigger: "網址",
        resolved_to: "連結",
    },
    Collocation {
        from: "鏈接",
        trigger: "URL",
        resolved_to: "連結",
    },
    Collocation {
        from: "鏈接",
        trigger: "超連結",
        resolved_to: "連結",
    },
    // 令牌 is "token", with different TW terms per domain
    Collocation {
        from: "令牌",
        trigger: "OAuth",
        resolved_to: "權杖",
    },
    Collocation {
        from: "令牌",
        trigger: "JWT",
        resolved_to: "權杖",
    },
    Collocation {
        from: "令牌",
        trigger: "認證",
        resolved_to: "權杖",
    },
    Collocation {
        from: "令牌",
        trigger: "區塊鏈",
        resolved_to: "代幣",
    },
    Collocation {
        from: "令牌",
        trigger: "加密貨幣",
        resolved_to: "代幣",
    },
    Collocation {
        from: "令牌",
        trigger: "NLP",
        resolved_to: "詞元",
    },
    Collocation {
        from: "令牌",
        trigger: "分詞",
        resolved_to: "詞元",
    },
    Collocation {
        from: "令牌",
        trigger: "tokenizer",
        resolved_to: "詞元",
    },
    // 中間件 → middleware
    Collocation {
        from: "中間件",
        trigger: "HTTP",
        resolved_to: "中介層",
    },
    Collocation {
        from: "中間件",
        trigger: "路由",
        resolved_to: "中介層",
    },
    Collocation {
        from: "中間件",
        trigger: "Express",
        resolved_to: "中介層",
    },
];

// Profile preferred-term priors

/// Profile-based preferred-term entry.
struct PreferredTerm {
    /// The ambiguous source term.
    from: &'static str,
    /// Preferred replacement for this profile.
    preferred: &'static str,
    /// Prior weight in [0.0, 1.0]: how strongly this profile favors
    /// this resolution.  0.3 = weak hint, 0.6 = strong prior.
    weight: f32,
}

/// Base profile prefers the most common TW computing terms.
const BASE_PREFERRED: &[PreferredTerm] = &[
    PreferredTerm {
        from: "進程",
        preferred: "行程",
        weight: 0.3,
    },
    PreferredTerm {
        from: "線程",
        preferred: "執行緒",
        weight: 0.3,
    },
    PreferredTerm {
        from: "內存",
        preferred: "記憶體",
        weight: 0.3,
    },
    PreferredTerm {
        from: "軟件",
        preferred: "軟體",
        weight: 0.3,
    },
    PreferredTerm {
        from: "硬件",
        preferred: "硬體",
        weight: 0.3,
    },
    PreferredTerm {
        from: "接口",
        preferred: "介面",
        weight: 0.3,
    },
    PreferredTerm {
        from: "服務器",
        preferred: "伺服器",
        weight: 0.3,
    },
    PreferredTerm {
        from: "程序",
        preferred: "程式",
        weight: 0.2,
    },
];

/// Strict profile has stronger priors: MoE prescriptive preference.
const STRICT_PREFERRED: &[PreferredTerm] = &[
    PreferredTerm {
        from: "進程",
        preferred: "行程",
        weight: 0.5,
    },
    PreferredTerm {
        from: "線程",
        preferred: "執行緒",
        weight: 0.5,
    },
    PreferredTerm {
        from: "內存",
        preferred: "記憶體",
        weight: 0.5,
    },
    PreferredTerm {
        from: "軟件",
        preferred: "軟體",
        weight: 0.5,
    },
    PreferredTerm {
        from: "硬件",
        preferred: "硬體",
        weight: 0.5,
    },
    PreferredTerm {
        from: "接口",
        preferred: "介面",
        weight: 0.5,
    },
    PreferredTerm {
        from: "服務器",
        preferred: "伺服器",
        weight: 0.5,
    },
    PreferredTerm {
        from: "程序",
        preferred: "程式",
        weight: 0.4,
    },
    PreferredTerm {
        from: "令牌",
        preferred: "權杖",
        weight: 0.3,
    },
    PreferredTerm {
        from: "中間件",
        preferred: "中介層",
        weight: 0.4,
    },
];

fn preferred_terms_for_profile(profile: Profile) -> &'static [PreferredTerm] {
    match profile {
        Profile::Base => BASE_PREFERRED,
        Profile::Strict => STRICT_PREFERRED,
    }
}

// Tier 2 scorer

/// Default thresholds for Tier 2 scoring.
/// Issues with score >= DECIDED are resolved locally.
/// Issues with score < AMBIGUOUS are suppressed (likely FP).
/// Issues with score in [AMBIGUOUS, DECIDED) go to Tier 3.
pub const DEFAULT_DECIDED_THRESHOLD: f32 = 0.6;
pub const DEFAULT_AMBIGUOUS_THRESHOLD: f32 = 0.3;

/// Configuration for the Tier 2 scorer.
pub struct DisambigConfig {
    /// Score at or above which Tier 2 resolves the issue locally.
    pub decided_threshold: f32,
    /// Score below which the issue is suppressed as likely FP.
    pub ambiguous_threshold: f32,
    /// Active profile for preferred-term priors.
    pub profile: Profile,
}

impl Default for DisambigConfig {
    fn default() -> Self {
        Self {
            decided_threshold: DEFAULT_DECIDED_THRESHOLD,
            ambiguous_threshold: DEFAULT_AMBIGUOUS_THRESHOLD,
            profile: Profile::Base,
        }
    }
}

/// Score a single issue using Tier 2 local strategies.
///
/// The composite score is built from three independent signals:
///   - Context clue density: how many of the rule's context_clues appear
///     in the surrounding text window.
///   - Profile prior: does the active profile have a preferred resolution
///     for this ambiguous term?
///   - Collocation: does a fixed compound pattern resolve the term?
///
/// Collocation is terminal: if matched, the score is 1.0 and the resolved
/// term is set directly.
pub fn score_issue(issue: &Issue, context_window: &str, cfg: &DisambigConfig) -> AmbiguityScore {
    // Strategy 1: hard anchor terminates immediately.
    if let Some(AnchorKind::Hard) = classify_anchor(issue) {
        let resolved = issue.suggestions.first().cloned();
        return AmbiguityScore {
            score: 1.0,
            resolved,
            resolution: Resolution::HardAnchor,
        };
    }

    // Strategy 2: collocation mapping (terminal if matched).
    if let Some(resolved) = check_collocation(&issue.found, context_window) {
        // Verify the resolved term is actually in the suggestion list.
        if issue.suggestions.iter().any(|s| s == resolved) {
            return AmbiguityScore {
                score: 1.0,
                resolved: Some(resolved.to_string()),
                resolution: Resolution::Collocation,
            };
        }
    }

    // Strategy 2b: domain veto for known false-positive contexts.
    if has_negative_domain_signal(&issue.found, context_window) {
        return AmbiguityScore {
            score: 0.0,
            resolved: None,
            resolution: Resolution::Suppressed,
        };
    }

    // Strategy 3: context clue density.
    let clue_score = compute_clue_score(issue, context_window);

    // Strategy 4: profile-based prior.
    let (prior_score, prior_term) = compute_profile_prior(issue, cfg.profile);

    // Strategy 5: soft anchor contributes partial evidence.
    let anchor_bonus = match classify_anchor(issue) {
        Some(AnchorKind::Soft) => 0.2,
        _ => match issue.anchor_match {
            Some(false) => -0.2, // anti-evidence: calibration rejected
            _ => 0.0,
        },
    };

    // Combine signals. Clue score is the primary signal; prior and anchor are
    // additive evidence.
    let raw = clue_score + prior_score + anchor_bonus;
    let score = raw.clamp(0.0, 1.0);

    // Determine resolution.
    if score >= cfg.decided_threshold {
        // Resolve: pick the best term.
        let resolved = pick_resolved_term(issue, prior_term, clue_score, prior_score);
        let resolution = if clue_score >= prior_score && clue_score > 0.0 {
            Resolution::ContextClue
        } else if prior_score > 0.0 {
            Resolution::ProfilePrior
        } else {
            Resolution::Combined
        };
        AmbiguityScore {
            score,
            resolved,
            resolution,
        }
    } else if score < cfg.ambiguous_threshold {
        AmbiguityScore {
            score,
            resolved: None,
            resolution: Resolution::Suppressed,
        }
    } else {
        AmbiguityScore {
            score,
            resolved: None,
            resolution: Resolution::GrayZone,
        }
    }
}

/// Check the collocation table for a deterministic resolution.
fn check_collocation<'a>(from: &str, context: &str) -> Option<&'a str> {
    for c in COLLOCATIONS {
        if c.from == from && context.contains(c.trigger) {
            return Some(c.resolved_to);
        }
    }
    None
}

/// Detect obvious non-target-domain usage for highly polysemous terms.
fn has_negative_domain_signal(from: &str, context: &str) -> bool {
    match from {
        // "進程" is only a safe correction in operating-system contexts.
        // General prose such as "學習的進程" should be suppressed locally.
        "進程" => [
            "學習", "成長", "改革", "歷史", "發展", "進展", "過程", "耐心", "毅力",
        ]
        .iter()
        .any(|trigger| context.contains(trigger)),
        _ => false,
    }
}

/// Compute a clue density score from the issue's context_clues.
///
/// The score is proportional to how many clue words appear in the window,
/// normalized by total clue count.  Minimum 1 clue match for any positive
/// signal (consistent with MIN_SCAN_CLUE_MATCHES).
fn compute_clue_score(issue: &Issue, context_window: &str) -> f32 {
    let clues = match &issue.context_clues {
        Some(c) if !c.is_empty() => c,
        _ => return 0.0,
    };

    let matched = clues
        .iter()
        .filter(|c| context_window.contains(c.as_str()))
        .count();

    if matched == 0 {
        return 0.0;
    }

    // Scale: 1 clue = 0.3, 2 clues = 0.5, 3+ clues = 0.6+
    let ratio = matched as f32 / clues.len() as f32;
    (0.3 + ratio * 0.4).min(0.7)
}

/// Look up the profile's preferred term for this issue's 'from' field.
fn compute_profile_prior(issue: &Issue, profile: Profile) -> (f32, Option<&'static str>) {
    let prefs = preferred_terms_for_profile(profile);
    for p in prefs {
        if p.from == issue.found {
            return (p.weight, Some(p.preferred));
        }
    }
    (0.0, None)
}

/// Pick the resolved term. Prefers collocation > profile prior > first
/// suggestion.
fn pick_resolved_term(
    issue: &Issue,
    prior_term: Option<&str>,
    clue_score: f32,
    prior_score: f32,
) -> Option<String> {
    // If profile prior is the dominant signal, use its preferred term (but only
    // if it is actually in the suggestion list).
    if prior_score > clue_score {
        if let Some(pt) = prior_term {
            if issue.suggestions.iter().any(|s| s == pt) {
                return Some(pt.to_string());
            }
        }
    }

    // Fall back to first suggestion.
    issue.suggestions.first().cloned()
}

// Batch Tier 2 processing

/// Result of running Tier 2 on a batch of issues.
#[derive(Debug, Default)]
pub struct DisambigStats {
    /// Issues resolved by hard anchor (bypassed Tier 2).
    pub hard_anchor: usize,
    /// Issues resolved by Tier 2 locally.
    pub tier2_resolved: usize,
    /// Issues suppressed (score below ambiguous threshold).
    pub suppressed: usize,
    /// Issues in gray zone (forwarded to Tier 3).
    pub gray_zone: usize,
    /// Issues not eligible for disambiguation (no english/clues, deterministic
    /// types).
    pub not_eligible: usize,
}

/// Whether an issue is eligible for Tier 2 disambiguation.
///
/// Deterministic issue types (Punctuation, Case, Variant, Grammar, AiStyle)
/// are resolved at Tier 1 and never enter Tier 2. Only genuinely ambiguous
/// lexical issues should enter Tier 2: multi-suggestion rules, clue-gated
/// rules, anchor-confirmed issues, or terms with explicit negative-domain
/// screening.
fn needs_negative_domain_screening(found: &str) -> bool {
    matches!(found, "進程")
}

pub fn is_tier2_eligible(issue: &Issue) -> bool {
    matches!(
        issue.rule_type,
        IssueType::CrossStrait | IssueType::Confusable
    ) && (issue.anchor_match == Some(true)
        || issue.suggestions.len() > 1
        || issue.context_clues.as_ref().is_some_and(|c| !c.is_empty())
        || needs_negative_domain_screening(&issue.found))
}

/// Run Tier 2 disambiguation on a batch of issues.
///
/// For each eligible issue:
/// - Score it using local strategies.
/// - If resolved: promote the chosen suggestion to front, annotate context.
/// - If suppressed: downgrade severity to Info with annotation.
/// - If gray zone: leave unchanged for Tier 3.
///
/// Returns stats for observability.
pub fn disambiguate_batch(issues: &mut [Issue], text: &str, cfg: &DisambigConfig) -> DisambigStats {
    let mut stats = DisambigStats::default();

    for issue in issues.iter_mut() {
        if !is_tier2_eligible(issue) {
            stats.not_eligible += 1;
            continue;
        }

        // Extract context window around the issue.
        let window = extract_context_for_disambig(text, issue.offset, issue.length);

        let result = score_issue(issue, window, cfg);

        match result.resolution {
            Resolution::HardAnchor
            | Resolution::ContextClue
            | Resolution::ProfilePrior
            | Resolution::Collocation
            | Resolution::Combined => {
                if result.resolution == Resolution::HardAnchor {
                    stats.hard_anchor += 1;
                } else {
                    stats.tier2_resolved += 1;
                }
                issue.tier2_outcome = Tier2Outcome::Resolved;
                if let Some(ref term) = result.resolved {
                    promote_suggestion(issue, term);
                }
                let label = result.resolution.label(result.score);
                annotate_issue(issue, &format!("tier2: {label}"));
            }
            Resolution::Suppressed => {
                stats.suppressed += 1;
                issue.tier2_outcome = Tier2Outcome::Suppressed;
                issue.severity = Severity::Info;
                annotate_issue(
                    issue,
                    &format!("tier2: {}", result.resolution.label(result.score)),
                );
            }
            Resolution::GrayZone => {
                stats.gray_zone += 1;
                issue.tier2_outcome = Tier2Outcome::GrayZone;
            }
        }
    }

    stats
}

/// Extract a context window for Tier 2 scoring.
/// Uses ±40 chars bounded by paragraph breaks, consistent with the scanner's
/// CONTEXT_WINDOW_CHARS.
fn extract_context_for_disambig(text: &str, offset: usize, length: usize) -> &str {
    let offset = offset.min(text.len());
    let offset = text.floor_char_boundary(offset);
    let end_offset = offset.saturating_add(length).min(text.len());
    let end_offset = text.ceil_char_boundary(end_offset);

    // Walk backward up to 40 chars, stop at paragraph boundary.
    let mut start = offset;
    let mut chars_back = 0;
    for (idx, c) in text[..offset].char_indices().rev() {
        if c == '\n' {
            // Check for paragraph break (double newline).
            if idx > 0 && text.as_bytes().get(idx.saturating_sub(1)) == Some(&b'\n') {
                start = idx + 1;
                break;
            }
        }
        start = idx;
        chars_back += 1;
        if chars_back >= 40 {
            break;
        }
    }

    // Walk forward up to 40 chars from end of matched span.
    let mut end = end_offset;
    let mut chars_fwd = 0;
    for (idx, c) in text[end_offset..].char_indices() {
        if c == '\n' {
            let abs = end_offset + idx;
            if abs + 1 < text.len() && text.as_bytes().get(abs + 1) == Some(&b'\n') {
                end = abs;
                break;
            }
        }
        end = end_offset + idx + c.len_utf8();
        chars_fwd += 1;
        if chars_fwd >= 40 {
            break;
        }
    }

    &text[start..end]
}

/// Promote a specific suggestion to the front of the suggestions list.
fn promote_suggestion(issue: &mut Issue, term: &str) {
    if let Some(pos) = issue.suggestions.iter().position(|s| s == term) {
        if pos != 0 {
            let mut sugs = issue.suggestions.to_vec();
            sugs.swap(0, pos);
            issue.suggestions = sugs.into();
        }
        issue.refresh_suggested_rewrite();
    }
}

/// Append a disambiguation annotation to the issue's context field.
fn annotate_issue(issue: &mut Issue, annotation: &str) {
    let new_ctx = match &issue.context {
        Some(ctx) => format!("{ctx} [{annotation}]"),
        None => format!("[{annotation}]"),
    };
    issue.context = Some(Arc::from(new_ctx));
}

// Semantic chunking for Tier 3 context windows

/// Maximum chunk size in characters for Tier 3 LLM context.
const MAX_CHUNK_CHARS: usize = 500;

/// Extract a semantically bounded chunk of text containing the given byte
/// range [offset, offset+length).  Chunks by paragraph breaks (\n\n),
/// markdown headings (^#{1,6} ), and list items (^[-*+] or ^\d+\.).
///
/// Returns a string slice of at most MAX_CHUNK_CHARS characters that
/// contains the issue span.  Never splits a multi-byte UTF-8 character.
pub fn extract_semantic_chunk(text: &str, offset: usize, length: usize) -> &str {
    if text.is_empty() {
        return text;
    }

    let offset = offset.min(text.len());
    let offset = text.floor_char_boundary(offset);
    let end_offset = offset.saturating_add(length).min(text.len());
    let end_offset = text.ceil_char_boundary(end_offset);

    // Find the paragraph/section containing the offset. First, find backward
    // boundary.
    let chunk_start = find_chunk_start(text, offset);
    // Then, find forward boundary.
    let chunk_end = find_chunk_end(text, end_offset);

    let chunk = &text[chunk_start..chunk_end];

    // If the chunk is within budget, return it directly.
    if chunk.chars().count() <= MAX_CHUNK_CHARS {
        return chunk;
    }

    // Chunk too large: narrow to ±230 chars around the offset, but respect char
    // boundaries and try to land on a sentence boundary. The 20-char sentence
    // boundary search margin means effective max is ~500.
    let half_budget = (MAX_CHUNK_CHARS - 40) / 2;

    // Walk backward from offset by half_budget chars.
    let mut narrow_start = offset;
    let mut chars_back = 0;
    for (idx, c) in text[chunk_start..offset].char_indices().rev() {
        if chars_back >= half_budget {
            // Try to land on a sentence boundary.
            if matches!(c, '。' | '！' | '？' | '；' | '\n') {
                narrow_start = chunk_start + idx + c.len_utf8();
                break;
            }
        }
        narrow_start = chunk_start + idx;
        chars_back += 1;
        if chars_back >= half_budget + 20 {
            break;
        }
    }

    // Walk forward from end_offset by half_budget chars.
    let mut narrow_end = end_offset;
    let mut chars_fwd = 0;
    for (idx, c) in text[end_offset..chunk_end].char_indices() {
        narrow_end = end_offset + idx + c.len_utf8();
        chars_fwd += 1;
        if chars_fwd >= half_budget {
            // Try to land on a sentence boundary.
            if matches!(c, '。' | '！' | '？' | '；' | '\n') {
                break;
            }
        }
        if chars_fwd >= half_budget + 20 {
            break;
        }
    }

    // Hard post-trim: enforce the MAX_CHUNK_CHARS contract. The sentence
    // boundary search margin can push the chunk slightly over budget; trim from
    // the end while preserving the issue span and char boundaries.
    let result = &text[narrow_start..narrow_end];
    if result.chars().count() > MAX_CHUNK_CHARS {
        let mut trimmed_end = narrow_start;
        for (i, (byte_idx, _)) in result.char_indices().enumerate() {
            trimmed_end = narrow_start + byte_idx;
            if i >= MAX_CHUNK_CHARS {
                break;
            }
        }
        // Ensure we don't trim past the issue span.
        let issue_end = end_offset.max(narrow_start);
        trimmed_end = trimmed_end.max(issue_end);
        &text[narrow_start..trimmed_end.min(narrow_end)]
    } else {
        result
    }
}

/// Maximum bytes to scan for a structural boundary before giving up.
/// Prevents O(N) scans on huge single-paragraph files.
const MAX_BOUNDARY_SEARCH: usize = 2000;

/// Find the start of the semantic chunk containing `offset`.
/// Scans backward for paragraph breaks (\n\n), headings, or list items.
fn find_chunk_start(text: &str, offset: usize) -> usize {
    if offset == 0 {
        return 0;
    }

    let bytes = text.as_bytes();
    let mut pos = offset;
    let limit = offset.saturating_sub(MAX_BOUNDARY_SEARCH);

    // Scan backward for a structural boundary.
    while pos > limit {
        // Paragraph break: \n\n
        if pos >= 2 && bytes[pos - 1] == b'\n' && bytes[pos - 2] == b'\n' {
            return pos;
        }

        // Single newline: check if the line starting at pos is a heading or
        // list item.
        if bytes[pos - 1] == b'\n' && is_heading_or_list_start(text, pos) {
            return pos;
        }
        pos -= 1;
    }

    // No boundary found within limit: use the limit position, adjusted to a
    // char boundary.
    text.floor_char_boundary(limit)
}

/// Find the end of the semantic chunk containing the byte range ending at
/// `end_offset`.
fn find_chunk_end(text: &str, end_offset: usize) -> usize {
    if end_offset >= text.len() {
        return text.len();
    }

    let bytes = text.as_bytes();
    let mut pos = end_offset;
    let limit = (end_offset + MAX_BOUNDARY_SEARCH).min(text.len());

    while pos < limit {
        if bytes[pos] == b'\n' {
            // Paragraph break: \n\n
            if pos + 1 < text.len() && bytes[pos + 1] == b'\n' {
                return pos;
            }
            // Check if next line is a heading or list item (structural break).
            if pos + 1 < text.len() && is_heading_or_list_start(text, pos + 1) {
                return pos;
            }
        }
        pos += 1;
    }

    // No boundary found within limit: use a char-safe position.
    text.ceil_char_boundary(limit.min(text.len()))
}

/// Check if the text at `pos` starts a markdown heading (^#{1,6} ) or
/// a list item (^[-*+] or ^\d+\.).
fn is_heading_or_list_start(text: &str, pos: usize) -> bool {
    let rest = &text[pos..];

    // Heading: # to ###### followed by space.
    if rest.starts_with('#') {
        let hashes = rest.bytes().take_while(|&b| b == b'#').count();
        if hashes <= 6 && rest.as_bytes().get(hashes) == Some(&b' ') {
            return true;
        }
    }

    // Unordered list: -, *, + followed by space.
    if rest.len() >= 2 {
        let first = rest.as_bytes()[0];
        if matches!(first, b'-' | b'*' | b'+') && rest.as_bytes()[1] == b' ' {
            return true;
        }
    }

    // Ordered list: digits followed by . and space.
    let digits: usize = rest.bytes().take_while(|b| b.is_ascii_digit()).count();
    if digits > 0 && digits < rest.len() {
        let after = &rest[digits..];
        if after.starts_with(". ") {
            return true;
        }
    }

    false
}

// Tests

#[cfg(test)]
#[path = "../../tests/unit/engine/disambig/tests.rs"]
mod tests;
