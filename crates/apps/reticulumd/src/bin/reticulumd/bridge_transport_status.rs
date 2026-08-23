use rns_transport::iface::InterfaceTrafficSnapshot;
use rns_transport::transport::Transport;
use serde_json::{json, Value as JsonValue};
use std::sync::Arc;

pub(crate) fn build_transport_status(
    transport: Arc<Transport>,
) -> Result<JsonValue, std::io::Error> {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|err| std::io::Error::other(format!("transport status runtime: {err}")))?;
    let (queues, interfaces, links, active_links, lowest_bitrate, medium_timeout) = runtime
        .block_on(async move {
            (
                transport.inbound_queue_snapshot().await,
                transport.interface_traffic_snapshots().await,
                transport.link_count().await,
                transport.active_link_count().await,
                transport.lowest_interface_bitrate().await,
                transport.medium_path_timeout().await.as_secs_f64(),
            )
        });
    // Backbone parent rows aggregate child connections. Sum only roots so global totals do not
    // double-count parent and child views of the same wire traffic.
    let sum_u64 = |field: fn(&InterfaceTrafficSnapshot) -> u64| {
        interfaces.iter().filter(|row| row.parent.is_none()).map(field).sum::<u64>()
    };
    let sum_f64 = |field: fn(&InterfaceTrafficSnapshot) -> f64| {
        interfaces.iter().filter(|row| row.parent.is_none()).map(field).sum::<f64>()
    };
    let interface_rows = interfaces
        .iter()
        .map(|row| {
            json!({
                "address": row.address.to_hex_string(),
                "parent": row.parent.map(|parent| parent.to_hex_string()),
                "rx_bytes": row.rx_bytes,
                "tx_bytes": row.tx_bytes,
                "rx_speed": row.rx_speed,
                "tx_speed": row.tx_speed,
                "announce": {
                    "rx_bytes": row.announce_rx_bytes,
                    "tx_bytes": row.announce_tx_bytes,
                    "rx_count": row.announce_rx_count,
                    "tx_count": row.announce_tx_count,
                    "rx_speed": row.announce_rx_speed,
                    "tx_speed": row.announce_tx_speed,
                    "rx_frequency": row.announce_rx_frequency,
                    "tx_frequency": row.announce_tx_frequency,
                },
                "path_request": {
                    "rx_bytes": row.path_request_rx_bytes,
                    "tx_bytes": row.path_request_tx_bytes,
                    "rx_count": row.path_request_rx_count,
                    "tx_count": row.path_request_tx_count,
                    "rx_speed": row.path_request_rx_speed,
                    "tx_speed": row.path_request_tx_speed,
                    "rx_frequency": row.path_request_rx_frequency,
                    "tx_frequency": row.path_request_tx_frequency,
                },
                "violations": {
                    "protocol": row.protocol_violations,
                    "ifac": row.ifac_violations,
                    "packet_filter_hits": row.packet_filter_hits,
                },
                "ingress_control": {
                    "announce_burst_active": row.announce_burst_active,
                    "path_request_burst_active": row.path_request_burst_active,
                    "ic_burst_count": row.ic_burst_count,
                    "ic_pr_burst_count": row.ic_pr_burst_count,
                },
            })
        })
        .collect::<Vec<_>>();
    let pressure = |height: usize, limit: usize| {
        if limit == 0 {
            0.0
        } else {
            height as f64 / limit as f64
        }
    };
    Ok(json!({
        "inbound_queues": {
            "total": queues.total,
            "total_limit": queues.limits.iter().sum::<usize>(),
            "limits": {
                "data": queues.limits[0],
                "announce": queues.limits[1],
                "path_request": queues.limits[2],
                "ingress_limited": queues.limits[3],
            },
            "heights": {
                "data": queues.heights[0],
                "announce": queues.heights[1],
                "path_request": queues.heights[2],
                "ingress_limited": queues.heights[3],
            },
            "dropped": {
                "data": queues.dropped[0],
                "announce": queues.dropped[1],
                "path_request": queues.dropped[2],
                "ingress_limited": queues.dropped[3],
            },
            "pressure": {
                "total": pressure(queues.total, queues.limits.iter().sum()),
                "data": pressure(queues.heights[0], queues.limits[0]),
                "announce": pressure(queues.heights[1], queues.limits[1]),
                "path_request": pressure(queues.heights[2], queues.limits[2]),
                "ingress_limited": pressure(queues.heights[3], queues.limits[3]),
            },
        },
        "traffic": {
            "rx_bytes": sum_u64(|row| row.rx_bytes),
            "tx_bytes": sum_u64(|row| row.tx_bytes),
            "rx_speed": sum_f64(|row| row.rx_speed),
            "tx_speed": sum_f64(|row| row.tx_speed),
            "announce_rx_bytes": sum_u64(|row| row.announce_rx_bytes),
            "announce_tx_bytes": sum_u64(|row| row.announce_tx_bytes),
            "announce_rx_count": sum_u64(|row| row.announce_rx_count),
            "announce_tx_count": sum_u64(|row| row.announce_tx_count),
            "announce_rx_speed": sum_f64(|row| row.announce_rx_speed),
            "announce_tx_speed": sum_f64(|row| row.announce_tx_speed),
            "announce_rx_frequency": sum_f64(|row| row.announce_rx_frequency),
            "announce_tx_frequency": sum_f64(|row| row.announce_tx_frequency),
            "path_request_rx_bytes": sum_u64(|row| row.path_request_rx_bytes),
            "path_request_tx_bytes": sum_u64(|row| row.path_request_tx_bytes),
            "path_request_rx_count": sum_u64(|row| row.path_request_rx_count),
            "path_request_tx_count": sum_u64(|row| row.path_request_tx_count),
            "path_request_rx_speed": sum_f64(|row| row.path_request_rx_speed),
            "path_request_tx_speed": sum_f64(|row| row.path_request_tx_speed),
            "path_request_rx_frequency": sum_f64(|row| row.path_request_rx_frequency),
            "path_request_tx_frequency": sum_f64(|row| row.path_request_tx_frequency),
        },
        "interfaces": interface_rows,
        "link_count": links,
        "active_link_count": active_links,
        "lowest_interface_bitrate": lowest_bitrate,
        "medium_path_timeout": medium_timeout,
    }))
}
