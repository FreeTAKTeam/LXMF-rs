use super::*;
use rns_transport::iface::{encode_packet_ifac, InterfaceSharedConfig};
use rns_transport::packet::{Packet, PacketDataBuffer};
use std::time::Duration;

struct ConfiguredCarrier(u8);
impl Interface for ConfiguredCarrier {
    fn mtu() -> usize {
        96
    }
    fn ifac_default_size_bytes() -> usize {
        8
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn configured_root_publishes_typed_policy_before_constructor_and_retains_control_handle() {
    let mut manager = InterfaceManager::new(8);
    let iface = InterfaceConfig {
        kind: "serial".to_string(),
        mode: Some("ap".to_string()),
        outgoing: Some(false),
        gravity: Some(7),
        bitrate: Some(1200),
        announce_cap: Some(5),
        network_name: Some("".to_string()),
        networkname: Some("configured-network".to_string()),
        pass_phrase: Some("preferred-key".to_string()),
        passphrase: Some("ignored-key".to_string()),
        ..Default::default()
    };
    let (started, first_poll) = tokio::sync::oneshot::channel();
    let (address, control) = spawn_configured_with_handle(
        &mut manager,
        ConfiguredCarrier(42),
        |context| {
            assert_eq!(
                context
                    .channel
                    .ifac_state
                    .read()
                    .expect("prepared IFAC")
                    .as_ref()
                    .expect("authenticated constructor")
                    .ifac_size(),
                8
            );
            async move {
                let packet = Packet {
                    data: PacketDataBuffer::new_from_slice(b"configured first poll"),
                    ..Default::default()
                };
                let frame =
                    encode_packet_ifac(&context.channel.ifac_state, &packet).expect("first frame");
                started.send(frame).expect("native first-poll observer");
                context.cancel.cancelled().await;
            }
        },
        IfaceRole::Multicast,
        &iface,
    )
    .expect("configured root");
    assert_eq!(control.lock().expect("existing carrier control").0, 42);
    assert_eq!(manager.role(&address), Some(IfaceRole::Multicast));
    assert_eq!(manager.mode(&address), Some(InterfaceMode::AccessPoint));
    assert_eq!(manager.gravity(&address), Some(7));
    assert_eq!(manager.outgoing(&address), Some(false));
    assert_eq!(manager.announce_pacing(&address), Some((1200, 5)));
    assert_eq!(manager.mtu(&address), Some(96));
    assert_eq!(
        manager.shared_config(&address).expect("typed shared policy").passphrase.as_deref(),
        Some("preferred-key")
    );
    let frame = tokio::time::timeout(Duration::from_secs(2), first_poll)
        .await
        .expect("poll")
        .expect("frame");
    let expected = InterfaceSharedConfig {
        network_name: Some("configured-network".to_string()),
        passphrase: Some("preferred-key".to_string()),
        ..Default::default()
    }
    .ifac_context_with_default_size(8)
    .expect("expected carrier default")
    .expect("IFAC");
    assert_eq!(
        expected
            .decode(&frame)
            .expect("valid first wire frame")
            .expect("typed credentials authenticate"),
        Packet {
            data: PacketDataBuffer::new_from_slice(b"configured first poll"),
            ..Default::default()
        }
        .to_bytes()
        .expect("exact plain bytes")
    );
    let workers = manager.take_worker_tree(address);
    assert_eq!(workers.len(), 1);
    assert!(manager.stop_interface(address));
    for worker in workers {
        worker.await.expect("owned carrier joins");
    }
}

#[tokio::test]
async fn configured_root_failure_does_not_construct_worker_or_leave_registration() {
    let mut manager = InterfaceManager::new(8);
    let iface = InterfaceConfig { ifac_size: Some(128), ..Default::default() };
    let result = spawn_configured(
        &mut manager,
        ConfiguredCarrier(0),
        |_| -> std::future::Ready<()> { panic!("invalid policy cannot construct worker") },
        IfaceRole::Unicast,
        &iface,
    );
    assert!(matches!(result, Err(rns_transport::error::RnsError::InvalidArgument)));
    assert!(manager.interface_hashes().is_empty());
}

#[test]
fn failed_typed_policy_publication_keeps_all_old_routing_fields() {
    let mut manager = InterfaceManager::new(8);
    let channel = manager.new_channel(8);
    let address = *channel.address();
    let old = InterfaceConfig {
        networkname: Some("old".to_string()),
        gravity: Some(7),
        ..Default::default()
    };
    apply_runtime_config(&mut manager, address, &old).expect("old policy");
    let poisoned = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let _guard = channel.ifac_state.write().expect("healthy IFAC");
        panic!("poison typed policy publication");
    }));
    assert!(poisoned.is_err());
    let new = InterfaceConfig {
        networkname: Some("new".to_string()),
        gravity: Some(99),
        mode: Some("ap".to_string()),
        outgoing: Some(false),
        bitrate: Some(1200),
        announce_cap: Some(1),
        ..Default::default()
    };
    assert!(apply_runtime_config(&mut manager, address, &new).is_err());
    assert_eq!(manager.mode(&address), Some(InterfaceMode::Full));
    assert_eq!(manager.gravity(&address), Some(7));
    assert_eq!(manager.outgoing(&address), Some(true));
    assert_eq!(manager.announce_pacing(&address), Some((62_500, 2)));
    assert_eq!(
        manager.shared_config(&address).expect("old shared policy").network_name.as_deref(),
        Some("old")
    );
}

#[tokio::test]
async fn synthetic_shared_origin_does_not_disable_explicit_local_authentication() {
    for synthetic in [false, true] {
        let mut manager = InterfaceManager::new(8);
        let iface = InterfaceConfig {
            kind: "local".to_string(),
            synthetic_shared_instance: synthetic,
            networkname: Some("explicit-local-network".to_string()),
            ..Default::default()
        };
        let address = spawn_configured(
            &mut manager,
            ConfiguredCarrier(0),
            move |context| {
                assert_eq!(
                    context.channel.ifac_state.read().expect("prepared local IFAC").is_some(),
                    !synthetic
                );
                async move {
                    context.cancel.cancelled().await;
                }
            },
            IfaceRole::Unicast,
            &iface,
        )
        .expect("local configured root");
        assert!(manager.is_shared_instance(&address), "routing classification precedes activation");
        let workers = manager.take_worker_tree(address);
        manager.stop_interface(address);
        for worker in workers {
            worker.await.expect("local worker joins");
        }
    }
    let from_json: InterfaceConfig = serde_json::from_value(serde_json::json!({
        "type": "local", "synthetic_shared_instance": true,
    }))
    .expect("interface config");
    assert!(!from_json.synthetic_shared_instance, "caller cannot opt into the native exception");
}
