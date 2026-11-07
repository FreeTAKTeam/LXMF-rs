use super::*;
use crate::interface_hot_apply::tcp_server_bind_addr;

#[test]
fn preferred_localhost_retains_native_resolver_choice_in_public_listener_bind() {
    let mut record = tcp_server_record("listener", "localhost", 4242);
    assert_eq!(tcp_server_bind_addr(&record).expect("default bind"), "127.0.0.1:4242");
    record.settings = Some(json!({"prefer_ipv6":true}));
    assert_eq!(
        tcp_server_bind_addr(&record).expect("preferred bind"),
        "localhost:4242",
        "a preference must reach the native resolver instead of forcing either address family"
    );
}

#[test]
fn preferred_localhost_rejects_native_loopback_alias_bind_collisions_in_both_orders() {
    reject_collisions(&["127.0.0.1", "::1", "[::1]", "0:0:0:0:0:0:0:1"]);
}

#[test]
fn preferred_localhost_rejects_native_wildcard_alias_bind_collisions_in_both_orders() {
    reject_collisions(&["0.0.0.0", "::", "[::]", "0:0:0:0:0:0:0:0"]);
}

fn reject_collisions(hosts: &[&str]) {
    for host in hosts {
        for reversed in [false, true] {
            let (tx, _rx) = tokio::sync::mpsc::channel(1);
            let bridge = test_bridge(tx);
            let mut preferred = tcp_server_record("preferred", "localhost", 4242);
            preferred.settings = Some(json!({"prefer_ipv6":true}));
            let mut records = vec![preferred, tcp_server_record("literal", host, 4242)];
            if reversed {
                records.reverse();
            }
            let error = bridge
                .apply_interfaces(records)
                .expect_err("overlapping native bind candidates reject before queueing");
            assert_eq!(error.kind(), io::ErrorKind::InvalidInput);
            assert!(error.to_string().contains("duplicate tcp_server bind address"));
        }
    }
}
