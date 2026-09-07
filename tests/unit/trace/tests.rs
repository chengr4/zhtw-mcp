use super::*;

/// One at a time: the sender these tests install is process-global, so
/// two of them running at once would each receive the other's events.
static SENDER: Mutex<()> = Mutex::new(());

/// Collect what the layer forwards for one closure's worth of events.
///
/// The subscriber is installed for this thread only, so events from tests
/// running beside this one do not reach the layer.
fn forwarded(emit: impl FnOnce()) -> Vec<McpLogMessage> {
    let _installed = SENDER.lock().unwrap_or_else(|e| e.into_inner());
    let (tx, rx) = mpsc::channel();
    set_mcp_log_sender(Some(tx));
    let subscriber = tracing_subscriber::registry().with(McpLogLayer);
    tracing::subscriber::with_default(subscriber, emit);
    set_mcp_log_sender(None);
    rx.try_iter().collect()
}

#[test]
fn an_event_arrives_as_message_target_and_typed_fields() {
    // The payload a client actually renders. Nothing else asserts its shape, so
    // a change to the visitor would otherwise only show up as a client
    // displaying nothing.
    let sent = forwarded(|| {
        tracing::info!(
            count = 3u64,
            signed = -1i64,
            ok = true,
            name = "zhtw",
            "scan done"
        );
    });

    assert_eq!(sent.len(), 1, "one event, one notification");
    let data = &sent[0].data;
    assert_eq!(data["message"], "scan done");
    assert_eq!(data["target"], "zhtw_mcp::trace::tests");

    // Numbers and booleans stay typed rather than being stringified, which is
    // the whole reason the visitor implements more than record_debug.
    assert_eq!(data["fields"]["count"], 3);
    assert_eq!(data["fields"]["signed"], -1);
    assert_eq!(data["fields"]["ok"], true);
    assert_eq!(data["fields"]["name"], "zhtw");
}

#[test]
#[allow(deprecated)]
fn levels_map_and_anything_below_info_is_dropped() {
    {
        let sent = forwarded(|| {
            tracing::error!("bad");
            tracing::warn!("iffy");
            tracing::info!("fine");
            tracing::debug!("noise");
            tracing::trace!("more noise");
        });
        let levels: Vec<_> = sent.iter().map(|m| m.level).collect();
        assert_eq!(
            levels,
            vec![
                rmcp::model::LoggingLevel::Error,
                rmcp::model::LoggingLevel::Warning,
                rmcp::model::LoggingLevel::Info,
            ],
            "debug and trace are not what a client asked for"
        );
    }
}

#[test]
fn another_crates_events_are_not_forwarded() {
    // The SDK's own tracing would otherwise arrive interleaved with the
    // notifications a request produced.
    let sent = forwarded(|| {
        tracing::info!(target: "rmcp::service", "sdk internals");
        // Shares our first eleven characters and is still not ours.
        tracing::info!(target: "zhtw_mcp_helper", "a different crate");
        tracing::info!(target: "zhtw_mcp", "the crate itself");
        tracing::info!(target: "zhtw_mcp::engine", "a module of ours");
    });
    let targets: Vec<_> = sent.iter().map(|m| m.data["target"].clone()).collect();
    assert_eq!(targets, vec!["zhtw_mcp", "zhtw_mcp::engine"]);
}

#[test]
fn a_message_less_event_falls_back_to_its_target() {
    let sent = forwarded(|| tracing::info!(rule = "ZY5"));
    assert_eq!(sent.len(), 1);
    assert_eq!(sent[0].data["message"], "zhtw_mcp::trace::tests");
    assert_eq!(sent[0].data["fields"]["rule"], "ZY5");
}
