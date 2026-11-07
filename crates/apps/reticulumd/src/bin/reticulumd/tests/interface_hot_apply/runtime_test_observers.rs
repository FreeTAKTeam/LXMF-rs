async fn wait_for_hot_apply_udp_refresh(
    bridge: &InterfaceHotApplyBridge,
) -> (rns_transport::hash::AddressHash, HotApplyUdpRefresh) {
    for _ in 0..20 {
        if let Some(refresh) = bridge
            .udp_refreshes
            .lock()
            .expect("udp refresh mutex poisoned")
            .values()
            .next()
            .cloned()
        {
            return (refresh.runtime_iface, refresh);
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    panic!("hot-applied udp refresh was not registered");
}

async fn wait_for_hot_apply_pipe_refresh(
    bridge: &InterfaceHotApplyBridge,
) -> (rns_transport::hash::AddressHash, HotApplyPipeRefresh) {
    for _ in 0..20 {
        if let Some(refresh) = bridge
            .pipe_refreshes
            .lock()
            .expect("pipe refresh mutex poisoned")
            .values()
            .next()
            .cloned()
        {
            return (refresh.runtime_iface, refresh);
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    panic!("hot-applied pipe refresh was not registered");
}

async fn wait_for_hot_apply_tcp_listener_refresh(
    bridge: &InterfaceHotApplyBridge,
) -> (rns_transport::hash::AddressHash, super::HotApplyTcpListenerRefresh) {
    for _ in 0..20 {
        if let Some(refresh) = bridge
            .tcp_listener_refreshes
            .lock()
            .expect("tcp listener refresh mutex poisoned")
            .values()
            .next()
            .cloned()
        {
            return (refresh.runtime_iface, refresh);
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    panic!("hot-applied tcp listener refresh was not registered");
}
