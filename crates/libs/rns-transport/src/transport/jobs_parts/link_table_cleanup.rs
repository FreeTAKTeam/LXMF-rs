#[allow(dead_code)]
pub(super) async fn handle_link_table_cleanup<'a>(mut handler: MutexGuard<'a, TransportHandler>) {
    let expired_links = handler.link_table.remove_stale();
    let now = std::time::Instant::now();
    let mut path_requests = Vec::new();

    for expired in expired_links {
        let destination = expired.original_destination;
        let path_hops = handler.path_table.get(&destination).map(|entry| entry.hops);
        let path_request_throttled = handler.path_requests.outgoing_request_recently_sent(
            &destination,
            now.into(),
            PATH_REQUEST_MI,
        );
        let mut blocked_if = None;
        let mut should_request = false;
        let mut should_mark_unresponsive = false;

        if path_hops.is_none() || (!path_request_throttled && expired.taken_hops == 0) {
            should_request = true;
        } else if !path_request_throttled && (path_hops == Some(1) || expired.taken_hops == 1) {
            should_request = true;
            blocked_if = Some(expired.received_from);
            should_mark_unresponsive = true;
        }

        if should_mark_unresponsive {
            if handler.config.transport_enabled {
                let ingress_mode = handler.iface_manager.lock().await.mode(&expired.received_from);
                if ingress_mode != Some(crate::iface::InterfaceMode::Boundary) {
                    handler.path_table.mark_path_unresponsive(&destination);
                }
            } else {
                handler.path_table.expire_path(&destination);
            }
        }

        if should_request {
            path_requests.push((destination, blocked_if));
        }
    }

    path_requests.sort_by_key(|(destination, blocked_if)| (*destination, blocked_if.is_none()));
    path_requests.dedup_by_key(|(destination, _)| *destination);
    for (destination, blocked_if) in path_requests {
        handler.request_path(&destination, blocked_if, None).await;
    }
}
