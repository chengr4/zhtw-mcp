// MCP JSON-RPC 2.0 types for stdio transport.
//
// Covers the MCP protocol: initialize (with capability negotiation),
// tools/list, tools/call, resources/list, resources/read, prompts/list,
// prompts/get.

use serde::{Deserialize, Serialize};
use serde_json::Value;

// JSON-RPC base types

/// A JSON-RPC 2.0 request ID (integer, string, or null).
///
/// JSON-RPC 2.0 allows `"id": null` in requests. A request with an explicit
/// null id is NOT a notification (notifications omit the id field entirely).
/// Error responses for requests with null id must include `"id": null`.
#[derive(Debug, Clone)]
pub enum RequestId {
    Int(i64),
    Str(String),
    /// Explicit `"id": null` in the request. Distinct from absent id
    /// (which indicates a notification).
    Null,
}

impl Serialize for RequestId {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        match self {
            RequestId::Int(i) => serializer.serialize_i64(*i),
            RequestId::Str(s) => serializer.serialize_str(s),
            RequestId::Null => serializer.serialize_unit(),
        }
    }
}

impl<'de> Deserialize<'de> for RequestId {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let v = Value::deserialize(deserializer)?;
        match &v {
            Value::Null => Ok(RequestId::Null),
            Value::Number(n) => n
                .as_i64()
                .map(RequestId::Int)
                .ok_or_else(|| serde::de::Error::custom("id number must be an integer")),
            Value::String(s) => Ok(RequestId::Str(s.clone())),
            _ => Err(serde::de::Error::custom(
                "id must be a string, integer, or null",
            )),
        }
    }
}

/// Incoming JSON-RPC request (method call or notification).
///
/// When `id` is `None`, this is a notification (no response expected).
/// When `id` is `Some(RequestId::Null)`, the client sent `"id": null`
/// and still expects a response.
#[derive(Debug, Deserialize)]
pub struct JsonRpcRequest {
    pub jsonrpc: String,
    #[serde(default, deserialize_with = "deserialize_request_id")]
    pub id: Option<RequestId>,
    pub method: String,
    #[serde(default)]
    pub params: Value,
}

/// Deserialize the `id` field so that `"id": null` becomes
/// `Some(RequestId::Null)` rather than `None` (which serde's default
/// `Option<T>` handling would produce).  An absent field still yields
/// `None` via `#[serde(default)]`.
fn deserialize_request_id<'de, D>(deserializer: D) -> Result<Option<RequestId>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    RequestId::deserialize(deserializer).map(Some)
}

/// Outgoing JSON-RPC response.
///
/// The `id` field is always serialized per JSON-RPC 2.0: `None` produces
/// `"id": null` (required for error responses when the request id is
/// unknown).
#[derive(Debug, Serialize)]
pub struct JsonRpcResponse {
    pub jsonrpc: &'static str,
    pub id: Option<RequestId>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<JsonRpcError>,
}

impl JsonRpcResponse {
    pub fn success(id: Option<RequestId>, result: Value) -> Self {
        Self {
            jsonrpc: JSONRPC_VERSION,
            id,
            result: Some(result),
            error: None,
        }
    }

    pub fn error(id: Option<RequestId>, code: i64, message: String) -> Self {
        Self {
            jsonrpc: JSONRPC_VERSION,
            id,
            result: None,
            error: Some(JsonRpcError {
                code,
                message,
                data: None,
            }),
        }
    }
}

#[derive(Debug, Serialize)]
pub struct JsonRpcError {
    pub code: i64,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<Value>,
}

// MCP protocol types

// Transport error types

/// Structured transport error for distinguishing failure modes in the
/// dispatch loop. Maps to specific JSON-RPC error codes:
///   - Parse → -32700 (PARSE_ERROR)
///   - InvalidRequest → -32600 (INVALID_REQUEST)
///   - PeerResponse → no reply, and passed to the SDK to correlate
#[derive(Debug)]
pub enum TransportError {
    /// Input is not valid JSON (malformed syntax).
    Parse(serde_json::Error),
    /// Valid JSON but not a valid JSON-RPC request (missing method, wrong
    /// version, response-shaped message with id, etc.).  Carries the
    /// extracted request id (if any) for error response correlation.
    InvalidRequest(Option<RequestId>, String),
    /// Response-shaped message (has result/error, no method): a reply to a
    /// request this server sent, not a request to serve.
    ///
    /// Not an error so much as a different kind of message. It gets no reply
    /// of its own, per JSON-RPC 2.0 ("The Server MUST NOT reply to a
    /// Response"), but it is not discarded either: the caller hands it to the
    /// SDK, which owns the ids it has outstanding and is the only thing that
    /// can match a reply to its request.
    PeerResponse,
}

impl TransportError {
    /// JSON-RPC error code for this transport error, if applicable.
    /// Returns None for PeerResponse, which is not answered at all.
    pub fn error_code(&self) -> Option<i64> {
        match self {
            TransportError::PeerResponse => None,
            TransportError::Parse(_) => Some(PARSE_ERROR),
            TransportError::InvalidRequest(..) => Some(INVALID_REQUEST),
        }
    }

    /// Human-readable error message for JSON-RPC error responses.
    pub fn error_message(&self) -> String {
        match self {
            TransportError::PeerResponse => "response to a server request".into(),
            TransportError::Parse(e) => format!("parse error: {e}"),
            TransportError::InvalidRequest(_, msg) => format!("invalid request: {msg}"),
        }
    }
}

impl std::fmt::Display for TransportError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TransportError::Parse(e) => write!(f, "JSON parse: {e}"),
            TransportError::InvalidRequest(_, msg) => write!(f, "invalid request: {msg}"),
            TransportError::PeerResponse => write!(f, "response to a server request"),
        }
    }
}

impl std::error::Error for TransportError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            TransportError::Parse(e) => Some(e),
            _ => None,
        }
    }
}

impl TransportError {
    /// Build a JSON-RPC error response for this transport error, if one
    /// should be sent.  Returns None for PeerResponse.
    ///
    /// For InvalidRequest, the carried id (extracted during parsing) is
    /// used so the client can correlate the error with its original request.
    /// The `fallback_id` is used for Parse, where the input never parsed far
    /// enough to yield an id.
    pub fn into_response(self, fallback_id: Option<RequestId>) -> Option<JsonRpcResponse> {
        let code = self.error_code()?;
        let id = match &self {
            TransportError::InvalidRequest(carried_id, _) => carried_id.clone(),
            _ => fallback_id,
        };
        let message = self.error_message();
        Some(JsonRpcResponse::error(id, code, message))
    }
}

/// Parse a raw JSON line into a validated JsonRpcRequest.
///
/// Returns TransportError variants that preserve the distinction between
/// malformed JSON (Parse → -32700) and valid-JSON-but-invalid-JSON-RPC
/// (InvalidRequest → -32600).
pub fn parse_jsonrpc_line(line: &str) -> Result<JsonRpcRequest, TransportError> {
    // Step 1: parse as generic JSON.
    //
    // Everything goes through Value. Deserializing straight into JsonRpcRequest
    // is measurably faster but not equivalent: serde's ignore-unknown-field
    // path skips strings without decoding them, so a lone UTF-16 surrogate
    // escape in a field this struct does not name parses fine there and fails
    // here. The server must not act on a line its own JSON parser rejects, and
    // a few hundred nanoseconds do not buy that.
    let obj: serde_json::Value = serde_json::from_str(line).map_err(TransportError::Parse)?;

    // Step 2: a JSON-RPC message is an object. Arrays (batches, which MCP does
    // not support, and positional forms) and bare scalars are invalid requests.
    // No id can be recovered from them, so none is echoed.
    //
    // This check is what keeps serde from building a JsonRpcRequest out of a
    // positional array in step 5: '["2.0",1,"ping",{}]' deserializes into the
    // struct just as happily as an object does.
    if !obj.is_object() {
        return Err(TransportError::InvalidRequest(
            None,
            "request must be a JSON object".into(),
        ));
    }

    // Step 3: extract id once for error correlation across all branches.
    // id_present is true when the JSON has an "id" key (even if null or an
    // unparseable type like boolean/array). raw_id is the parsed id when it's a
    // valid string, integer, or null.
    let id_value = obj.get("id");
    let raw_id: Option<RequestId> = id_value.and_then(|v| serde_json::from_value(v.clone()).ok());
    let id_present = id_value.is_some();

    // Step 4: handle messages without a method field.
    if obj.get("method").is_none() {
        let is_response = obj.get("result").is_some() || obj.get("error").is_some();
        if is_response {
            // Response-shaped (has result/error, no method): silently discard.
            // JSON-RPC 2.0: "The Server MUST NOT reply to a Response." Covers
            // late sampling responses (with id) and orphaned responses (without
            // id).
            return Err(TransportError::PeerResponse);
        }
        if id_present {
            // Has id, no method, not response-shaped: genuinely invalid.
            return Err(TransportError::InvalidRequest(
                raw_id,
                "message has id but no method".into(),
            ));
        }

        // No id, no method, no result/error: genuinely invalid request (e.g.
        // "{}" or '{"foo":"bar"}').
        return Err(TransportError::InvalidRequest(
            None,
            "object has no method, result, or error field".into(),
        ));
    }

    // Step 5: convert to typed request.
    let req: JsonRpcRequest = serde_json::from_value(obj)
        .map_err(|e| TransportError::InvalidRequest(raw_id.clone(), e.to_string()))?;

    // Step 6: validate JSON-RPC version.
    if req.jsonrpc != JSONRPC_VERSION {
        return Err(TransportError::InvalidRequest(
            raw_id,
            format!(
                "expected jsonrpc \"{JSONRPC_VERSION}\", got \"{}\"",
                req.jsonrpc
            ),
        ));
    }

    Ok(req)
}

// Protocol constants

pub const JSONRPC_VERSION: &str = "2.0";

// Standard JSON-RPC error codes.
pub const PARSE_ERROR: i64 = -32700;
pub const INVALID_REQUEST: i64 = -32600;
pub const SERVER_NOT_INITIALIZED: i64 = -32002;

#[cfg(test)]
#[path = "../../tests/unit/mcp/types/tests.rs"]
mod tests;
