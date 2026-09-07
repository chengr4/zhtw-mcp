use super::*;
use serde_json::json;

#[test]
fn request_id_int_roundtrip() {
    let id = RequestId::Int(42);
    let json = serde_json::to_string(&id).unwrap();
    assert_eq!(json, "42");
    let parsed: RequestId = serde_json::from_str(&json).unwrap();
    assert!(matches!(parsed, RequestId::Int(42)));
}

#[test]
fn request_id_string_roundtrip() {
    let id = RequestId::Str("req-abc".into());
    let json = serde_json::to_string(&id).unwrap();
    assert_eq!(json, "\"req-abc\"");
    let parsed: RequestId = serde_json::from_str(&json).unwrap();
    match parsed {
        RequestId::Str(s) => assert_eq!(s, "req-abc"),
        _ => panic!("expected string id"),
    }
}

#[test]
fn request_id_negative_int() {
    let id = RequestId::Int(-1);
    let json = serde_json::to_string(&id).unwrap();
    let parsed: RequestId = serde_json::from_str(&json).unwrap();
    assert!(matches!(parsed, RequestId::Int(-1)));
}

#[test]
fn request_id_null_roundtrip() {
    let id = RequestId::Null;
    let json = serde_json::to_string(&id).unwrap();
    assert_eq!(json, "null");
    let parsed: RequestId = serde_json::from_str(&json).unwrap();
    assert!(matches!(parsed, RequestId::Null));
}

#[test]
fn request_id_boolean_rejected() {
    let result = serde_json::from_str::<RequestId>("true");
    assert!(result.is_err());
}

#[test]
fn request_id_array_rejected() {
    let result = serde_json::from_str::<RequestId>("[1,2]");
    assert!(result.is_err());
}

#[test]
fn request_null_id_is_not_notification() {
    // "id": null is a request, not a notification.
    let line = r#"{"jsonrpc":"2.0","method":"ping","id":null}"#;
    let req = parse_jsonrpc_line(line).unwrap();
    assert!(req.id.is_some(), "null id must be Some(RequestId::Null)");
    assert!(matches!(req.id, Some(RequestId::Null)));
}

#[test]
fn request_absent_id_is_notification() {
    let line = r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#;
    let req = parse_jsonrpc_line(line).unwrap();
    assert!(req.id.is_none(), "absent id must be None (notification)");
}

// -- JsonRpcError serde --

#[test]
fn jsonrpc_error_with_data_roundtrip() {
    let err = JsonRpcError {
        code: INVALID_REQUEST,
        message: "bad request".into(),
        data: Some(json!({"field": "profile", "accepted": ["base", "strict"]})),
    };
    let json = serde_json::to_string(&err).unwrap();

    // Read the wire, not the struct. Parsing back through the same derive that
    // produced the JSON is self-consistent by construction: a stray rename on a
    // field would survive it, while these pin the key names JSON-RPC 2.0
    // actually specifies.
    let parsed: Value = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed["code"], INVALID_REQUEST);
    assert_eq!(parsed["message"], "bad request");
    assert_eq!(parsed["data"]["field"], "profile");
}

#[test]
fn jsonrpc_error_without_data_omits_field() {
    let err = JsonRpcError {
        code: PARSE_ERROR,
        message: "parse error".into(),
        data: None,
    };
    let json = serde_json::to_string(&err).unwrap();
    let parsed: Value = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed["code"], PARSE_ERROR);
    assert!(parsed.get("data").is_none());
}

// -- JsonRpcResponse serde --

#[test]
fn response_success_omits_error() {
    let resp = JsonRpcResponse::success(Some(RequestId::Int(1)), json!("ok"));
    let json = serde_json::to_string(&resp).unwrap();
    let parsed: Value = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed["result"], "ok");
    assert!(parsed.get("error").is_none());
}

#[test]
fn response_error_omits_result() {
    let resp = JsonRpcResponse::error(Some(RequestId::Int(1)), -32600, "bad".into());
    let json = serde_json::to_string(&resp).unwrap();
    let parsed: Value = serde_json::from_str(&json).unwrap();
    assert!(parsed.get("result").is_none());
    assert_eq!(parsed["error"]["code"], -32600);
}

#[test]
fn response_unknown_id_serializes_as_null() {
    // JSON-RPC 2.0: error responses with unknown id must include "id": null
    let resp = JsonRpcResponse::error(None, PARSE_ERROR, "err".into());
    let json = serde_json::to_string(&resp).unwrap();
    let parsed: Value = serde_json::from_str(&json).unwrap();
    assert!(parsed.get("id").is_some(), "id field must be present");
    assert!(parsed["id"].is_null(), "unknown id must serialize as null");
}

// -- parse_jsonrpc_line --

#[test]
fn parse_valid_request() {
    let line = r#"{"jsonrpc":"2.0","method":"tools/list","id":1,"params":{}}"#;
    let req = parse_jsonrpc_line(line).unwrap();
    assert_eq!(req.method, "tools/list");
    assert!(matches!(req.id, Some(RequestId::Int(1))));
}

#[test]
fn parse_malformed_json_returns_parse_error() {
    let line = "not json at all";
    let err = parse_jsonrpc_line(line).unwrap_err();
    assert!(matches!(err, TransportError::Parse(_)));
    assert_eq!(err.error_code(), Some(PARSE_ERROR));
}

#[test]
fn parse_response_shaped_with_id_returns_stale() {
    // JSON-RPC 2.0: "The Server MUST NOT reply to a Response." Response-shaped
    // messages (has result/error, no method) are silently discarded regardless
    // of whether they carry an id.
    let line = r#"{"jsonrpc":"2.0","id":1,"result":"ok"}"#;
    let err = parse_jsonrpc_line(line).unwrap_err();
    assert!(matches!(err, TransportError::PeerResponse));
    assert_eq!(err.error_code(), None);
    assert!(err.into_response(None).is_none());
}

#[test]
fn parse_response_shaped_without_id_returns_stale() {
    let line = r#"{"jsonrpc":"2.0","result":"stale"}"#;
    let err = parse_jsonrpc_line(line).unwrap_err();
    assert!(matches!(err, TransportError::PeerResponse));
    assert_eq!(err.error_code(), None);
    assert!(err.into_response(None).is_none());
}

#[test]
fn parse_wrong_jsonrpc_version() {
    let line = r#"{"jsonrpc":"1.0","method":"test","id":1}"#;
    let err = parse_jsonrpc_line(line).unwrap_err();
    assert!(matches!(err, TransportError::InvalidRequest(..)));
}

#[test]
fn parse_notification_no_id() {
    let line = r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#;
    let req = parse_jsonrpc_line(line).unwrap();
    assert!(req.id.is_none());
    assert_eq!(req.method, "notifications/initialized");
}

#[test]
fn parse_empty_object_returns_invalid_request() {
    let line = r#"{}"#;
    let err = parse_jsonrpc_line(line).unwrap_err();
    assert!(matches!(err, TransportError::InvalidRequest(..)));
    assert_eq!(err.error_code(), Some(INVALID_REQUEST));
}

#[test]
fn parse_arbitrary_object_without_method_returns_invalid_request() {
    let line = r#"{"foo":"bar","baz":42}"#;
    let err = parse_jsonrpc_line(line).unwrap_err();
    assert!(matches!(err, TransportError::InvalidRequest(..)));
}

#[test]
fn parse_invalid_request_carries_id() {
    // A message with id but no method and not response-shaped should produce an
    // error response that echoes the id back to the client.
    let line = r#"{"id":99,"jsonrpc":"2.0"}"#;
    let err = parse_jsonrpc_line(line).unwrap_err();
    assert!(matches!(err, TransportError::InvalidRequest(..)));
    let resp = err.into_response(None).expect("should produce response");
    match &resp.id {
        Some(RequestId::Int(99)) => {}
        other => panic!("expected id=99, got {other:?}"),
    }
}

#[test]
fn parse_positional_array_returns_invalid_request() {
    // serde deserializes a struct from a positional sequence, so without an
    // explicit object check this parsed as a valid ping and the server answered
    // it.
    let line = r#"["2.0",1,"ping",{}]"#;
    let err = parse_jsonrpc_line(line).unwrap_err();
    assert!(matches!(err, TransportError::InvalidRequest(..)));
    assert_eq!(err.error_code(), Some(INVALID_REQUEST));
}

#[test]
fn parse_batch_array_returns_invalid_request() {
    // MCP does not support JSON-RPC batching; 2025-06-18 removed it.
    let line = r#"[{"jsonrpc":"2.0","method":"ping","id":1}]"#;
    let err = parse_jsonrpc_line(line).unwrap_err();
    assert!(matches!(err, TransportError::InvalidRequest(..)));
    assert_eq!(err.error_code(), Some(INVALID_REQUEST));
}

#[test]
fn parse_bare_scalar_returns_invalid_request() {
    // Valid JSON, not a JSON-RPC message.
    for line in [r#""ping""#, "42", "true", "null"] {
        let err = parse_jsonrpc_line(line).unwrap_err();
        assert!(
            matches!(err, TransportError::InvalidRequest(..)),
            "scalar {line} must be an invalid request"
        );
        assert_eq!(err.error_code(), Some(INVALID_REQUEST));
    }
}

#[test]
fn parse_surrounding_whitespace() {
    let line = "  \t{\"jsonrpc\":\"2.0\",\"method\":\"ping\",\"id\":3}  \t ";
    let req = parse_jsonrpc_line(line).unwrap();
    assert_eq!(req.method, "ping");
    assert!(matches!(req.id, Some(RequestId::Int(3))));
}

#[test]
fn parse_lone_surrogate_in_ignored_field_is_a_parse_error() {
    // Regression guard against reintroducing a from_str::<JsonRpcRequest>
    // shortcut: serde's ignore-unknown-field path skips strings without
    // decoding them, so this line parses as a request there but not as a Value.
    // Acting on it would mean executing a message our own JSON parser rejects.
    let line = r#"{"jsonrpc":"2.0","method":"ping","id":1,"x":"\ud800"}"#;
    let err = parse_jsonrpc_line(line).unwrap_err();
    assert!(matches!(err, TransportError::Parse(_)));
    assert_eq!(err.error_code(), Some(PARSE_ERROR));
}

#[test]
fn parse_duplicate_key_takes_the_last_value() {
    // Value applies last-key-wins, while serde's derived impl rejects duplicate
    // fields outright. Pinned because the two disagree.
    let line = r#"{"jsonrpc":"2.0","method":"ping","id":1,"id":2}"#;
    let req = parse_jsonrpc_line(line).unwrap();
    assert!(matches!(req.id, Some(RequestId::Int(2))));
}

#[test]
fn parse_duplicate_key_with_invalid_last_value_is_rejected() {
    let line = r#"{"jsonrpc":"2.0","method":"ping","id":1,"id":true}"#;
    let err = parse_jsonrpc_line(line).unwrap_err();
    assert!(matches!(err, TransportError::InvalidRequest(..)));
    let resp = err.into_response(None).expect("should produce response");
    assert!(resp.id.is_none());
}

#[test]
fn parse_non_object_error_response_has_null_id() {
    for line in [r#"["2.0",1,"ping",{}]"#, r#""ping""#] {
        let err = parse_jsonrpc_line(line).unwrap_err();
        let resp = err.into_response(None).expect("should produce response");
        assert!(resp.id.is_none(), "{line} must answer with a null id");
        let parsed: Value = serde_json::from_str(&serde_json::to_string(&resp).unwrap()).unwrap();
        assert!(parsed["id"].is_null());
    }
}

#[test]
fn parse_wrong_version_carries_id() {
    let line = r#"{"jsonrpc":"1.0","method":"test","id":7}"#;
    let err = parse_jsonrpc_line(line).unwrap_err();
    let resp = err.into_response(None).expect("should produce response");
    match &resp.id {
        Some(RequestId::Int(7)) => {}
        other => panic!("expected id=7, got {other:?}"),
    }
}

// -- TransportError --
