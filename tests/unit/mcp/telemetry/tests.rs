use super::*;

#[test]
fn zero_input_yields_null_metrics() {
    let t = TokenTelemetry::default();
    let m = t.derive_metrics();
    assert!(m.estimated_tokens_per_1k_chars.is_none());
    assert!(m.estimated_llm_token_share.is_none());
    assert!(m.estimated_tokens_per_ambiguous_term.is_none());
}

#[test]
fn no_llm_yields_zero_share() {
    let t = TokenTelemetry {
        input_chars: 5000,
        rule_hits: 10,
        ..Default::default()
    };
    let m = t.derive_metrics();
    assert_eq!(m.estimated_tokens_per_1k_chars, Some(0.0));
    assert_eq!(m.estimated_llm_token_share, Some(0.0));
    assert!(m.estimated_tokens_per_ambiguous_term.is_none());
}

#[test]
fn llm_metrics_computed_correctly() {
    let t = TokenTelemetry {
        input_chars: 2000,
        rule_hits: 8,
        ambiguous_terms: 2,
        llm_round_trips: 2,
        prompt_tokens: 100,
        completion_tokens: 20,
        ..Default::default()
    };
    let m = t.derive_metrics();
    // total_llm = 120, per 1k chars = 120 * 1000 / 2000 = 60.0
    assert_eq!(m.estimated_tokens_per_1k_chars, Some(60.0));
    // share = 120 / (8 + 120) = 120/128 = 0.9375
    assert!((m.estimated_llm_token_share.unwrap() - 0.9375).abs() < 1e-6);
    // per ambiguous = 120 / 2 = 60.0
    assert_eq!(m.estimated_tokens_per_ambiguous_term, Some(60.0));
}

#[test]
fn saturating_accumulation() {
    let mut t = TokenTelemetry {
        prompt_tokens: u64::MAX,
        ..Default::default()
    };
    t.record_llm_call(1, 0);
    assert_eq!(t.prompt_tokens, u64::MAX);
    assert_eq!(t.llm_round_trips, 1);
}

#[test]
fn cache_counters_in_metrics() {
    let t = TokenTelemetry {
        cache_hits: 5,
        cache_misses: 3,
        ..Default::default()
    };
    let m = t.derive_metrics();
    assert_eq!(m.cache_hit_count, 5);
    assert_eq!(m.cache_miss_count, 3);
}
