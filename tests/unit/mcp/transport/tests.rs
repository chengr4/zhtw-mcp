use super::*;
use std::io;
use std::pin::Pin;
use std::task::{Context, Poll};

/// Fails at one of the two points a write can fail, with a caller-chosen
/// kind so the tests can tell a carried error from a synthesized one.
struct FailingWriter {
    on_flush: bool,
    kind: io::ErrorKind,
}

impl FailingWriter {
    fn on_write(kind: io::ErrorKind) -> Self {
        Self {
            on_flush: false,
            kind,
        }
    }

    /// The shape `tokio::io::stdout` actually fails in: it buffers, so the
    /// write reports success and the error arrives on the flush behind it.
    fn on_flush(kind: io::ErrorKind) -> Self {
        Self {
            on_flush: true,
            kind,
        }
    }
}

impl AsyncWrite for FailingWriter {
    fn poll_write(
        self: Pin<&mut Self>,
        _: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        if self.on_flush {
            Poll::Ready(Ok(buf.len()))
        } else {
            Poll::Ready(Err(io::Error::from(self.kind)))
        }
    }

    fn poll_flush(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<io::Result<()>> {
        if self.on_flush {
            Poll::Ready(Err(io::Error::from(self.kind)))
        } else {
            Poll::Ready(Ok(()))
        }
    }

    fn poll_shutdown(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<io::Result<()>> {
        Poll::Ready(Ok(()))
    }
}

fn frame(answers: Option<PeerRequestId>) -> (Outbound, oneshot::Receiver<io::Result<()>>) {
    let (done, written) = oneshot::channel();
    (
        Outbound {
            frame: b"frame\n".to_vec(),
            answers,
            done: Some(done),
        },
        written,
    )
}

fn lifecycle() -> Lifecycle {
    Lifecycle::default()
}

#[tokio::test]
async fn a_failure_at_either_point_reaches_the_sender_and_stops_the_read() {
    // Both points matter. A real stdout buffers, so the write reports success
    // and the error only shows up when the buffer is pushed out: losing the
    // flush would leave that case undetected entirely.
    for mut out in [
        FailingWriter::on_write(io::ErrorKind::ConnectionReset),
        FailingWriter::on_flush(io::ErrorKind::PermissionDenied),
    ] {
        let kind = out.kind;
        let lifecycle = lifecycle();
        let (tx, mut rx) = mpsc::unbounded_channel();
        let (item, written) = frame(None);
        tx.send(item).unwrap();
        drop(tx);

        write_outbound(&mut out, &mut rx, &lifecycle).await;
        assert_eq!(written.await.unwrap().unwrap_err().kind(), kind);

        // RMCP only logs the send errors, so raising this is what stops the
        // server accepting requests it can no longer answer.
        tokio::time::timeout(Duration::from_secs(1), lifecycle.write_failed_wait())
            .await
            .expect("a write failure has to stop the read side");
    }
}

#[tokio::test]
async fn writer_failure_retires_queued_responses() {
    let lifecycle = lifecycle();
    let (tx, mut rx) = mpsc::unbounded_channel();
    let mut queued = Vec::new();
    for id in [1, 2] {
        let id = PeerRequestId::Number(id);
        lifecycle.accept_request(id.clone());
        let (item, written) = frame(Some(id));
        tx.send(item).unwrap();
        queued.push(written);
    }

    // tx stays alive on purpose. Both the lifecycle and the transport hold
    // sender clones in the real thing, so the drain has to close the receiver
    // itself; without that this waits forever for a frame that can no longer be
    // written.
    let mut out = FailingWriter::on_write(io::ErrorKind::ConnectionReset);
    let drained = tokio::time::timeout(
        Duration::from_secs(1),
        write_outbound(&mut out, &mut rx, &lifecycle),
    )
    .await;
    assert!(drained.is_ok(), "the drain must not wait on a live sender");

    assert_eq!(lifecycle.outstanding(), 0);
    // Every frame behind the failure learns why, not a stand-in for it.
    for written in queued {
        assert_eq!(
            written.await.unwrap().unwrap_err().kind(),
            io::ErrorKind::ConnectionReset
        );
    }
    assert!(
        tx.send(frame(None).0).is_err(),
        "a closed queue must reject anything sent after the failure"
    );
}

#[tokio::test]
async fn a_parked_read_gives_up_when_stdout_fails() {
    // The whole point of the signal. Stdout dying does not close stdin, so a
    // failure noticed only between lines would never be noticed at all: this
    // read is parked on a client that has stopped talking.
    let lifecycle = Arc::new(lifecycle());
    // Held open, so the read parks rather than seeing end of input.
    let (client, _server) = tokio::io::duplex(64);
    let read = tokio::spawn({
        let lifecycle = lifecycle.clone();
        async move {
            let mut reader = BufReader::new(client);
            let mut raw = Vec::new();
            read_line_unless_write_failed(&mut reader, &mut raw, &lifecycle)
                .await
                .is_none()
        }
    });

    // Let the read park before the failure lands, which is the ordering that
    // has no other way out.
    tokio::task::yield_now().await;

    lifecycle.mark_write_failed();

    let gave_up = tokio::time::timeout(Duration::from_secs(1), read)
        .await
        .expect("a parked read must give up, not wait on input that is not coming")
        .unwrap();
    assert!(gave_up, "the failure has to win the select, not the read");
}

fn req(method: &str, id: Option<i64>) -> super::super::types::JsonRpcRequest {
    super::super::types::JsonRpcRequest {
        jsonrpc: "2.0".into(),
        id: id.map(RequestId::Int),
        method: method.into(),
        params: serde_json::json!({}),
    }
}

/// A request declaring the handshake-free revision the way its clients do.
fn declared(method: &str, id: i64) -> super::super::types::JsonRpcRequest {
    let mut request = req(method, Some(id));
    request.params = serde_json::json!({
        "_meta": {
            "io.modelcontextprotocol/protocolVersion": "2026-07-28",
            "io.modelcontextprotocol/clientCapabilities": {}
        }
    });
    request
}

#[test]
fn pre_init_request_is_rejected_not_fatal() {
    let lc = lifecycle();
    let Gate::Reply(response) = gate(&lc, &req("tools/list", Some(1))) else {
        panic!("a pre-init request must be answered, not dropped");
    };
    assert_eq!(response.error.unwrap().code, SERVER_NOT_INITIALIZED);
}

#[test]
fn self_declaring_request_is_served_without_opening_the_gate() {
    // 2026-07-28 clients open a connection per call and declare the revision in
    // _meta, so this must be served, not refused. What it must not do is turn
    // that declaration into connection state: the undeclared follow-up is how
    // the gate proves it stayed shut.
    let lc = lifecycle();
    assert!(matches!(
        gate(&lc, &declared("tools/list", 1)),
        Gate::Forward
    ));

    let Gate::Reply(response) = gate(&lc, &req("tools/call", Some(2))) else {
        panic!("an undeclared follow-up request must be refused");
    };
    assert_eq!(response.error.unwrap().code, SERVER_NOT_INITIALIZED);
}

#[test]
fn an_active_stateless_request_allows_its_peer_reply() {
    let lc = lifecycle();
    assert!(!lc.may_route_peer_response());
    lc.accept_request(PeerRequestId::Number(1));
    assert!(lc.may_route_peer_response());
    lc.retire_request(&PeerRequestId::Number(1));
    assert!(!lc.may_route_peer_response());
}

#[test]
fn a_handshake_revision_in_meta_does_not_skip_the_handshake() {
    // _meta is not where the older revisions carry the protocol version, so
    // naming one there buys no exemption from their initialize.
    let lc = lifecycle();
    let mut request = req("tools/list", Some(1));
    request.params = serde_json::json!({
        "_meta": { "io.modelcontextprotocol/protocolVersion": "2025-06-18" }
    });
    let Gate::Reply(response) = gate(&lc, &request) else {
        panic!("a request that declares a handshake revision must be refused");
    };
    assert_eq!(response.error.unwrap().code, SERVER_NOT_INITIALIZED);
    assert!(!lc.initialized.load(Ordering::Relaxed));
}

#[test]
fn pre_init_notification_is_dropped() {
    let lc = lifecycle();
    assert!(matches!(
        gate(&lc, &req("notifications/initialized", None)),
        Gate::Drop
    ));
}

#[test]
fn pre_init_ping_is_answered() {
    let lc = lifecycle();
    let Gate::Reply(response) = gate(&lc, &req("ping", Some(1))) else {
        panic!("pre-init ping must be answered");
    };
    assert!(response.result.is_some());
}

#[test]
fn successful_initialize_opens_the_gate() {
    let lc = lifecycle();
    assert!(matches!(
        gate(&lc, &req("initialize", Some(1))),
        Gate::Forward
    ));
    lc.mark_initialized();
    assert!(matches!(
        gate(&lc, &req("tools/list", Some(2))),
        Gate::Forward
    ));
}

#[test]
fn notification_with_id_is_rejected() {
    let lc = lifecycle();
    lc.initialized.store(true, Ordering::Relaxed);
    let Gate::Reply(response) = gate(&lc, &req("notifications/cancelled", Some(3))) else {
        panic!("an id-bearing notification must be answered");
    };
    assert_eq!(response.error.unwrap().code, INVALID_REQUEST);
}

#[test]
fn after_shutdown_everything_but_exit_is_rejected() {
    let lc = lifecycle();
    lc.initialized.store(true, Ordering::Relaxed);
    lc.mark_shutdown();
    assert!(matches!(gate(&lc, &req("exit", None)), Gate::Exit));
    let Gate::Reply(response) = gate(&lc, &req("tools/list", Some(4))) else {
        panic!("a post-shutdown request must be answered");
    };
    assert_eq!(response.error.unwrap().code, INVALID_REQUEST);
}

#[test]
fn discover_forwards_without_opening_the_gate() {
    // Forwarded whatever it declares: the branch above the declaration check
    // takes it, because the version list is the one answer a client on an
    // unknown revision still needs. Declaring is therefore not what is under
    // test, and passing a declaration would hide which branch answered.
    let lc = lifecycle();
    assert!(matches!(
        gate(&lc, &req("server/discover", Some(1))),
        Gate::Forward
    ));
    assert!(!lc.initialized(), "discovery must not mark the session up");
}

#[test]
fn pre_init_exit_terminates_rather_than_forwarding() {
    // Forwarding it would reach RMCP as a failed handshake, which ends the
    // session with the wrong status and an error the client did not cause. The
    // gate decides that this terminates; what status it terminates with is read
    // at the exit, from the same flag either path consults.
    let lc = lifecycle();
    assert!(matches!(gate(&lc, &req("exit", None)), Gate::Exit));
    assert_eq!(lc.exit_code(), 1);
    lc.mark_shutdown();
    assert!(matches!(gate(&lc, &req("exit", None)), Gate::Exit));
    assert_eq!(lc.exit_code(), 0);
}

#[test]
fn pre_init_shutdown_is_answered_not_forwarded() {
    // Forwarding it would reach RMCP as a failed handshake and end the session,
    // which is the one thing the pre-init gate exists to prevent.
    let lc = lifecycle();
    let Gate::Reply(response) = gate(&lc, &req("shutdown", Some(1))) else {
        panic!("a pre-init shutdown must be answered here");
    };
    assert!(response.result.is_some());
    assert_eq!(lc.exit_code(), 0);
}

#[test]
fn shutdown_as_a_notification_sets_the_flag_without_replying() {
    let lc = lifecycle();
    lc.initialized.store(true, Ordering::Relaxed);
    assert!(matches!(gate(&lc, &req("shutdown", None)), Gate::Drop));
    assert_eq!(lc.exit_code(), 0);
}

#[test]
fn a_handshake_does_not_move_the_exit_off_the_gate() {
    // The case the other two gate tests do not reach: initialized and not
    // shutting down, which is where exit used to be forwarded. That put
    // termination on a task RMCP spawns and raced it against the read side
    // ending the session. Anything but Gate::Exit here brings that back.
    // Pre-init is pinned by pre_init_exit_terminates_rather_than_forwarding and
    // post-shutdown by after_shutdown_everything_but_exit_is_rejected.
    let lc = lifecycle();
    lc.initialized.store(true, Ordering::Relaxed);
    assert!(matches!(gate(&lc, &req("exit", None)), Gate::Exit));
}

#[test]
fn the_exit_hook_keeps_its_first_owner() {
    // A second install would let late wiring displace the flush that the exit
    // is about to run.
    let lc = lifecycle();
    let ran = Arc::new(AtomicBool::new(false));
    let first = ran.clone();
    lc.set_exit_hook(move || first.store(true, Ordering::Relaxed));
    lc.set_exit_hook(|| unreachable!("the second install must not take"));

    lc.on_exit.get().expect("a hook is installed")();
    assert!(
        ran.load(Ordering::Relaxed),
        "the first hook is the one kept"
    );
}

#[test]
fn exit_code_is_one_without_shutdown() {
    assert_eq!(lifecycle().exit_code(), 1);
}

#[test]
fn shutdown_closes_the_gate_before_its_handler_runs() {
    let lc = lifecycle();
    lc.initialized.store(true, Ordering::Relaxed);
    assert!(matches!(
        gate(&lc, &req("shutdown", Some(1))),
        Gate::Reply(_)
    ));
    let Gate::Reply(response) = gate(&lc, &req("tools/call", Some(2))) else {
        panic!("a pipelined request after shutdown must be answered here");
    };
    assert_eq!(response.error.unwrap().code, INVALID_REQUEST);
}

#[tokio::test]
async fn oversize_line_is_drained_and_the_next_line_parses() {
    let big = "x".repeat(MAX_LINE_BYTES + 10);
    let input = format!("{big}\n{{\"jsonrpc\":\"2.0\"}}\n");
    let mut reader = BufReader::new(input.as_bytes());
    let mut raw = Vec::new();

    assert!(matches!(
        read_line(&mut reader, &mut raw).await.unwrap(),
        ReadLine::TooLong
    ));
    let ReadLine::Line(next) = read_line(&mut reader, &mut raw).await.unwrap() else {
        panic!("the line after an oversize one must still parse");
    };
    assert_eq!(next, "{\"jsonrpc\":\"2.0\"}");
}

#[tokio::test(start_paused = true)]
async fn end_of_input_waits_for_an_accepted_request() {
    let lifecycle = Lifecycle::default();
    lifecycle.accept_request(PeerRequestId::Number(1));

    let mut deadline = None;
    let drain = std::pin::pin!(drain_in_flight(&lifecycle, &mut deadline));
    // Well past DRAIN_POLL, and with time paused it costs no wall clock.
    let waited = tokio::time::timeout(DRAIN_TIMEOUT / 2, drain).await;
    assert!(waited.is_err(), "a request still running holds the drain");

    lifecycle.retire_request(&PeerRequestId::Number(1));
    tokio::time::timeout(DRAIN_TIMEOUT, drain_in_flight(&lifecycle, &mut None))
        .await
        .expect("the drain returns once the response exists");
}

#[tokio::test]
async fn a_reused_id_does_not_hold_the_drain_open() {
    // RMCP keeps one cancellation-token entry per request id, so a client that
    // reuses an id while the first request is still running is answered once
    // and RMCP discards the other response. Counting two owed responses here
    // would hold end of input open for the full drain deadline waiting for a
    // reply that no longer exists: measured at 30s to exit instead of 0s. One
    // entry per id is what RMCP will deliver.
    let lifecycle = Lifecycle::default();
    let id = PeerRequestId::Number(7);
    lifecycle.accept_request(id.clone());
    lifecycle.accept_request(id.clone());
    assert_eq!(lifecycle.outstanding(), 1, "one id, one response owed");

    lifecycle.retire_request(&id);
    assert_eq!(lifecycle.outstanding(), 0);
    tokio::time::timeout(DRAIN_TIMEOUT, drain_in_flight(&lifecycle, &mut None))
        .await
        .expect("the one response RMCP sends must release the drain");
}

#[test]
fn retiring_a_request_twice_is_harmless() {
    // The double retire the set exists to absorb: a cancellation settles the
    // request, and a response already past RMCP's cancellation check arrives
    // afterwards and settles it again.
    let lifecycle = Lifecycle::default();
    let id = PeerRequestId::Number(1);
    lifecycle.accept_request(id.clone());
    lifecycle.retire_request(&id);
    lifecycle.retire_request(&id);
    lifecycle.retire_request(&PeerRequestId::Number(99));
    assert_eq!(lifecycle.outstanding(), 0);

    // Still usable afterwards rather than stuck at a wrapped count.
    lifecycle.accept_request(id);
    assert_eq!(lifecycle.outstanding(), 1);
}

#[test]
fn distinct_ids_are_tracked_separately() {
    let lifecycle = Lifecycle::default();
    lifecycle.accept_request(PeerRequestId::Number(1));
    lifecycle.accept_request(PeerRequestId::Number(2));
    assert_eq!(lifecycle.outstanding(), 2);

    lifecycle.retire_request(&PeerRequestId::Number(2));
    assert_eq!(
        lifecycle.outstanding(),
        1,
        "answering one request must not settle the other"
    );
    lifecycle.retire_request(&PeerRequestId::Number(1));
    assert_eq!(lifecycle.outstanding(), 0);
}

#[tokio::test]
async fn end_of_input_stops_when_stdout_fails() {
    let lifecycle = Lifecycle::default();
    lifecycle.accept_request(PeerRequestId::Number(1));

    let mut deadline = None;
    let mut drain = std::pin::pin!(drain_in_flight(&lifecycle, &mut deadline));
    let waited = tokio::time::timeout(Duration::from_millis(20), &mut drain).await;
    assert!(waited.is_err(), "an in-flight request starts the EOF drain");

    lifecycle.mark_write_failed();
    tokio::time::timeout(Duration::from_secs(1), drain)
        .await
        .expect("a failed stdout stops the EOF drain immediately");
}

#[tokio::test(start_paused = true)]
async fn end_of_input_gives_up_on_a_request_that_never_finishes() {
    let lifecycle = Lifecycle::default();
    lifecycle.accept_request(PeerRequestId::Number(1));

    // Bounded, so a wedged handler cannot keep the process alive.
    tokio::time::timeout(DRAIN_TIMEOUT * 2, drain_in_flight(&lifecycle, &mut None))
        .await
        .expect("the drain gives up at DRAIN_TIMEOUT");
}

#[tokio::test]
async fn a_final_frame_without_its_newline_is_still_answered() {
    // A caller that writes its last request and closes without a trailing
    // newline gets it answered. Nothing distinguishes that from a line still
    // being written except end of input, so it is only delivered once the
    // client has stopped sending.
    let (client, mut server) = tokio::io::duplex(64);
    let mut reader = BufReader::new(client);
    let mut raw = Vec::new();

    server.write_all(b"{\"jsonrpc\":\"2.0\"}").await.unwrap();
    drop(server);

    let ReadLine::Line(line) = read_line(&mut reader, &mut raw).await.unwrap() else {
        panic!("an unterminated final frame is a request, not a broken one");
    };
    assert_eq!(line, "{\"jsonrpc\":\"2.0\"}");
    // And end of input follows, so the loop still terminates.
    assert!(matches!(
        read_line(&mut reader, &mut raw).await.unwrap(),
        ReadLine::Eof
    ));
}

#[tokio::test]
async fn a_line_at_exactly_the_limit_is_not_too_long() {
    // The limit is on the content, so the newline that terminates a
    // maximum-length line does not push it over.
    let (client, mut server) = tokio::io::duplex(MAX_LINE_BYTES + 64);
    let mut reader = BufReader::new(client);
    let mut raw = Vec::new();

    let body = "x".repeat(MAX_LINE_BYTES);
    tokio::spawn(async move {
        server.write_all(body.as_bytes()).await.unwrap();
        server.write_all(b"\n").await.unwrap();
    });

    let ReadLine::Line(line) = read_line(&mut reader, &mut raw).await.unwrap() else {
        panic!("a line of exactly MAX_LINE_BYTES fits");
    };
    assert_eq!(line.len(), MAX_LINE_BYTES);
}

#[tokio::test]
async fn a_read_resumed_mid_character_still_decodes() {
    // raw outlives one call on purpose: RMCP polls receive inside a select! and
    // drops the read whenever another arm wins, which can land between the
    // bytes of one character. The decode runs once on the reassembled buffer,
    // so the halves have to be carried as bytes rather than decoded apart and
    // rejected as malformed.
    let mut raw = vec![0xE4, 0xBD]; // the first two bytes of 你
    let rest = [0xA0, b'\n'];
    let mut reader = BufReader::new(&rest[..]);

    let ReadLine::Line(line) = read_line(&mut reader, &mut raw).await.unwrap() else {
        panic!("the halves of one character must reassemble");
    };
    assert_eq!(line, "你");
    assert!(raw.is_empty(), "a delivered line must not stay buffered");
}

#[tokio::test]
async fn a_cancelled_read_of_an_oversize_line_is_still_too_long() {
    // The resumed read has no budget left. That must report the line as
    // oversize, not mistake an empty read for end of input and hang up.
    let (client, mut server) = tokio::io::duplex(MAX_LINE_BYTES + 64);
    let mut reader = BufReader::new(client);
    let mut raw = vec![b'x'; MAX_LINE_BYTES + 1];

    server
        .write_all(b"tail-of-the-oversize-line\n{\"jsonrpc\":\"2.0\"}\n")
        .await
        .unwrap();
    assert!(matches!(
        read_line(&mut reader, &mut raw).await.unwrap(),
        ReadLine::TooLong
    ));

    let ReadLine::Line(next) = read_line(&mut reader, &mut raw).await.unwrap() else {
        panic!("the line after an oversize one still parses");
    };
    assert_eq!(next, "{\"jsonrpc\":\"2.0\"}");
}

#[tokio::test]
async fn a_cancelled_read_resumes_instead_of_losing_the_line() {
    // RMCP polls receive inside a select!, so a read in progress is dropped
    // whenever a response becomes ready. The bytes already taken have to
    // survive that, or the client's request is split in two and never answered.
    // This is the failure that hung CI: it needs an outgoing response to land
    // mid-read, so it is timing-dependent in the server and deterministic only
    // here.
    let (client, mut server) = tokio::io::duplex(64);
    let mut reader = BufReader::new(client);
    let mut raw = Vec::new();

    server.write_all(b"{\"jsonrpc\":").await.unwrap();
    // Drop the read future mid-line, exactly as the select! would.
    let cancelled =
        tokio::time::timeout(Duration::from_millis(20), read_line(&mut reader, &mut raw)).await;
    assert!(cancelled.is_err(), "the read must still be waiting");
    assert!(!raw.is_empty(), "the bytes taken so far are kept");

    server.write_all(b"\"2.0\"}\n").await.unwrap();
    let ReadLine::Line(line) = read_line(&mut reader, &mut raw).await.unwrap() else {
        panic!("the resumed read must produce the whole line");
    };
    assert_eq!(line, "{\"jsonrpc\":\"2.0\"}");
    assert!(raw.is_empty(), "a consumed line leaves the buffer empty");
}

#[tokio::test]
async fn malformed_utf8_is_reported_not_dropped() {
    let mut reader = BufReader::new(&b"\xff\xfe\n"[..]);
    let mut raw = Vec::new();
    assert!(matches!(
        read_line(&mut reader, &mut raw).await.unwrap(),
        ReadLine::MalformedUtf8
    ));
}

#[tokio::test]
async fn empty_input_is_eof() {
    let mut reader = BufReader::new(&b""[..]);
    let mut raw = Vec::new();
    assert!(matches!(
        read_line(&mut reader, &mut raw).await.unwrap(),
        ReadLine::Eof
    ));
}

#[tokio::test(start_paused = true)]
async fn draining_after_the_queue_is_closed_returns_rather_than_waiting() {
    // The shape of the bug this has been bitten by: a drain that waits on a
    // writer which can no longer answer. With the sender taken there is nothing
    // to enqueue against, so it has to return, not block.
    let lifecycle = Lifecycle::default();
    let (outbound, _rx) = mpsc::unbounded_channel();
    lifecycle.set_outbound(outbound);
    lifecycle.close_outbound();

    tokio::time::timeout(Duration::from_secs(1), lifecycle.drain_outbound())
        .await
        .expect("a closed queue has nothing to wait for");
}

#[tokio::test(start_paused = true)]
async fn draining_gives_up_rather_than_waiting_on_a_writer_that_cannot_write() {
    // Nothing consumes the queue here, which is what a client that has stopped
    // reading its end of the pipe looks like from in here.
    let lifecycle = Lifecycle::default();
    let (outbound, _rx) = mpsc::unbounded_channel();
    lifecycle.set_outbound(outbound);

    tokio::time::timeout(FLUSH_TIMEOUT * 2, lifecycle.drain_outbound())
        .await
        .expect("the drain is bounded, so termination does not depend on the client");
}
