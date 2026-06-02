use serde_json::json;

pub(crate) fn resolve_query_rpc_addr(
    remote: Option<&str>,
    rpc_addr: &str,
) -> Result<String, String> {
    match remote {
        Some(remote) if looks_like_rpc_addr(remote) => Ok(remote.to_string()),
        Some(_) => Ok(rpc_addr.to_string()),
        None => Ok(rpc_addr.to_string()),
    }
}

pub(crate) fn looks_like_rpc_addr(value: &str) -> bool {
    value.contains(':') || value.starts_with("http://") || value.starts_with("https://")
}

pub(crate) fn show_status_and_peers(
    rpc_addr: &str,
    show_status: bool,
    show_peers: bool,
) -> Result<serde_json::Value, String> {
    let status = unwrap_rpc_result(
        crate::rpc_client::rpc_call(rpc_addr, "daemon_status_ex", None)
            .or_else(|_| crate::rpc_client::rpc_call(rpc_addr, "propagation_status", None))?,
    );
    let peers = unwrap_rpc_result(
        crate::rpc_client::rpc_call(rpc_addr, "list_peers", None)
            .unwrap_or_else(|_| json!({ "peers": [] })),
    );

    if show_status {
        print_status_summary(&status, &peers);
    }
    if show_peers {
        print_peer_summary(&peers);
    }
    if !show_status && !show_peers {
        print_status_summary(&status, &peers);
    }

    Ok(json!({
        "status": status,
        "peers": peers,
    }))
}

pub(crate) fn show_remote_status_and_peers(
    rpc_addr: &str,
    remote: &str,
    identity_private_key_hex: Option<&str>,
    timeout_secs: f64,
    show_status: bool,
    show_peers: bool,
) -> Result<serde_json::Value, String> {
    let response = unwrap_rpc_result(crate::rpc_client::rpc_call(
        rpc_addr,
        "propagation_remote_status",
        Some(json!({
            "remote": remote,
            "identity_private_key_hex": identity_private_key_hex,
            "timeout_secs": timeout_secs,
        })),
    )?);
    let status = response.get("status").cloned().unwrap_or(response.clone());

    if show_status || !show_peers {
        print_remote_status_summary(&status);
    }
    if show_peers {
        print_remote_peer_summary(&status);
    }

    Ok(status)
}

fn unwrap_rpc_result(value: serde_json::Value) -> serde_json::Value {
    value.get("result").cloned().unwrap_or(value)
}

fn print_status_summary(status: &serde_json::Value, peers: &serde_json::Value) {
    let propagation = status.get("propagation").unwrap_or(status);
    let delivery_policy = status.get("delivery_policy");
    let stamp_policy = status.get("stamp_policy");
    let enabled = propagation.get("enabled").and_then(|value| value.as_bool()).unwrap_or(false);
    let target_cost = propagation.get("target_cost").and_then(|value| value.as_u64()).unwrap_or(0);
    let total_ingested =
        propagation.get("total_ingested").and_then(|value| value.as_u64()).unwrap_or(0);
    let selected_node =
        propagation.get("selected_node").and_then(|value| value.as_str()).unwrap_or("<none>");
    let peer_count =
        peers.get("peers").and_then(|value| value.as_array()).map(|items| items.len()).unwrap_or(0);
    let auth_required = delivery_policy
        .and_then(|policy| policy.get("auth_required"))
        .and_then(|value| value.as_bool())
        .unwrap_or(false);
    let ignored_count = delivery_policy
        .and_then(|policy| policy.get("ignored_destinations"))
        .and_then(|value| value.as_array())
        .map(|items| items.len())
        .unwrap_or(0);
    let prioritised_count = delivery_policy
        .and_then(|policy| policy.get("prioritised_destinations"))
        .and_then(|value| value.as_array())
        .map(|items| items.len())
        .unwrap_or(0);
    let flexibility = stamp_policy
        .and_then(|policy| policy.get("flexibility"))
        .and_then(|value| value.as_u64())
        .unwrap_or(0);

    println!();
    println!("LXMF Propagation Node {}", if enabled { "enabled" } else { "disabled" });
    println!("Peers: {peer_count}");
    println!("Target stamp cost: {target_cost}");
    println!("Stamp cost flexibility: {flexibility}");
    println!("Authentication required: {}", if auth_required { "yes" } else { "no" });
    println!("Ignored destinations: {ignored_count}");
    println!("Prioritised destinations: {prioritised_count}");
    println!("Total ingested messages: {total_ingested}");
    println!("Selected outbound propagation node: {selected_node}");
}

fn print_peer_summary(peers: &serde_json::Value) {
    println!();
    let items = peers.get("peers").and_then(|value| value.as_array()).cloned().unwrap_or_default();
    if items.is_empty() {
        println!("No peers discovered");
        return;
    }

    for peer in items {
        let id = peer.get("peer").and_then(|value| value.as_str()).unwrap_or("<unknown>");
        let name = peer.get("name").and_then(|value| value.as_str()).unwrap_or("");
        let last_seen = peer.get("last_seen").and_then(|value| value.as_i64()).unwrap_or(0);
        if name.is_empty() {
            println!("{id} last_seen={last_seen}");
        } else {
            println!("{id} name=\"{name}\" last_seen={last_seen}");
        }
    }
}

fn print_remote_status_summary(status: &serde_json::Value) {
    let total_peers = status.get("total_peers").and_then(|value| value.as_u64()).unwrap_or(0);
    let discovered_peers =
        status.get("discovered_peers").and_then(|value| value.as_u64()).unwrap_or(0);
    let target_cost = status.get("target_stamp_cost").and_then(|value| value.as_u64()).unwrap_or(0);
    let flexibility =
        status.get("stamp_cost_flexibility").and_then(|value| value.as_u64()).unwrap_or(0);
    let max_peers = status.get("max_peers").and_then(|value| value.as_u64()).unwrap_or(0);
    let destination =
        status.get("destination_hash").and_then(|value| value.as_str()).unwrap_or("<unknown>");

    println!();
    println!("Remote LXMF Propagation Node status");
    println!("Destination hash: {destination}");
    println!("Peers: {total_peers}");
    println!("Discovered peers: {discovered_peers}");
    println!("Max peers: {max_peers}");
    println!("Target stamp cost: {target_cost}");
    println!("Stamp cost flexibility: {flexibility}");
}

fn print_remote_peer_summary(status: &serde_json::Value) {
    println!();
    let Some(peers) = status.get("peers").and_then(|value| value.as_object()) else {
        println!("No peers discovered");
        return;
    };
    if peers.is_empty() {
        println!("No peers discovered");
        return;
    }

    let mut rows: Vec<_> = peers.iter().collect();
    rows.sort_by_key(|(peer, _)| *peer);
    for (peer, details) in rows {
        let name = details.get("name").and_then(|value| value.as_str()).unwrap_or("");
        let last_heard = details.get("last_heard").and_then(|value| value.as_i64()).unwrap_or(0);
        if name.is_empty() {
            println!("{peer} last_seen={last_heard}");
        } else {
            println!("{peer} name=\"{name}\" last_seen={last_heard}");
        }
    }
}
