use reticulum::rpc::RpcDaemon;

#[test]
fn announce_scheduler_emits_events() {
    let daemon = RpcDaemon::test_instance();
    daemon.schedule_announce_for_test(1);
    let event = daemon.take_event().expect("announce event");
    assert_eq!(event.event_type, "announce_sent");
}
