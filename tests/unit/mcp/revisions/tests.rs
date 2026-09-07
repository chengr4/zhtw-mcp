use super::*;

/// The two keys are spelled here and in RMCP. A rename on either side has
/// to fail loudly: read raw, a drifted key is not a compile error, it is a
/// server that quietly decides no client ever declares itself.
#[test]
fn the_keys_are_the_ones_rmcp_requires() {
    assert_eq!(
        [key::PROTOCOL_VERSION, key::CLIENT_CAPABILITIES],
        rmcp::model::RequestMetaObject::DRAFT_REQUIRED_KEYS,
        "SEP-2575 required-key spelling drifted from RMCP's"
    );
}

fn meta(version: &str, capabilities: Value) -> Map<String, Value> {
    let params = serde_json::json!({
        "_meta": {
            "io.modelcontextprotocol/protocolVersion": version,
            "io.modelcontextprotocol/clientCapabilities": capabilities,
        }
    });
    declaration(&params).unwrap().clone()
}

#[test]
fn only_a_handshake_free_revision_declares_itself() {
    assert!(is_self_declaring(&meta(
        "2026-07-28",
        serde_json::json!({})
    )));
    assert!(!is_self_declaring(&meta(
        "2025-06-18",
        serde_json::json!({})
    )));
    assert!(!is_self_declaring(&meta(
        "2099-01-01",
        serde_json::json!({})
    )));
}

#[test]
fn capabilities_are_part_of_the_declaration() {
    // Present but not an object is not a declaration: the revision puts a
    // capabilities object in every request, and half of one says nothing.
    assert!(!is_self_declaring(&meta(
        "2026-07-28",
        serde_json::json!("nope")
    )));
    let no_capabilities = serde_json::json!({
        "_meta": { "io.modelcontextprotocol/protocolVersion": "2026-07-28" }
    });
    assert!(!is_self_declaring(
        declaration(&no_capabilities).expect("params carry a `_meta`")
    ));
}

#[test]
fn the_logging_extension_is_read_off_the_capabilities() {
    assert!(logging_opt_in(&meta(
        "2026-07-28",
        serde_json::json!({ "logging": {} })
    )));
    assert!(!logging_opt_in(&meta(
        "2026-07-28",
        serde_json::json!({ "roots": {} })
    )));
}

#[test]
fn params_without_a_meta_declare_nothing() {
    assert!(declaration(&serde_json::json!({})).is_none());

    // Array params reach the gate too, and indexing one by a string key is not
    // a lookup that can succeed.
    assert!(declaration(&serde_json::json!([1, 2])).is_none());
}

#[test]
fn a_revision_without_a_handshake_is_never_offered_by_one() {
    // The refusal for an unsupported initialize names what the client could ask
    // for instead, so a revision that has no initialize must not appear there:
    // it would send the client back to the method that just failed. The table
    // is what keeps the two lists in step.
    //
    // 2026-07-28 is named outright rather than left to the loop below, which
    // passes for free if nothing is marked as lacking a handshake. That it
    // deleted initialize is a fact about the revision, not a preference, so the
    // table is wrong if it ever says otherwise.
    let negotiable = negotiable_protocol_versions();
    assert!(
        supported_protocol_versions().contains(&ProtocolVersion::V_2026_07_28),
        "2026-07-28 is served, through server/discover"
    );
    assert!(
        !negotiable.contains(&ProtocolVersion::V_2026_07_28),
        "2026-07-28 has no initialize, so it cannot be negotiated by one"
    );
    for revision in REVISIONS.iter().filter(|r| !r.handshake) {
        assert!(
            !negotiable.contains(&revision.version),
            "{} has no handshake but is offered as one to negotiate",
            revision.version
        );
    }
    assert!(
        !negotiable.is_empty(),
        "some revision has to be reachable through initialize"
    );
}

#[test]
fn every_served_revision_is_advertised() {
    // server/discover is the only place a client can learn about a revision it
    // cannot negotiate, so the advertised list is all of them.
    let supported = supported_protocol_versions();
    assert_eq!(supported.len(), REVISIONS.len());
    for revision in REVISIONS {
        assert!(supported.contains(&revision.version));
    }
}
