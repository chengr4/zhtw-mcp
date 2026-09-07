use super::*;

fn test_key() -> JudgmentKey {
    JudgmentKey {
        ruleset_hash: "abc123".into(),
        judgment_prompt_version: JUDGMENT_PROMPT_VERSION,
        local_disambig_version: LOCAL_DISAMBIG_VERSION,
        profile: "base".into(),
        content_type: "markdown".into(),
        normalized_context: normalize_context_for_cache("some context around the term"),
        ambiguous_term: "進程".into(),
        candidate_set_hash: hash_candidate_set(&["行程".into(), "進程".into()]),
        english_anchor: "process".into(),
    }
}

#[test]
fn insert_and_retrieve() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("cache.json");
    let mut cache = JudgmentCache::open(&path);
    let key = test_key();
    let value = cache.make_value(
        Some("行程".into()),
        0.9,
        "confirmed".into(),
        "claude".into(),
    );
    cache.insert(&key, value);
    assert_eq!(cache.len(), 1);
    let hit = cache.get(&key);
    assert!(hit.is_some());
    assert_eq!(hit.unwrap().chosen_replacement.as_deref(), Some("行程"));
    assert_eq!(cache.hits, 1);
}

#[test]
fn miss_on_different_key() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("cache.json");
    let mut cache = JudgmentCache::open(&path);
    let key = test_key();
    let value = cache.make_value(Some("行程".into()), 0.9, String::new(), "claude".into());
    cache.insert(&key, value);

    let mut key2 = test_key();
    key2.ruleset_hash = "different".into();
    assert!(cache.get(&key2).is_none());
    assert_eq!(cache.misses, 1);
}

#[test]
fn expired_entry_evicted() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("cache.json");
    let mut cache = JudgmentCache::open(&path);
    let key = test_key();
    // Insert with already-expired timestamp.
    let mut value = cache.make_value(Some("行程".into()), 0.9, String::new(), "claude".into());
    value.expires_at = 1; // long ago
    cache.store.entries.insert(key_string(&key), value);

    assert!(cache.get(&key).is_none());
    assert_eq!(cache.misses, 1);
    // Entry was lazily removed.
    assert!(cache.is_empty());
}

#[test]
fn a_call_that_judged_nothing_does_not_rewrite_the_store() {
    // The per-call flush exists for judgments a signal would otherwise take
    // with it, and for nothing else. Opening a store marks it stale whenever
    // housekeeping changed it, which has no deadline: the next open redoes it.
    // Writing megabytes for that on a call that learned nothing is the
    // amplification this separation removes.
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("cache.json");
    {
        let mut cache = JudgmentCache::open(&path);
        let value = cache.make_value(Some("行程".into()), 0.8, "t".into(), "claude".into());
        cache.insert(&test_key(), value);
        cache.flush();
    }
    let before = std::fs::metadata(&path).unwrap().modified().unwrap();

    let mut cache = JudgmentCache::open(&path);
    cache.flush_if_judged();
    assert_eq!(
        std::fs::metadata(&path).unwrap().modified().unwrap(),
        before,
        "a call that judged nothing must not rewrite the store"
    );

    let value = cache.make_value(Some("行程".into()), 0.8, "t".into(), "claude".into());
    cache.insert(&test_key(), value);
    cache.flush_if_judged();
    assert!(
        !cache.dirty,
        "a judgment has to reach disk on the call itself"
    );
}

#[test]
fn flush_and_reload() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("cache.json");
    let key = test_key();
    {
        let mut cache = JudgmentCache::open(&path);
        let value = cache.make_value(Some("行程".into()), 0.8, "test".into(), "claude".into());
        cache.insert(&key, value);
        cache.flush();
    }
    // Reload from disk.
    let mut cache2 = JudgmentCache::open(&path);
    assert_eq!(cache2.len(), 1);
    let hit = cache2.get(&key);
    assert!(hit.is_some());
}

#[test]
fn schema_mismatch_resets() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("cache.json");
    // Write a store with a different schema version.
    let old = serde_json::json!({
        "schema_version": 999,
        "entries": { "k1": { "chosen_replacement": null, "confidence": 0.5,
            "created_at": 0, "expires_at": 99999999999u64, "model_family": "x" } }
    });
    std::fs::write(&path, serde_json::to_string(&old).unwrap()).unwrap();
    let cache = JudgmentCache::open(&path);
    assert!(cache.is_empty());
    // Backup file should exist.
    assert!(path.with_extension("json.bak").exists());
}

#[test]
fn clear_empties_cache() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("cache.json");
    let mut cache = JudgmentCache::open(&path);
    let key = test_key();
    let value = cache.make_value(None, 0.1, String::new(), "claude".into());
    cache.insert(&key, value);
    assert_eq!(cache.len(), 1);
    cache.clear();
    assert!(cache.is_empty());
}

#[test]
fn normalize_context_strips_whitespace_and_trims() {
    let ctx = "  hello   world   with   lots   of   spaces  ";
    let normalized = normalize_context_for_cache(ctx);
    assert!(normalized.starts_with("v1:"));
    assert!(!normalized.contains(' '));
}

#[test]
fn normalize_context_different_semantics_differ() {
    // Two contexts that differ semantically should produce different keys.
    let ctx1 = "不，好的我們可以這樣做";
    let ctx2 = "不好的我們可以這樣做";
    let n1 = normalize_context_for_cache(ctx1);
    let n2 = normalize_context_for_cache(ctx2);
    // The comma is preserved (not whitespace), so these differ.
    assert_ne!(n1, n2);
}

#[test]
fn ttl_zero_disables_caching() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("cache.json");
    let mut cache = JudgmentCache::open(&path);
    cache.set_ttl_days(0);
    let key = test_key();
    let value = cache.make_value(Some("行程".into()), 0.9, String::new(), "claude".into());
    cache.insert(&key, value);
    assert!(cache.is_empty()); // not stored
}

#[test]
fn candidate_set_hash_order_independent() {
    let h1 = hash_candidate_set(&["行程".into(), "進程".into()]);
    let h2 = hash_candidate_set(&["進程".into(), "行程".into()]);
    assert_eq!(h1, h2);
}

#[test]
fn evict_expired_removes_old_entries() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("cache.json");
    let mut cache = JudgmentCache::open(&path);
    let key = test_key();
    let mut value = cache.make_value(Some("行程".into()), 0.9, String::new(), "claude".into());
    value.expires_at = 1; // expired
    cache.store.entries.insert(key_string(&key), value);
    assert_eq!(cache.len(), 1);
    cache.evict_expired();
    assert!(cache.is_empty());
}
