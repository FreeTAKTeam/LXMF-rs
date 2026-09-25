#[tokio::test]
#[ignore = "requires pinned Python Reticulum reference at RETICULUM_PY_REPO"]
async fn paced_announce_queue_matches_pinned_python_capacity() {
    const PINNED_RETICULUM: &str = "99de23c040d507e3fefca19e87b182302902725d";
    let python_repo = std::env::var("RETICULUM_PY_REPO")
        .expect("set RETICULUM_PY_REPO to the pinned Python Reticulum checkout");
    let python = std::env::var("LXMF_PYTHON_BIN").unwrap_or_else(|_| "python3".to_string());
    let script = include_str!("../../tests/support/pinned_announce_queue_limit.py");
    let output = std::process::Command::new(python)
        .args(["-c", script, &python_repo, PINNED_RETICULUM])
        .output()
        .expect("read per-interface announce capacity from pinned Python source");
    assert!(
        output.status.success(),
        "pinned Python capacity lookup failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let python_capacity: usize = String::from_utf8_lossy(&output.stdout)
        .trim()
        .parse()
        .expect("Python announce capacity is an integer");
    assert_eq!(python_capacity, 4_096);

    let mut manager = InterfaceManager::new(16);
    let mut tx_rx = manager.new_channel(8_192).tx_channel;
    let mut previous = None;
    for index in 0..=python_capacity + 1 {
        let seed = format!("issue-609-announce-{index}");
        let message = TxMessage {
            tx_type: TxMessageType::Broadcast(None),
            packet: announce_packet_with(1, seed.as_bytes(), index as u64),
        };
        let trace = manager.send(message).await;
        if index == 0 {
            assert_eq!(trace.sent_ifaces, 1, "first announce should use the interface");
            assert_eq!(trace.queued_ifaces, 0);
        } else if index <= python_capacity {
            assert_eq!(trace.queued_ifaces, 1, "announce {index} should fit the paced queue");
        } else {
            assert_eq!(trace.queued_ifaces, 0);
            assert_eq!(trace.failed_ifaces, 1, "announce beyond Python's capacity must be dropped");
        }
        previous = Some(index);
    }
    assert_eq!(previous, Some(python_capacity + 1));
    assert!(tx_rx.try_recv().is_ok(), "first announce should have been transmitted");
}
