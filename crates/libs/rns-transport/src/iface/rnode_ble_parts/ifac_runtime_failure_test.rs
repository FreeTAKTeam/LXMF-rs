#[tokio::test]
async fn rnode_ble_runtime_error_is_reported_and_restart_remains_ifac_fail_closed() {
    let shared = InterfaceSharedConfig {
        network_name: Some("rnode-ble-runtime-error-ifac".into()),
        passphrase: Some("runtime-error-key".into()),
        ..InterfaceSharedConfig::default()
    };
    let interface = NativeRnodeBleKissInterface::new(
        "software-rnode-ble-runtime-error",
        NativeRnodeBleSettings::for_peripheral("fake-peripheral"),
        RnodeBleKissConfig { max_write_len: 508, ..RnodeBleKissConfig::default() },
    )
    .with_rnode_validation(
        crate::iface::rnode_ble::LoraConfig::us915_default(),
        std::time::Duration::from_secs(1),
    )
    .with_reconnect_backoff(std::time::Duration::from_millis(1));
    let status = interface.runtime_status_handle().expect("RNode runtime status is enabled");
    let mut manager = InterfaceManager::new(8);
    let context = manager.new_context(interface);
    let address = *context.channel.address();
    let ifac_state = context.channel.ifac_state.clone();
    let violations = context.channel.ifac_violations.clone();
    assert!(manager.set_shared_config(address, shared));

    let plaintext = Packet {
        destination: AddressHash::new_from_slice(&[0x96; 16]),
        data: PacketDataBuffer::new_from_slice(b"plaintext after runtime failure"),
        ..Packet::default()
    };
    let authenticated = Packet {
        destination: AddressHash::new_from_slice(&[0x97; 16]),
        data: PacketDataBuffer::new_from_slice(b"authenticated after runtime failure"),
        ..Packet::default()
    };
    let state = Arc::new(StartupRetryState::default());
    {
        let mut incoming = state.io.incoming.lock().await;
        incoming.push_back(encode_data_frame(&plaintext.to_bytes().expect("serialize plaintext")));
        incoming.push_back(worker_wire_packet(&ifac_state, &authenticated));
    }
    let receiver = manager.receiver();
    let cancel = context.cancel.clone();
    let worker_state = state.clone();
    let task = tokio::spawn(NativeRnodeBleKissInterface::spawn_with_backend_factory(
        context,
        move |_| {
            let attempt = worker_state.attempts.fetch_add(1, Ordering::SeqCst);
            StartupRetryBackend {
                state: worker_state.clone(),
                fail_connect: false,
                fail_notification: attempt == 0,
            }
        },
    ));

    let ingress = timeout(std::time::Duration::from_secs(1), async {
        loop {
            let mut rx = receiver.lock().await;
            if let Ok(message) = rx.try_recv() {
                break message;
            }
            drop(rx);
            sleep(std::time::Duration::from_millis(1)).await;
        }
    })
    .await
    .expect("authenticated ingress is admitted after runtime reconnect");
    assert_eq!(ingress.packet.destination, authenticated.destination);
    assert_eq!(violations.load(Ordering::Relaxed), 1);
    assert!(receiver.lock().await.try_recv().is_err(), "plaintext never reaches routing");
    timeout(std::time::Duration::from_secs(1), async {
        while status.to_json()["worker_error"].is_null() {
            sleep(std::time::Duration::from_millis(1)).await;
        }
    })
    .await
    .expect("runtime read failure is published in status");
    assert_eq!(
        status.to_json()["worker_error"],
        "packet read failed: Backend { operation: \"next_notification\", message: \"injected BLE notification failure\" }"
    );

    cancel.cancel();
    timeout(std::time::Duration::from_secs(1), task)
        .await
        .expect("worker stops after cancellation")
        .expect("worker task joins");
    let attempts_at_stop = state.attempts.load(Ordering::SeqCst);
    sleep(std::time::Duration::from_millis(5)).await;
    assert_eq!(state.attempts.load(Ordering::SeqCst), attempts_at_stop, "stop prevents restart");
    assert!(state.cleanups.load(Ordering::SeqCst) >= 2, "both sessions are cleaned");
}
