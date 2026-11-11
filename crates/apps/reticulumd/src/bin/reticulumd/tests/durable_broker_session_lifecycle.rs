use super::*;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn durable_broker_replays_issued_custody_after_zmq_sdk_restart() {
    durable_broker_zmq_restart(false).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "waits for the production five-minute idle timeout"]
async fn durable_broker_expires_old_zmq_sdk_session_and_replays_custody() {
    durable_broker_zmq_restart(true).await;
}

async fn durable_broker_zmq_restart(wait_for_expiry: bool) {
    use base64::Engine as _;
    use lxmf_sdk::{
        Client, IdentityImportRequest, LxmfSdkIdentity, SdkConfig, StartRequest,
        ZmqPipelineBackendClient, ZmqPipelineBackendConfig, ZmqRecoveryDecision,
    };
    use rns_rpc::broker::{
        AckStoredRequest, ConsumerId, EventPosition, FetchRequest, ResumeRequest,
    };
    use std::time::Duration;

    let temp = TempDir::new().expect("durable broker test directory");
    let mut args = test_args(temp.path().join("reticulum.db"), None, None, false);
    args.rpc = None;
    args.rpc_unix = None;
    args.zmq_durable_broker = true;
    let context = bootstrap::bootstrap(args).await;
    let daemon = context.daemon.clone();

    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("reserve loopback port");
    let endpoint = format!("tcp://{}", listener.local_addr().expect("loopback endpoint"));
    drop(listener);
    let (stop, shutdown) = tokio::sync::watch::channel(false);
    let router = tokio::spawn(crate::zmq_rpc_loop::run_zmq_router_loop_until(
        endpoint.clone(),
        true,
        daemon.clone(),
        shutdown,
    ));
    tokio::time::sleep(Duration::from_millis(50)).await;

    tokio::task::spawn_blocking(move || {
        let private_key = PrivateIdentity::new_from_rand(rand_core::OsRng);
        let open_client = || {
            let client = Client::new(
                ZmqPipelineBackendClient::new(ZmqPipelineBackendConfig::local(endpoint.clone()))
                    .expect("real ZeroMQ SDK client"),
            );
            let handle = client
                .start_durable_zmq(StartRequest::new(SdkConfig::desktop_local_default()))
                .expect("durable broker negotiation");
            let bundle = client
                .identity_import(IdentityImportRequest {
                    bundle_base64: base64::engine::general_purpose::STANDARD
                        .encode(private_key.to_private_key_bytes()),
                    passphrase: None,
                    display_name: None,
                    capabilities: Vec::new(),
                    metadata: Default::default(),
                    extensions: Default::default(),
                })
                .expect("import the same service identity for each session");
            client.identity_activate(bundle.identity.clone()).expect("activate service identity");
            (client, handle.runtime_id, bundle)
        };
        let (first, runtime_id, identity) = open_client();
        let resume = ResumeRequest {
            identity: identity.identity.0.clone(),
            consumer_id: ConsumerId("restart-consumer".into()),
            journal_id: None,
            stored: EventPosition(0),
        };
        let fetch = FetchRequest {
            identity: resume.identity.clone(),
            consumer_id: resume.consumer_id.clone(),
            max_events: 32,
            max_bytes: 16_384,
        };
        let checkpoint = first.backend().broker_resume(resume.clone()).expect("initial checkpoint");
        daemon
            .accept_inbound(rns_rpc::MessageRecord {
                id: "unique-restart-inbound".into(),
                source: "11".repeat(16),
                destination: identity
                    .delivery_destination
                    .expect("registered delivery destination"),
                title: String::new(),
                content: "custody before client restart".into(),
                timestamp: 1,
                direction: "in".into(),
                fields: None,
                receipt_status: None,
            })
            .expect("daemon commits inbound custody");
        let issued = first.backend().broker_fetch(fetch.clone()).expect("issued durable range");
        assert_eq!(issued.events.len(), 1);
        assert_eq!(issued.events[0].payload["message"]["id"], "unique-restart-inbound");

        if wait_for_expiry {
            std::thread::sleep(Duration::from_secs(301));
            let expired = first.identity_list().expect_err("ordinary SDK call reports expiry");
            assert_eq!(expired.machine_code, "SDK_BROKER_NEGOTIATION_REQUIRED");
            let expired =
                first.backend().broker_fetch(fetch.clone()).expect_err("old route expired");
            assert_eq!(expired.machine_code, "SDK_BROKER_NEGOTIATION_REQUIRED");
            assert_eq!(
                ZmqRecoveryDecision::for_error(&expired, true),
                ZmqRecoveryDecision::RestoreSession
            );
        }
        drop(first);

        let (replacement, same_runtime, restored_identity) = open_client();
        assert_eq!(same_runtime, runtime_id);
        assert_eq!(restored_identity.identity.0, resume.identity);
        let resume = ResumeRequest { journal_id: Some(checkpoint.journal_id), ..resume };
        assert_eq!(
            replacement
                .backend()
                .broker_resume(resume.clone())
                .expect("resume old consumer")
                .stored,
            resume.stored
        );
        let replayed =
            replacement.backend().broker_fetch(fetch.clone()).expect("replay issued range");
        assert_eq!(replayed, issued);
        assert_eq!(
            replacement
                .backend()
                .broker_ack_stored(AckStoredRequest {
                    consumer_id: resume.consumer_id.clone(),
                    identity: resume.identity.clone(),
                    journal_id: replayed.journal_id,
                    end: replayed.end,
                    receipt: replayed.receipt,
                })
                .expect("acknowledge replayed custody"),
            replayed.end
        );
        assert!(replacement.backend().broker_fetch(fetch).expect("next range").events.is_empty());
        let counts = daemon.resource_usage_snapshot().expect("resource diagnostics");
        assert_eq!(
            counts["durable_broker_sessions"]["current"],
            if wait_for_expiry { 1 } else { 2 }
        );
        assert_eq!(counts["durable_broker_sessions"]["expired_total"], u64::from(wait_for_expiry));
    })
    .await
    .expect("real ZeroMQ custody test");
    stop.send_replace(true);
    router.await.expect("ZeroMQ router task").expect("clean ZeroMQ shutdown");
    context.daemon.shutdown_outbound_workers();
}
