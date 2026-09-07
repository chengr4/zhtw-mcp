use super::*;

fn test_server() -> (Mutex<Server>, tempfile::TempDir) {
    let dir = tempfile::tempdir().expect("temp dir");
    let server = Server::new(
        crate::rules::store::OverrideStore::open(&dir.path().join("overrides.json")).unwrap(),
        crate::rules::store::SuppressionStore::open(&dir.path().join("suppressions.json")).unwrap(),
        crate::rules::store::PackStore::new(dir.path().join("packs")),
        vec![],
        None,
    )
    .expect("build server");
    (Mutex::new(server), dir)
}

#[test]
fn the_server_wires_its_cache_flush_into_the_exit() {
    // exit terminates in the framing layer now, and that layer cannot reach the
    // judgment cache. Losing this wiring costs the cache on every clean exit
    // and fails nothing else, which is why it is asserted here rather than left
    // to an end-to-end test that would still pass.
    let (server, _dir) = test_server();
    let sdk = SdkServer::new(server.into_inner().expect("unpoisoned"));
    assert!(
        sdk.lifecycle().exit_hook_installed(),
        "new must hand the flush to the lifecycle"
    );
}

/// Poison a lock the way a panicking handler does.
fn poison(inner: &Mutex<Server>) {
    let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let _guard = inner.lock().unwrap();
        panic!("handler panicked");
    }));
    assert!(inner.is_poisoned(), "the lock should now be poisoned");
}

#[test]
fn a_panicked_handler_does_not_cost_the_judgment_cache() {
    // try_lock reports a poisoned lock as an error even when nothing holds it,
    // so reading every error as "a scan is in flight" threw the flush away for
    // the rest of the process once any handler panicked.
    let (inner, _dir) = test_server();
    poison(&inner);
    assert_eq!(flush_before_exit(&inner), Flushed::Yes);
}

#[test]
fn a_scan_in_flight_is_left_to_finish_without_the_flush() {
    // The other half, and it is about contention alone: a lock genuinely held
    // is what the warning is for. Poisoning it as well would pass only because
    // try_lock reports contention ahead of poison.
    let (inner, _dir) = test_server();
    let _held = inner.lock().expect("a fresh lock is not poisoned");
    assert_eq!(flush_before_exit(&inner), Flushed::SkippedForScanInFlight);
}

#[test]
fn an_unadvertised_method_is_not_listed_as_implemented() {
    // The list is maintained by hand, and this is the entry that costs
    // something when it drifts. completion/complete is refused because the
    // completions capability is unadvertised; listing it here would make the
    // same method answer method-not-found on good parameters and invalid-params
    // on bad ones, which is the bug the list exists to prevent rather than one
    // it should introduce.
    assert!(
        !IMPLEMENTED_METHODS.contains(&"completion/complete"),
        "completion/complete is refused, so it must not count as implemented"
    );
}

#[test]
fn implemented_methods_has_no_duplicates() {
    // A duplicate is invisible at the call site (contains() still says true)
    // but means someone edited the list twice for one method, which is the
    // state where the next edit removes only one of them.
    let unique: std::collections::BTreeSet<_> = IMPLEMENTED_METHODS.iter().collect();
    assert_eq!(
        unique.len(),
        IMPLEMENTED_METHODS.len(),
        "duplicate entry in IMPLEMENTED_METHODS"
    );
}

/// A client reply carrying the given text blocks, in order.
///
/// Written as the wire payload and parsed back, the same way the sampling
/// tests script their canned replies: a shape a real client could send is
/// then a shape this test can build.
// Deprecated by SEP-2577 along with the sampling API this exercises; the allow
// matches the one on reply_text itself.
#[allow(deprecated)]
fn sampling_reply(blocks: &[&str]) -> rmcp::model::CreateMessageResult {
    let content: Vec<_> = blocks
        .iter()
        .map(|t| serde_json::json!({"type": "text", "text": t}))
        .collect();
    serde_json::from_value(serde_json::json!({
        "role": "assistant",
        "model": "test-model",
        "content": content,
    }))
    .expect("a CreateMessageResult payload")
}

#[test]
fn a_sampling_reply_yields_its_first_non_blank_text() {
    // The model is free to lead with an empty or whitespace-only block, and
    // taking it would hand the caller "" as though that were the judgment.
    // First block with something in it wins, trimmed.
    assert_eq!(
        reply_text(sampling_reply(&["  ", "", "  軟體  ", "檔案"])),
        Some("軟體".to_string())
    );
}

#[test]
fn a_sampling_reply_with_nothing_to_say_is_none() {
    assert_eq!(reply_text(sampling_reply(&[])), None);
    assert_eq!(reply_text(sampling_reply(&["", "   ", "\n\t"])), None);
}
