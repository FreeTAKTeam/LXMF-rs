#[derive(Default)]
struct BlackholePathLookupBridge {
    removed_identities: std::sync::Mutex<Vec<String>>,
}

impl PathLookupBridge for BlackholePathLookupBridge {
    fn has_path(&self, _destination: &str) -> Result<bool, std::io::Error> {
        Ok(false)
    }

    fn request_path(&self, _destination: &str) -> Result<(), std::io::Error> {
        Ok(())
    }

    fn remove_paths_for_identity(&self, identity: &str) -> Result<usize, std::io::Error> {
        self.removed_identities
            .lock()
            .expect("removed identities mutex poisoned")
            .push(identity.to_string());
        Ok(2)
    }
}

#[test]
fn blackhole_identity_rpc_matches_reticulum_boolean_semantics() {
    let daemon = RpcDaemon::test_instance();
    let identity = "00112233445566778899AABBCCDDEEFF";
    let normalized = "00112233445566778899aabbccddeeff";

    let empty = daemon
        .handle_rpc(RpcRequest {
            id: 1,
            method: "get_blackholed_identities".to_string(),
            params: None,
        })
        .expect("get empty blackholed identities");
    assert_eq!(empty.result.expect("empty result"), json!({}));

    let added = daemon
        .handle_rpc(rpc_request(
            2,
            "blackhole_identity",
            json!({
                "identity_hash": identity,
                "until": 1_700_000_100_i64,
                "reason": "operator"
            }),
        ))
        .expect("blackhole identity");
    assert_eq!(added.result.expect("added result"), json!(true));

    let duplicate = daemon
        .handle_rpc(rpc_request(3, "blackhole_identity", json!({ "identity": normalized })))
        .expect("duplicate blackhole identity");
    assert_eq!(duplicate.result.expect("duplicate result"), JsonValue::Null);

    let listed = daemon
        .handle_rpc(RpcRequest {
            id: 4,
            method: "get_blackholed_identities".to_string(),
            params: None,
        })
        .expect("get blackholed identities");
    let listed = listed.result.expect("listed result");
    assert_eq!(listed[normalized]["source"], json!("test-identity"));
    assert_eq!(listed[normalized]["until"], json!(1_700_000_100_i64));
    assert_eq!(listed[normalized]["reason"], json!("operator"));

    let removed = daemon
        .handle_rpc(rpc_request(5, "unblackhole_identity", json!({ "hash": normalized })))
        .expect("unblackhole identity");
    assert_eq!(removed.result.expect("removed result"), json!(true));

    let duplicate_remove = daemon
        .handle_rpc(rpc_request(
            6,
            "unblackhole_identity",
            json!({ "identity_hash": normalized }),
        ))
        .expect("duplicate unblackhole identity");
    assert_eq!(duplicate_remove.result.expect("duplicate remove result"), JsonValue::Null);
}

#[test]
fn is_blackholed_matches_reticulum_identity_hash_validation() {
    let daemon = RpcDaemon::test_instance();
    let identity = "00112233445566778899aabbccddeeff";
    assert!(!daemon.is_blackholed(identity).expect("initial blackhole check"));

    daemon
        .handle_rpc(rpc_request(12, "blackhole_identity", json!({ "identity": identity })))
        .expect("blackhole identity");
    assert!(daemon.is_blackholed(identity).expect("blackhole check"));
    assert!(daemon.is_blackholed(&identity.to_ascii_uppercase()).expect("case-insensitive check"));
    assert!(daemon.is_blackholed("abcd").is_err());
}

#[test]
fn blackhole_identity_rpc_requests_associated_path_eviction_once() {
    let daemon = RpcDaemon::test_instance();
    let bridge = Arc::new(BlackholePathLookupBridge::default());
    daemon.set_path_lookup_bridge(bridge.clone());
    let identity = "00112233445566778899AABBCCDDEEFF";
    let normalized = identity.to_ascii_lowercase();

    let added = daemon
        .handle_rpc(rpc_request(7, "blackhole_identity", json!({ "identity": identity })))
        .expect("blackhole identity");
    assert_eq!(added.result.expect("added result"), json!(true));

    let duplicate = daemon
        .handle_rpc(rpc_request(8, "blackhole_identity", json!({ "identity": identity })))
        .expect("duplicate blackhole identity");
    assert_eq!(duplicate.result.expect("duplicate result"), JsonValue::Null);
    assert_eq!(
        *bridge.removed_identities.lock().expect("removed identities mutex poisoned"),
        vec![normalized]
    );
}

#[test]
fn blackhole_identity_rpc_returns_false_for_malformed_identity_hashes() {
    let daemon = RpcDaemon::test_instance();

    let short = daemon
        .handle_rpc(rpc_request(10, "blackhole_identity", json!({ "identity_hash": "abcd" })))
        .expect("short identity hash returns false");
    assert_eq!(short.result.expect("short result"), json!(false));

    let non_hex = daemon
        .handle_rpc(rpc_request(
            11,
            "unblackhole_identity",
            json!({ "identity_hash": "not-hex" }),
        ))
        .expect("non-hex identity hash returns false");
    assert_eq!(non_hex.result.expect("non-hex result"), json!(false));

    let listed = daemon
        .handle_rpc(RpcRequest {
            id: 12,
            method: "get_blackholed_identities".to_string(),
            params: None,
        })
        .expect("get blackholed identities");
    assert_eq!(listed.result.expect("listed result"), json!({}));
}

#[test]
fn blackholed_identities_survive_restart_and_removal_is_persisted() {
    use std::time::{SystemTime, UNIX_EPOCH};

    let run_id = SystemTime::now().duration_since(UNIX_EPOCH).expect("unix epoch").as_nanos();
    let db_path = std::env::temp_dir()
        .join(format!("lxmf-rs-blackholes-{run_id}-{}.sqlite", std::process::id()));
    let identity = "00112233445566778899aabbccddeeff";

    {
        let store = MessagesStore::open(db_path.as_path()).expect("open sqlite store");
        let daemon = RpcDaemon::with_store(store, "persist-node".to_string());
        let added = daemon
            .handle_rpc(rpc_request(
                20,
                "blackhole_identity",
                json!({
                    "identity_hash": identity,
                    "until": 4_102_444_800_i64,
                    "reason": "operator"
                }),
            ))
            .expect("blackhole identity");
        assert_eq!(added.result.expect("added result"), json!(true));
    }

    {
        let store = MessagesStore::open(db_path.as_path()).expect("reopen sqlite store");
        let daemon = RpcDaemon::with_store(store, "persist-node".to_string());
        let listed = daemon
            .handle_rpc(RpcRequest {
                id: 21,
                method: "get_blackholed_identities".to_string(),
                params: None,
            })
            .expect("get persisted blackholed identities");
        let listed = listed.result.expect("listed result");
        assert_eq!(listed[identity]["source"], json!("persist-node"));
        assert_eq!(listed[identity]["until"], json!(4_102_444_800_i64));
        assert_eq!(listed[identity]["reason"], json!("operator"));

        let removed = daemon
            .handle_rpc(rpc_request(
                22,
                "unblackhole_identity",
                json!({ "identity_hash": identity }),
            ))
            .expect("unblackhole identity");
        assert_eq!(removed.result.expect("removed result"), json!(true));
    }

    {
        let store = MessagesStore::open(db_path.as_path()).expect("reopen sqlite store again");
        let daemon = RpcDaemon::with_store(store, "persist-node".to_string());
        let listed = daemon
            .handle_rpc(RpcRequest {
                id: 23,
                method: "get_blackholed_identities".to_string(),
                params: None,
            })
            .expect("get blackholed identities after persisted removal");
        assert_eq!(listed.result.expect("listed result"), json!({}));
    }

    let _ = std::fs::remove_file(&db_path);
}
