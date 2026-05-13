#[path = "support/lxmd_three_node.rs"]
mod lxmd_three_node;

use lxmd_three_node::*;
use serde_json::json;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

const REMOTE_PATH_RESPONSE_MIN: Duration = Duration::from_millis(900);

#[test]
fn lxmd_four_node_tcp_remote_path_response_uses_relayed_route() {
    let lxmd_bin = resolve_test_binary("lxmd", option_env!("CARGO_BIN_EXE_lxmd"));
    let reticulumd_bin = resolve_test_binary("reticulumd", option_env!("CARGO_BIN_EXE_reticulumd"));

    let temp = tempfile::tempdir().expect("tempdir");
    let upstream_server_port = ReservedPort::reserve();
    let upstream_server_rpc = ReservedPort::reserve();
    let relay_rpc = ReservedPort::reserve();
    let relay_transport = ReservedPort::reserve();
    let client_two_rpc = ReservedPort::reserve();
    let client_two_transport = ReservedPort::reserve();
    let client_three_rpc = ReservedPort::reserve();
    let client_three_transport = ReservedPort::reserve();

    let upstream_server_dir = temp.path().join("upstream-server");
    let relay_dir = temp.path().join("relay");
    let client_two_dir = temp.path().join("client-two");
    let client_three_dir = temp.path().join("client-three");

    write_config(
        &upstream_server_dir,
        &node_config(
            "upstream-server",
            upstream_server_rpc.port(),
            Some(upstream_server_port.port()),
            &[],
        ),
    );
    write_config(
        &relay_dir,
        &node_config(
            "relay",
            relay_rpc.port(),
            Some(relay_transport.port()),
            &[tcp_client_interface("relay-uplink", upstream_server_port.port())],
        ),
    );
    write_config(
        &client_two_dir,
        &node_config(
            "client-two",
            client_two_rpc.port(),
            Some(client_two_transport.port()),
            &[tcp_client_interface("client-two-uplink", relay_transport.port())],
        ),
    );
    write_config(
        &client_three_dir,
        &node_config(
            "client-three",
            client_three_rpc.port(),
            Some(client_three_transport.port()),
            &[tcp_client_interface("client-three-uplink", upstream_server_port.port())],
        ),
    );

    let mut upstream_server = Some(spawn_lxmd(
        &lxmd_bin,
        &reticulumd_bin,
        upstream_server_rpc.port(),
        &upstream_server_dir,
        &mut [upstream_server_port, upstream_server_rpc],
    ));
    let mut relay = None;
    let mut client_two = None;
    let mut client_three = None;

    let outcome: Result<(), String> = (|| {
        wait_for_ready(
            upstream_server.as_ref().expect("upstream server child").rpc_port(),
            upstream_server.as_mut().expect("upstream server child"),
            "upstream-server",
        )?;

        relay = Some(spawn_lxmd(
            &lxmd_bin,
            &reticulumd_bin,
            relay_rpc.port(),
            &relay_dir,
            &mut [relay_rpc, relay_transport],
        ));
        wait_for_ready(
            relay.as_ref().expect("relay child").rpc_port(),
            relay.as_mut().expect("relay child"),
            "relay",
        )?;

        client_two = Some(spawn_lxmd(
            &lxmd_bin,
            &reticulumd_bin,
            client_two_rpc.port(),
            &client_two_dir,
            &mut [client_two_rpc, client_two_transport],
        ));
        wait_for_ready(
            client_two.as_ref().expect("client-two child").rpc_port(),
            client_two.as_mut().expect("client-two child"),
            "client-two",
        )?;

        client_three = Some(spawn_lxmd(
            &lxmd_bin,
            &reticulumd_bin,
            client_three_rpc.port(),
            &client_three_dir,
            &mut [client_three_rpc, client_three_transport],
        ));
        wait_for_ready(
            client_three.as_ref().expect("client-three child").rpc_port(),
            client_three.as_mut().expect("client-three child"),
            "client-three",
        )?;

        let client_two_rpc = client_two.as_ref().expect("client-two child").rpc_port();
        let client_three_rpc = client_three.as_ref().expect("client-three child").rpc_port();

        let client_two_status = daemon_status(client_two_rpc)?;
        let client_three_status = daemon_status(client_three_rpc)?;
        let client_two_hash = status_hash(&client_two_status)
            .unwrap_or_else(|| panic!("client-two delivery hash: {client_two_status}"));
        let client_three_hash = status_hash(&client_three_status)
            .unwrap_or_else(|| panic!("client-three delivery hash: {client_three_status}"));

        rpc_call(client_two_rpc, "announce_now", None)?;

        let message_id = format!(
            "remote-path-{}",
            SystemTime::now().duration_since(UNIX_EPOCH).expect("time").as_millis()
        );
        let delivery_started_at = Instant::now();
        rpc_call(
            client_three_rpc,
            "send_message_v2",
            Some(json!({
                "id": message_id,
                "source": client_three_hash,
                "destination": client_two_hash,
                "title": "",
                "content": "hello world",
                "method": "direct"
            })),
        )?;

        wait_for_inbound_message(client_two_rpc, "hello world")?;

        let delivery_elapsed = delivery_started_at.elapsed();
        if delivery_elapsed < REMOTE_PATH_RESPONSE_MIN {
            return Err(format!(
                "remote path response completed too quickly: {:?} < {:?}",
                delivery_elapsed, REMOTE_PATH_RESPONSE_MIN
            ));
        }

        Ok(())
    })();

    let upstream_server_rpc = upstream_server.as_ref().map_or(0, SpawnedNode::rpc_port);
    let relay_rpc = relay.as_ref().map_or(0, SpawnedNode::rpc_port);
    let client_two_rpc = client_two.as_ref().map_or(0, SpawnedNode::rpc_port);
    let client_three_rpc = client_three.as_ref().map_or(0, SpawnedNode::rpc_port);
    let failure_details = if let Err(err) = &outcome {
        Some(format!(
            "{err}\n\n{}\n\n{}\n\n{}\n\n{}",
            collect_node_diagnostics(
                "upstream-server",
                upstream_server_rpc,
                upstream_server.as_mut()
            ),
            collect_node_diagnostics("relay", relay_rpc, relay.as_mut()),
            collect_node_diagnostics("client-two", client_two_rpc, client_two.as_mut()),
            collect_node_diagnostics("client-three", client_three_rpc, client_three.as_mut()),
        ))
    } else {
        None
    };

    if let Some(node) = client_three.as_mut() {
        terminate_child(node);
    }
    if let Some(node) = client_two.as_mut() {
        terminate_child(node);
    }
    if let Some(node) = relay.as_mut() {
        terminate_child(node);
    }
    if let Some(node) = upstream_server.as_mut() {
        terminate_child(node);
    }

    if let Some(details) = failure_details {
        panic!("four-node lxmd remote path flow failed:\n{details}");
    }
}
