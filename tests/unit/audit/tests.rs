use super::*;

#[test]
fn hash_deterministic() {
    let a = hash_hex(b"hello");
    let b = hash_hex(b"hello");
    assert_eq!(a, b);
    assert_eq!(a.len(), 64);
}

#[test]
fn hash_known_value() {
    let h = hash_hex(b"hello");
    // BLAKE3 hash of "hello"
    assert_eq!(
        h,
        "ea8f163db38682925e4491c5e58d4bb3506ef8c14eb78a86e908c5624a67200f"
    );
}

#[test]
fn trace_unique_ids() {
    let t1 = Trace::new("zhtw", "abc", "text");
    let t2 = Trace::new("zhtw", "abc", "text");
    assert_ne!(t1.trace_id, t2.trace_id);
    // Same input → same input_hash
    assert_eq!(t1.input_hash, t2.input_hash);
}

#[test]
fn trace_output_hash() {
    let t = Trace::new("zhtw", "abc", "input")
        .with_output("output")
        .with_issue_count(3);
    assert!(t.output_hash.is_some());
    assert_eq!(t.issue_count, 3);
}

#[test]
fn timestamp_format() {
    let ts = now_iso8601();
    // Basic format check: YYYY-MM-DDTHH:MM:SS.mmmZ
    assert_eq!(ts.len(), 24);
    assert!(ts.ends_with('Z'));
    assert_eq!(&ts[4..5], "-");
    assert_eq!(&ts[10..11], "T");
    assert_eq!(&ts[19..20], ".");
}
