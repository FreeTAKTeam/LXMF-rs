#[tokio::test(flavor = "multi_thread", worker_threads = 1)]
async fn durable_broker_bootstrap_provides_sdk_identity_without_network_interfaces() {
    use base64::Engine as _;

    let temp = TempDir::new().expect("temp dir");
    let mut args = test_args(temp.path().join("reticulum.db"), None, None, false);
    args.rpc = None;
    args.rpc_unix = None;
    args.zmq_durable_broker = true;

    let context = bootstrap::bootstrap(args).await;
    let service_identity = PrivateIdentity::new_from_rand(rand_core::OsRng);
    let response = context
        .daemon
        .handle_rpc(RpcRequest {
            id: 1,
            method: "sdk_identity_import_v2".to_string(),
            params: Some(json!({
                "bundle_base64": base64::engine::general_purpose::STANDARD
                    .encode(service_identity.to_private_key_bytes()),
                "display_name": "Broker-only service"
            })),
        })
        .expect("SDK identity import RPC");

    assert!(response.error.is_none(), "{:?}", response.error);
    assert!(response.result.expect("identity import result")["identity"]["delivery_destination"]
        .as_str()
        .is_some_and(|destination| !destination.is_empty()));

    let interfaces = context
        .daemon
        .handle_rpc(RpcRequest { id: 2, method: "list_interfaces".to_string(), params: None })
        .expect("list interfaces RPC")
        .result
        .expect("list interfaces result")["interfaces"]
        .as_array()
        .expect("interfaces array")
        .clone();
    assert!(interfaces.is_empty(), "broker-only startup must not create network interfaces");
}
