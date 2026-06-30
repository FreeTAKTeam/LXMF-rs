#![recursion_limit = "256"]

use std::io::{Read, Write};
use std::net::{Shutdown, TcpListener};
#[cfg(unix)]
use std::os::unix::net::UnixListener;
use std::process::Command;
use std::thread;

use rns_rpc::rpc::codec;
use rns_rpc::RpcResponse;
use serde_json::json;

#[test]
fn rnstatus_help_exposes_rpc_options() {
    let output = Command::new(rnstatus_bin()).arg("--help").output().expect("run rnstatus-rs");
    assert!(output.status.success(), "stderr: {}", String::from_utf8_lossy(&output.stderr));
    let stdout = String::from_utf8(output.stdout).expect("utf8 stdout");
    assert!(stdout.contains("--rpc <ADDR>"), "stdout: {stdout}");
    assert!(stdout.contains("127.0.0.1:4243"), "stdout: {stdout}");
    #[cfg(unix)]
    assert!(stdout.contains("--rpc-unix"), "stdout: {stdout}");
}

#[test]
fn rnstatus_fetches_daemon_status_and_renders_interface_runtime_state() {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind mock rpc");
    let rpc = listener.local_addr().expect("mock rpc addr").to_string();
    let server = thread::spawn(move || {
        let (mut stream, _) = listener.accept().expect("accept rpc request");
        let mut request = Vec::new();
        stream.read_to_end(&mut request).expect("read rpc request");
        let body = http_body(&request);
        let rpc_request = codec::decode_frame::<rns_rpc::RpcRequest>(body).expect("decode request");
        assert_eq!(rpc_request.method, "daemon_status_ex");

        let response = RpcResponse {
            id: rpc_request.id,
            result: Some(json!({
                "identity_hash": "0123456789abcdef0123456789abcdef",
                "running": true,
                "peer_count": 2,
                "interface_count": 8,
                "propagation": {
                    "enabled": true,
                    "selected_node": "cafebabe",
                    "sync_state": 0,
                    "sync_progress": 1.0,
                    "target_cost": 8,
                    "from_static_only": false
                },
                "interfaces": [
                    {
                        "name": "field-uplink",
                        "type": "tcp_server",
                        "enabled": true,
                        "host": "0.0.0.0",
                        "port": 4242,
                        "settings": {
                            "_runtime": {
                                "startup_status": "failed",
                                "startup_error": "bind denied"
                            }
                        }
                    },
                    {
                        "name": "auto-main",
                        "type": "auto",
                        "enabled": true,
                        "settings": {
                            "_runtime": {
                                "startup_status": "spawned",
                                "auto": {
                                    "carrier_runtime": {
                                        "online": true,
                                        "final_init_done": true,
                                        "carrier_changed": true,
                                        "carrier_event_count": 1,
                                        "adopted_device_count": 1,
                                        "adopted_add_count": 2,
                                        "adopted_remove_count": 1,
                                        "link_local_replacement_count": 1,
                                        "carrier_events": [
                                            {
                                                "event": "carrier_recovered",
                                                "ifname": "eth0"
                                            }
                                        ],
                                        "link_local_update": {
                                            "ifname": "eth0",
                                            "old_link_local_address": "fe80::1234%eth0",
                                            "new_link_local_address": "fe80::5678%eth0"
                                        }
                                    }
                                }
                            }
                        }
                    },
                    {
                        "name": "i2p-main",
                        "type": "i2p",
                        "enabled": true,
                        "settings": {
                            "_runtime": {
                                "startup_status": "spawned",
                                "i2p": {
                                    "tunnel_status": {
                                        "sam_endpoint": "127.0.0.1:7656",
                                        "accept_state": "listening",
                                        "configured_peer_count": 1,
                                        "last_accept_error": null,
                                        "peers": [
                                            {
                                                "direction": "outbound",
                                                "state": "connected",
                                                "bytes_rx": 3,
                                                "bytes_tx": 7
                                            },
                                            {
                                                "direction": "incoming",
                                                "state": "closed",
                                                "bytes_rx": 11,
                                                "bytes_tx": 0
                                            }
                                        ]
                                    }
                                }
                            }
                        }
                    },
                    {
                        "name": "backbone-main",
                        "type": "backbone_client",
                        "enabled": true,
                        "settings": {
                            "_runtime": {
                                "startup_status": "spawned",
                                "tcp": {
                                    "stream_status": {
                                        "endpoint": "127.0.0.1:4242",
                                        "stream_state": "reconnecting",
                                        "reconnect_attempts": 3,
                                        "bytes_rx": 12,
                                        "bytes_tx": 34,
                                        "keepalives_sent": 2,
                                        "stale_events": 1,
                                        "read_timeouts": 1,
                                        "closed_events": 1,
                                        "error_events": 1,
                                        "liveness_enabled": true,
                                        "forced_bitrate_bps": 9600,
                                        "last_error": "tcp stream read timeout"
                                    }
                                }
                            }
                        }
                    },
                    {
                        "name": "backbone-listener",
                        "type": "backbone",
                        "enabled": true,
                        "settings": {
                            "_runtime": {
                                "startup_status": "active",
                                "tcp": {
                                    "listener_status": {
                                        "bind_addr": "0.0.0.0:4242",
                                        "listener_state": "listening",
                                        "client_liveness_enabled": true,
                                        "client_forced_bitrate_bps": 9600,
                                        "accepted_connections": 2,
                                        "accept_errors": 1,
                                        "latest_client_endpoint": "127.0.0.1:54000",
                                        "latest_stream_status": {
                                            "stream_state": "connected",
                                            "bytes_rx": 56,
                                            "bytes_tx": 78
                                        },
                                        "last_error": null
                                    }
                                }
                            }
                        }
                    },
                    {
                        "name": "weave-main",
                        "type": "weave",
                        "enabled": true,
                        "settings": {
                            "_runtime": {
                                "startup_status": "spawned",
                                "weave": {
                                    "status": {
                                        "link_state": "reconnecting",
                                        "endpoint_count": 2,
                                        "wdcl_connected": false,
                                        "remote_switch_id": "0011223344556677",
                                        "bytes_rx": 120,
                                        "bytes_tx": 80,
                                        "frames_rx": 9,
                                        "frames_tx": 7,
                                        "invalid_frames": 1,
                                        "last_log_event": "0xe003",
                                        "display": {
                                            "color_format": 1,
                                            "width": 128,
                                            "height": 64,
                                            "total_size": 1024,
                                            "received_size": 512,
                                            "complete": false
                                        },
                                        "device_stats": {
                                            "cpu_load": 42,
                                            "memory_used_percent_bp": 5125,
                                            "task_cpu": {
                                                "wdcl": {
                                                    "cpu_load": 7,
                                                    "samples": 3
                                                },
                                                "ui": {
                                                    "cpu_load": 5,
                                                    "samples": 1
                                                }
                                            }
                                        },
                                        "last_error": "synthetic weave write failure"
                                    }
                                }
                            }
                        }
                    },
                    {
                        "name": "rnode-multi",
                        "type": "rnode_multi",
                        "enabled": true,
                        "settings": {
                            "_runtime": {
                                "startup_status": "spawned",
                                "rnode_multi": {
                                    "radio_status": {
                                        "stream_state": "write_failed",
                                        "selected_vport": 2,
                                        "last_error": "data frame write failed",
                                        "startup_probe": {
                                            "detected": true,
                                            "firmware_version": {
                                                "major": 1,
                                                "minor": 74,
                                                "label": "1.74"
                                            },
                                            "platform": 128,
                                            "mcu": 1,
                                            "interfaces": {
                                                "2": "SX126X",
                                                "3": "SX128X"
                                            },
                                            "interface_summary": "2:SX126X,3:SX128X"
                                        },
                                        "vports": [2, 3]
                                    }
                                }
                            }
                        }
                    },
                    {
                        "name": "vrn76-main",
                        "type": "vrn76_kiss_ble",
                        "enabled": true,
                        "settings": {
                            "_runtime": {
                                "startup_status": "spawned",
                                "vrn76": {
                                    "status": {
                                        "connected": true,
                                        "subscribed": true,
                                        "interface_ready": true,
                                        "startup_write_failures": 1,
                                        "pending_payloads": 2,
                                        "pending_writes": 3,
                                        "pending_packets": 4
                                    }
                                }
                            }
                        }
                    }
                ]
            })),
            error: None,
        };
        let body = codec::encode_frame(&response).expect("encode response");
        let response = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: application/msgpack\r\nContent-Length: {}\r\n\r\n",
            body.len()
        );
        stream.write_all(response.as_bytes()).expect("write response headers");
        stream.write_all(&body).expect("write response body");
        stream.shutdown(Shutdown::Write).expect("shutdown response");
    });

    let output = Command::new(rnstatus_bin())
        .arg("--rpc")
        .arg(rpc)
        .arg("--json")
        .output()
        .expect("run rnstatus-rs");
    assert!(output.status.success(), "stderr: {}", String::from_utf8_lossy(&output.stderr));
    let stdout = String::from_utf8(output.stdout).expect("utf8 stdout");
    let value: serde_json::Value = serde_json::from_str(&stdout).expect("json output");
    assert_eq!(value["identity_hash"], "0123456789abcdef0123456789abcdef");
    assert_eq!(value["interfaces"][0]["settings"]["_runtime"]["startup_status"], "failed");
    assert_eq!(
        value["interfaces"][4]["settings"]["_runtime"]["tcp"]["listener_status"]["listener_state"],
        "listening"
    );
    assert_eq!(
        value["interfaces"][7]["settings"]["_runtime"]["vrn76"]["status"]["interface_ready"],
        true
    );

    server.join().expect("mock rpc server");

    let listener = TcpListener::bind("127.0.0.1:0").expect("bind mock rpc");
    let rpc = listener.local_addr().expect("mock rpc addr").to_string();
    let server = thread::spawn(move || {
        let (mut stream, _) = listener.accept().expect("accept rpc request");
        let mut request = Vec::new();
        stream.read_to_end(&mut request).expect("read rpc request");
        let body = http_body(&request);
        let rpc_request = codec::decode_frame::<rns_rpc::RpcRequest>(body).expect("decode request");
        let response = RpcResponse {
            id: rpc_request.id,
            result: Some(json!({
                "identity_hash": "0123456789abcdef0123456789abcdef",
                "running": true,
                "peer_count": 2,
                "interface_count": 13,
                "propagation": {
                    "enabled": true,
                    "selected_node": "cafebabe",
                    "sync_state": 0,
                    "sync_progress": 1.0,
                    "target_cost": 8,
                    "from_static_only": false
                },
                "interfaces": [
                    {
                        "name": "field-uplink",
                        "type": "tcp_server",
                        "enabled": true,
                        "host": "0.0.0.0",
                        "port": 4242,
                        "settings": {
                            "_runtime": {
                                "startup_status": "failed",
                                "startup_error": "bind denied"
                            }
                        }
                    },
                    {
                        "name": "auto-main",
                        "type": "auto",
                        "enabled": true,
                        "settings": {
                            "_runtime": {
                                "startup_status": "spawned",
                                "auto": {
                                    "carrier_runtime": {
                                        "online": true,
                                        "final_init_done": true,
                                        "carrier_changed": true,
                                        "carrier_event_count": 1,
                                        "adopted_device_count": 1,
                                        "adopted_add_count": 2,
                                        "adopted_remove_count": 1,
                                        "link_local_replacement_count": 1,
                                        "carrier_events": [
                                            {
                                                "event": "carrier_recovered",
                                                "ifname": "eth0"
                                            }
                                        ],
                                        "link_local_update": {
                                            "ifname": "eth0",
                                            "old_link_local_address": "fe80::1234%eth0",
                                            "new_link_local_address": "fe80::5678%eth0"
                                        }
                                    }
                                }
                            }
                        }
                    },
                    {
                        "name": "i2p-main",
                        "type": "i2p",
                        "enabled": true,
                        "settings": {
                            "_runtime": {
                                "startup_status": "spawned",
                                "i2p": {
                                    "tunnel_status": {
                                        "sam_endpoint": "127.0.0.1:7656",
                                        "accept_state": "listening",
                                        "configured_peer_count": 1,
                                        "last_accept_error": null,
                                        "peers": [
                                            {
                                                "direction": "outbound",
                                                "state": "connected",
                                                "bytes_rx": 3,
                                                "bytes_tx": 7
                                            },
                                            {
                                                "direction": "incoming",
                                                "state": "closed",
                                                "bytes_rx": 11,
                                                "bytes_tx": 0
                                            }
                                        ]
                                    }
                                }
                            }
                        }
                    },
                    {
                        "name": "backbone-main",
                        "type": "backbone_client",
                        "enabled": true,
                        "settings": {
                            "_runtime": {
                                "startup_status": "spawned",
                                "tcp": {
                                    "stream_status": {
                                        "endpoint": "127.0.0.1:4242",
                                        "stream_state": "reconnecting",
                                        "reconnect_attempts": 3,
                                        "bytes_rx": 12,
                                        "bytes_tx": 34,
                                        "keepalives_sent": 2,
                                        "stale_events": 1,
                                        "read_timeouts": 1,
                                        "closed_events": 1,
                                        "error_events": 1,
                                        "liveness_enabled": true,
                                        "forced_bitrate_bps": 9600,
                                        "last_error": "tcp stream read timeout"
                                    }
                                }
                            }
                        }
                    },
                    {
                        "name": "backbone-listener",
                        "type": "backbone",
                        "enabled": true,
                        "settings": {
                            "_runtime": {
                                "startup_status": "active",
                                "tcp": {
                                    "listener_status": {
                                        "bind_addr": "0.0.0.0:4242",
                                        "listener_state": "listening",
                                        "client_liveness_enabled": true,
                                        "client_forced_bitrate_bps": 9600,
                                        "accepted_connections": 2,
                                        "accept_errors": 1,
                                        "latest_client_endpoint": "127.0.0.1:54000",
                                        "latest_stream_status": {
                                            "stream_state": "connected",
                                            "bytes_rx": 56,
                                            "bytes_tx": 78
                                        },
                                        "last_error": null
                                    }
                                }
                            }
                        }
                    },
                    {
                        "name": "weave-main",
                        "type": "weave",
                        "enabled": true,
                        "settings": {
                            "_runtime": {
                                "startup_status": "spawned",
                                "weave": {
                                    "status": {
                                        "link_state": "reconnecting",
                                        "endpoint_count": 2,
                                        "wdcl_connected": false,
                                        "remote_switch_id": "0011223344556677",
                                        "bytes_rx": 120,
                                        "bytes_tx": 80,
                                        "frames_rx": 9,
                                        "frames_tx": 7,
                                        "invalid_frames": 1,
                                        "last_log_event": "0xe003",
                                        "display": {
                                            "color_format": 1,
                                            "width": 128,
                                            "height": 64,
                                            "total_size": 1024,
                                            "received_size": 512,
                                            "complete": false
                                        },
                                        "device_stats": {
                                            "cpu_load": 42,
                                            "memory_used_percent_bp": 5125,
                                            "task_cpu": {
                                                "wdcl": {
                                                    "cpu_load": 7,
                                                    "samples": 3
                                                },
                                                "ui": {
                                                    "cpu_load": 5,
                                                    "samples": 1
                                                }
                                            }
                                        },
                                        "last_error": "synthetic weave write failure"
                                    }
                                }
                            }
                        }
                    },
                    {
                        "name": "rnode-multi",
                        "type": "rnode_multi",
                        "enabled": true,
                        "settings": {
                            "_runtime": {
                                "startup_status": "spawned",
                                "rnode_multi": {
                                    "radio_status": {
                                        "stream_state": "write_failed",
                                        "selected_vport": 2,
                                        "last_error": "data frame write failed",
                                        "startup_probe": {
                                            "detected": true,
                                            "firmware_version": {
                                                "major": 1,
                                                "minor": 74,
                                                "label": "1.74"
                                            },
                                            "platform": 128,
                                            "mcu": 1,
                                            "interfaces": {
                                                "2": "SX126X",
                                                "3": "SX128X"
                                            },
                                            "interface_summary": "2:SX126X,3:SX128X"
                                        },
                                        "vports": [2, 3]
                                    }
                                }
                            }
                        }
                    },
                    {
                        "name": "vrn76-main",
                        "type": "vrn76_kiss_ble",
                        "enabled": true,
                        "settings": {
                            "_runtime": {
                                "startup_status": "spawned",
                                "vrn76": {
                                    "status": {
                                        "connected": true,
                                        "subscribed": true,
                                        "interface_ready": true,
                                        "startup_write_failures": 1,
                                        "pending_payloads": 2,
                                        "pending_writes": 3,
                                        "pending_packets": 4
                                    }
                                }
                            }
                        }
                    },
                    {
                        "name": "udp-main",
                        "type": "udp",
                        "enabled": true,
                        "settings": {
                            "_runtime": {
                                "startup_status": "spawned",
                                "udp": {
                                    "status": {
                                        "link_state": "configured",
                                        "role": "peer",
                                        "bind_addr": "127.0.0.1:4242",
                                        "forward_addr": "192.0.2.1:4242",
                                        "peer_routes": 2,
                                        "packets_rx": 3,
                                        "packets_tx": 4,
                                        "bytes_rx": 120,
                                        "bytes_tx": 80,
                                        "decode_errors": 1,
                                        "rx_queue_errors": 2,
                                        "socket_errors": 3,
                                        "tx_errors": 4,
                                        "dropped_direct": 5,
                                        "last_error": "simulated udp decode failure"
                                    }
                                }
                            }
                        }
                    },
                    {
                        "name": "serial-main",
                        "type": "serial",
                        "enabled": true,
                        "settings": {
                            "_runtime": {
                                "startup_status": "spawned",
                                "serial": {
                                    "status": {
                                        "link_state": "configured",
                                        "device": "/dev/ttyUSB0",
                                        "baud_rate": 19200,
                                        "data_bits": 7,
                                        "parity": "even",
                                        "stop_bits": 2,
                                        "flow_control": "hardware",
                                        "mtu": 1024,
                                        "reconnect_attempts": 2,
                                        "open_errors": 1,
                                        "packets_rx": 3,
                                        "packets_tx": 4,
                                        "frames_rx": 5,
                                        "frames_tx": 6,
                                        "bytes_rx": 120,
                                        "bytes_tx": 80,
                                        "decode_errors": 1,
                                        "deserialize_errors": 2,
                                        "rx_queue_errors": 3,
                                        "serialize_errors": 4,
                                        "hdlc_encode_errors": 5,
                                        "tx_errors": 6,
                                        "read_errors": 7,
                                        "eof_count": 8,
                                        "last_error": "simulated serial read failure"
                                    }
                                }
                            }
                        }
                    },
                    {
                        "name": "kiss-main",
                        "type": "ax25_kiss",
                        "enabled": true,
                        "settings": {
                            "_runtime": {
                                "startup_status": "spawned",
                                "kiss": {
                                    "status": {
                                        "link_state": "configured",
                                        "bearer": "serial",
                                        "device": "/dev/ttyKISS0",
                                        "baud_rate": 1200,
                                        "mtu": 564,
                                        "preamble_ms": 350,
                                        "tx_tail_ms": 20,
                                        "kiss_flow_control": true,
                                        "ax25": true,
                                        "callsign": "N0CALL",
                                        "ssid": 1,
                                        "id_callsign": "MYCALL-0",
                                        "id_interval": 600,
                                        "interface_ready": false,
                                        "pending_depth": 2,
                                        "reconnect_attempts": 3,
                                        "open_errors": 1,
                                        "packets_rx": 4,
                                        "packets_tx": 5,
                                        "data_frames_rx": 6,
                                        "data_frames_tx": 7,
                                        "command_frames_rx": 8,
                                        "ready_frames_rx": 9,
                                        "init_frames_tx": 10,
                                        "shutdown_frames_tx": 11,
                                        "management_frames_tx": 12,
                                        "activity_frames_tx": 13,
                                        "id_beacon_frames_tx": 14,
                                        "bytes_rx": 120,
                                        "bytes_tx": 80,
                                        "decode_errors": 1,
                                        "deserialize_errors": 2,
                                        "rx_queue_errors": 3,
                                        "serialize_errors": 4,
                                        "read_errors": 5,
                                        "tx_errors": 6,
                                        "eof_count": 7,
                                        "flow_control_timeouts": 8,
                                        "ax25_drops": 9,
                                        "data_notifications_dropped": 10,
                                        "command_notifications_dropped": 11,
                                        "last_error": "simulated kiss read failure"
                                    }
                                }
                            }
                        }
                    },
                    {
                        "name": "kiss-wifi",
                        "type": "kiss_tcp_client",
                        "enabled": true,
                        "settings": {
                            "_runtime": {
                                "startup_status": "spawned",
                                "kiss_tcp": {
                                    "status": {
                                        "link_state": "configured",
                                        "bearer": "tcp",
                                        "endpoint": "127.0.0.1:8001",
                                        "kiss_flow_control": false,
                                        "ax25": false,
                                        "connect_errors": 2,
                                        "packets_rx": 3,
                                        "packets_tx": 4,
                                        "bytes_rx": 55,
                                        "bytes_tx": 66
                                    }
                                }
                            }
                        }
                    },
                    {
                        "name": "ble-main",
                        "type": "ble_gatt",
                        "enabled": true,
                        "settings": {
                            "_runtime": {
                                "startup_status": "spawned",
                                "ble_gatt": {
                                    "status": {
                                        "link_state": "configured",
                                        "adapter": "Bluetooth",
                                        "peripheral_id": "AA:BB:CC:DD:EE:FF",
                                        "service_uuid": "12345678-1234-1234-1234-1234567890ab",
                                        "mtu": 128,
                                        "scan_timeout_ms": 10000,
                                        "connect_timeout_ms": 3000,
                                        "connected": true,
                                        "subscribed": true,
                                        "reconnect_attempts": 2,
                                        "packets_rx": 6,
                                        "packets_tx": 7,
                                        "frames_rx": 8,
                                        "frames_tx": 9,
                                        "notification_bytes_rx": 100,
                                        "bytes_rx": 80,
                                        "bytes_tx": 90,
                                        "write_chunks_tx": 10,
                                        "scan_errors": 1,
                                        "connect_errors": 2,
                                        "subscribe_errors": 3,
                                        "probe_write_errors": 4,
                                        "probe_read_errors": 5,
                                        "serialize_errors": 11,
                                        "hdlc_encode_errors": 12,
                                        "hdlc_decode_errors": 13,
                                        "deserialize_errors": 14,
                                        "rx_queue_errors": 15,
                                        "write_errors": 16,
                                        "read_errors": 17,
                                        "stale_buffer_drops": 18,
                                        "cleanup_errors": 19,
                                        "last_error": "simulated ble read failure"
                                    }
                                }
                            }
                        }
                    }
                ]
            })),
            error: None,
        };
        let body = codec::encode_frame(&response).expect("encode response");
        let response = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: application/msgpack\r\nContent-Length: {}\r\n\r\n",
            body.len()
        );
        stream.write_all(response.as_bytes()).expect("write response headers");
        stream.write_all(&body).expect("write response body");
        stream.shutdown(Shutdown::Write).expect("shutdown response");
    });

    let output =
        Command::new(rnstatus_bin()).arg("--rpc").arg(rpc).output().expect("run rnstatus-rs");
    assert!(output.status.success(), "stderr: {}", String::from_utf8_lossy(&output.stderr));
    let stdout = String::from_utf8(output.stdout).expect("utf8 stdout");
    assert!(stdout.contains("field-uplink"), "stdout: {stdout}");
    assert!(stdout.contains("tcp_server"), "stdout: {stdout}");
    assert!(stdout.contains("failed"), "stdout: {stdout}");
    assert!(stdout.contains("bind denied"), "stdout: {stdout}");
    assert!(stdout.contains("auto-main"), "stdout: {stdout}");
    assert!(
        stdout.contains("auto online=true init=true carrier_changed=true carrier_events=1"),
        "stdout: {stdout}"
    );
    assert!(stdout.contains("adopted=1"), "stdout: {stdout}");
    assert!(stdout.contains("added=2"), "stdout: {stdout}");
    assert!(stdout.contains("removed=1"), "stdout: {stdout}");
    assert!(stdout.contains("replaced=1"), "stdout: {stdout}");
    assert!(stdout.contains("link_local=eth0"), "stdout: {stdout}");
    assert!(stdout.contains("new_ll=fe80::5678%eth0"), "stdout: {stdout}");
    assert!(stdout.contains("i2p-main"), "stdout: {stdout}");
    assert!(stdout.contains("i2p sam=127.0.0.1:7656 accept=listening peers=2"), "stdout: {stdout}");
    assert!(stdout.contains("connected=1"), "stdout: {stdout}");
    assert!(stdout.contains("closed=1"), "stdout: {stdout}");
    assert!(stdout.contains("outbound=1"), "stdout: {stdout}");
    assert!(stdout.contains("incoming=1"), "stdout: {stdout}");
    assert!(stdout.contains("rx=14"), "stdout: {stdout}");
    assert!(stdout.contains("tx=7"), "stdout: {stdout}");
    assert!(stdout.contains("backbone-main"), "stdout: {stdout}");
    assert!(
        stdout.contains("tcp stream=reconnecting endpoint=127.0.0.1:4242 reconnects=3"),
        "stdout: {stdout}"
    );
    assert!(stdout.contains("keepalives=2"), "stdout: {stdout}");
    assert!(stdout.contains("stale=1"), "stdout: {stdout}");
    assert!(stdout.contains("timeouts=1"), "stdout: {stdout}");
    assert!(stdout.contains("errors=1"), "stdout: {stdout}");
    assert!(stdout.contains("liveness=true"), "stdout: {stdout}");
    assert!(stdout.contains("bitrate=9600"), "stdout: {stdout}");
    assert!(stdout.contains("err=tcp stream read timeout"), "stdout: {stdout}");
    assert!(stdout.contains("backbone-listener"), "stdout: {stdout}");
    assert!(
        stdout.contains("tcp listener=listening bind=0.0.0.0:4242 accepted=2"),
        "stdout: {stdout}"
    );
    assert!(stdout.contains("accept_errors=1"), "stdout: {stdout}");
    assert!(stdout.contains("child_liveness=true"), "stdout: {stdout}");
    assert!(stdout.contains("child_bitrate=9600"), "stdout: {stdout}");
    assert!(stdout.contains("latest=127.0.0.1:54000"), "stdout: {stdout}");
    assert!(stdout.contains("latest_state=connected"), "stdout: {stdout}");
    assert!(stdout.contains("latest_rx=56"), "stdout: {stdout}");
    assert!(stdout.contains("latest_tx=78"), "stdout: {stdout}");
    assert!(stdout.contains("weave-main"), "stdout: {stdout}");
    assert!(stdout.contains("weave link=reconnecting endpoints=2 wdcl=false"), "stdout: {stdout}");
    assert!(stdout.contains("remote=0011223344556677"), "stdout: {stdout}");
    assert!(stdout.contains("rx_frames=9"), "stdout: {stdout}");
    assert!(stdout.contains("tx_frames=7"), "stdout: {stdout}");
    assert!(stdout.contains("invalid_frames=1"), "stdout: {stdout}");
    assert!(stdout.contains("last_log=0xe003"), "stdout: {stdout}");
    assert!(stdout.contains("display=128x64/false"), "stdout: {stdout}");
    assert!(stdout.contains("display_bytes=512/1024"), "stdout: {stdout}");
    assert!(stdout.contains("color=1"), "stdout: {stdout}");
    assert!(stdout.contains("cpu=42"), "stdout: {stdout}");
    assert!(stdout.contains("mem=51.25%"), "stdout: {stdout}");
    assert!(stdout.contains("tasks=2"), "stdout: {stdout}");
    assert!(stdout.contains("err=synthetic weave write failure"), "stdout: {stdout}");
    assert!(stdout.contains("rnode-multi"), "stdout: {stdout}");
    assert!(
        stdout.contains("rnode_multi stream=write_failed selected=2 vports=2"),
        "stdout: {stdout}"
    );
    assert!(stdout.contains("detected=true"), "stdout: {stdout}");
    assert!(stdout.contains("fw=1.74"), "stdout: {stdout}");
    assert!(stdout.contains("platform=128"), "stdout: {stdout}");
    assert!(stdout.contains("mcu=1"), "stdout: {stdout}");
    assert!(stdout.contains("probe=2:SX126X,3:SX128X"), "stdout: {stdout}");
    assert!(stdout.contains("err=data frame write failed"), "stdout: {stdout}");
    assert!(stdout.contains("vrn76-main"), "stdout: {stdout}");
    assert!(stdout.contains("vrn76 connected=true subscribed=true ready=true"), "stdout: {stdout}");
    assert!(stdout.contains("startup_write_failures=1"), "stdout: {stdout}");
    assert!(stdout.contains("pending_payloads=2"), "stdout: {stdout}");
    assert!(stdout.contains("pending_writes=3"), "stdout: {stdout}");
    assert!(stdout.contains("pending_packets=4"), "stdout: {stdout}");
    assert!(stdout.contains("udp-main"), "stdout: {stdout}");
    assert!(
        stdout.contains("udp state=configured role=peer bind=127.0.0.1:4242"),
        "stdout: {stdout}"
    );
    assert!(stdout.contains("forward=192.0.2.1:4242"), "stdout: {stdout}");
    assert!(stdout.contains("peers=2"), "stdout: {stdout}");
    assert!(stdout.contains("rxp=3"), "stdout: {stdout}");
    assert!(stdout.contains("txp=4"), "stdout: {stdout}");
    assert!(stdout.contains("rx=120"), "stdout: {stdout}");
    assert!(stdout.contains("tx=80"), "stdout: {stdout}");
    assert!(stdout.contains("decode_errors=1"), "stdout: {stdout}");
    assert!(stdout.contains("rx_queue_errors=2"), "stdout: {stdout}");
    assert!(stdout.contains("socket_errors=3"), "stdout: {stdout}");
    assert!(stdout.contains("tx_errors=4"), "stdout: {stdout}");
    assert!(stdout.contains("dropped_direct=5"), "stdout: {stdout}");
    assert!(stdout.contains("err=simulated udp decode failure"), "stdout: {stdout}");
    assert!(stdout.contains("serial-main"), "stdout: {stdout}");
    assert!(
        stdout.contains("serial state=configured device=/dev/ttyUSB0 baud=19200"),
        "stdout: {stdout}"
    );
    assert!(stdout.contains("flow=hardware"), "stdout: {stdout}");
    assert!(stdout.contains("reconnects=2"), "stdout: {stdout}");
    assert!(stdout.contains("open_errors=1"), "stdout: {stdout}");
    assert!(stdout.contains("rx_frames=5"), "stdout: {stdout}");
    assert!(stdout.contains("tx_frames=6"), "stdout: {stdout}");
    assert!(stdout.contains("deserialize_errors=2"), "stdout: {stdout}");
    assert!(stdout.contains("serialize_errors=4"), "stdout: {stdout}");
    assert!(stdout.contains("hdlc_encode_errors=5"), "stdout: {stdout}");
    assert!(stdout.contains("read_errors=7"), "stdout: {stdout}");
    assert!(stdout.contains("eof=8"), "stdout: {stdout}");
    assert!(stdout.contains("err=simulated serial read failure"), "stdout: {stdout}");
    assert!(stdout.contains("kiss-main"), "stdout: {stdout}");
    assert!(
        stdout.contains("kiss state=configured bearer=serial device=/dev/ttyKISS0"),
        "stdout: {stdout}"
    );
    assert!(stdout.contains("ax25=true"), "stdout: {stdout}");
    assert!(stdout.contains("callsign=N0CALL"), "stdout: {stdout}");
    assert!(stdout.contains("ready=false"), "stdout: {stdout}");
    assert!(stdout.contains("pending=2"), "stdout: {stdout}");
    assert!(stdout.contains("data_rx=6"), "stdout: {stdout}");
    assert!(stdout.contains("cmd_rx=8"), "stdout: {stdout}");
    assert!(stdout.contains("beacon_tx=14"), "stdout: {stdout}");
    assert!(stdout.contains("flow_timeouts=8"), "stdout: {stdout}");
    assert!(stdout.contains("ax25_drops=9"), "stdout: {stdout}");
    assert!(stdout.contains("data_drops=10"), "stdout: {stdout}");
    assert!(stdout.contains("cmd_drops=11"), "stdout: {stdout}");
    assert!(stdout.contains("err=simulated kiss read failure"), "stdout: {stdout}");
    assert!(stdout.contains("kiss-wifi"), "stdout: {stdout}");
    assert!(
        stdout.contains("kiss state=configured bearer=tcp endpoint=127.0.0.1:8001"),
        "stdout: {stdout}"
    );
    assert!(stdout.contains("connect_errors=2"), "stdout: {stdout}");
    assert!(stdout.contains("rx=55"), "stdout: {stdout}");
    assert!(stdout.contains("tx=66"), "stdout: {stdout}");
    assert!(stdout.contains("ble-main"), "stdout: {stdout}");
    assert!(
        stdout.contains("ble_gatt state=configured peripheral=AA:BB:CC:DD:EE:FF"),
        "stdout: {stdout}"
    );
    assert!(stdout.contains("connected=true"), "stdout: {stdout}");
    assert!(stdout.contains("subscribed=true"), "stdout: {stdout}");
    assert!(stdout.contains("notify_rx=100"), "stdout: {stdout}");
    assert!(stdout.contains("chunks_tx=10"), "stdout: {stdout}");
    assert!(stdout.contains("scan_errors=1"), "stdout: {stdout}");
    assert!(stdout.contains("probe_read_errors=5"), "stdout: {stdout}");
    assert!(stdout.contains("hdlc_decode_errors=13"), "stdout: {stdout}");
    assert!(stdout.contains("buffer_drops=18"), "stdout: {stdout}");
    assert!(stdout.contains("err=simulated ble read failure"), "stdout: {stdout}");
    assert!(stdout.contains("Propagation: enabled=true"), "stdout: {stdout}");
    assert!(stdout.contains("peers=2"), "stdout: {stdout}");
    assert!(stdout.contains("selected=cafebabe"), "stdout: {stdout}");

    server.join().expect("mock rpc server");
}

#[test]
fn rnstatus_weave_display_renders_display_focused_view() {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind mock rpc");
    let rpc = listener.local_addr().expect("mock rpc addr").to_string();
    let server = thread::spawn(move || {
        let (mut stream, _) = listener.accept().expect("accept rpc request");
        let mut request = Vec::new();
        stream.read_to_end(&mut request).expect("read rpc request");
        let body = http_body(&request);
        let rpc_request = codec::decode_frame::<rns_rpc::RpcRequest>(body).expect("decode request");
        assert_eq!(rpc_request.method, "daemon_status_ex");

        let response = RpcResponse {
            id: rpc_request.id,
            result: Some(json!({
                "identity_hash": "0123456789abcdef0123456789abcdef",
                "running": true,
                "interface_count": 1,
                "interfaces": [
                    {
                        "name": "weave-main",
                        "type": "weave",
                        "enabled": true,
                        "settings": {
                            "_runtime": {
                                "startup_status": "spawned",
                                "weave": {
                                    "status": {
                                        "link_state": "connected",
                                        "wdcl_connected": true,
                                        "remote_switch_id": "0011223344556677",
                                        "display": {
                                            "color_format": 1,
                                            "width": 128,
                                            "height": 64,
                                            "total_size": 4,
                                            "received_size": 4,
                                            "complete": true,
                                            "buffer_hex": "aabbccdd"
                                        },
                                        "device_stats": {
                                            "cpu_load": 42,
                                            "memory_used_percent_bp": 5125,
                                            "task_cpu": {
                                                "wdcl": {
                                                    "cpu_load": 7,
                                                    "samples": 3
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                ]
            })),
            error: None,
        };
        let body = codec::encode_frame(&response).expect("encode response");
        let response = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: application/msgpack\r\nContent-Length: {}\r\n\r\n",
            body.len()
        );
        stream.write_all(response.as_bytes()).expect("write response headers");
        stream.write_all(&body).expect("write response body");
        stream.shutdown(Shutdown::Write).expect("shutdown response");
    });

    let output = Command::new(rnstatus_bin())
        .arg("--rpc")
        .arg(rpc)
        .arg("--weave-display")
        .arg("weave-main")
        .output()
        .expect("run rnstatus-rs");
    assert!(output.status.success(), "stderr: {}", String::from_utf8_lossy(&output.stderr));
    let stdout = String::from_utf8(output.stdout).expect("utf8 stdout");
    assert!(stdout.contains("Weave Display: weave-main"), "stdout: {stdout}");
    assert!(
        stdout.contains("link=connected wdcl=true remote=0011223344556677"),
        "stdout: {stdout}"
    );
    assert!(stdout.contains("size=128x64 complete=true color=1 bytes=4/4"), "stdout: {stdout}");
    assert!(stdout.contains("buffer_hex=aabbccdd"), "stdout: {stdout}");
    assert!(stdout.contains("stats cpu=42 mem=51.25% tasks=1"), "stdout: {stdout}");

    server.join().expect("mock rpc server");
}

#[test]
fn rnstatus_uses_default_tcp_rpc_when_no_transport_flag_is_supplied() {
    let Ok(listener) = TcpListener::bind("127.0.0.1:4243") else {
        eprintln!("skipping default TCP RPC test because 127.0.0.1:4243 is already in use");
        return;
    };
    let server = thread::spawn(move || {
        let (mut stream, _) = listener.accept().expect("accept rpc request");
        let mut request = Vec::new();
        stream.read_to_end(&mut request).expect("read rpc request");
        let request_text = String::from_utf8_lossy(&request);
        assert!(request_text.contains("\r\nHost: 127.0.0.1:4243\r\n"), "request: {request_text}");
        let body = http_body(&request);
        let rpc_request = codec::decode_frame::<rns_rpc::RpcRequest>(body).expect("decode request");
        assert_eq!(rpc_request.method, "daemon_status_ex");

        let response = RpcResponse {
            id: rpc_request.id,
            result: Some(json!({
                "identity_hash": "00110011001100110011001100110011",
                "running": true,
                "interface_count": 0,
                "interfaces": []
            })),
            error: None,
        };
        write_rpc_response(&mut stream, &response);
    });

    let output = Command::new(rnstatus_bin()).arg("--json").output().expect("run rnstatus-rs");
    assert!(output.status.success(), "stderr: {}", String::from_utf8_lossy(&output.stderr));
    let stdout = String::from_utf8(output.stdout).expect("utf8 stdout");
    let value: serde_json::Value = serde_json::from_str(&stdout).expect("json output");
    assert_eq!(value["identity_hash"], "00110011001100110011001100110011");

    server.join().expect("mock rpc server");
}

#[cfg(unix)]
#[test]
fn rnstatus_fetches_daemon_status_over_unix_rpc() {
    let temp = tempfile::tempdir().expect("temp dir");
    let socket_path = temp.path().join("rnstatus.sock");
    let listener = UnixListener::bind(&socket_path).expect("bind mock unix rpc");
    let server = thread::spawn(move || {
        let (mut stream, _) = listener.accept().expect("accept unix rpc request");
        let mut request = Vec::new();
        stream.read_to_end(&mut request).expect("read rpc request");
        let request_text = String::from_utf8_lossy(&request);
        assert!(request_text.starts_with("POST /rpc HTTP/1.1\r\n"), "request: {request_text}");
        assert!(request_text.contains("\r\nHost: localhost\r\n"), "request: {request_text}");
        let body = http_body(&request);
        let rpc_request = codec::decode_frame::<rns_rpc::RpcRequest>(body).expect("decode request");
        assert_eq!(rpc_request.id, 1);
        assert_eq!(rpc_request.method, "daemon_status_ex");
        assert!(rpc_request.params.is_none());

        let response = RpcResponse {
            id: rpc_request.id,
            result: Some(json!({
                "identity_hash": "fedcba9876543210fedcba9876543210",
                "running": true,
                "interface_count": 0,
                "interfaces": []
            })),
            error: None,
        };
        write_rpc_response(&mut stream, &response);
    });

    let output = Command::new(rnstatus_bin())
        .arg("--rpc-unix")
        .arg(&socket_path)
        .arg("--json")
        .output()
        .expect("run rnstatus-rs");
    assert!(output.status.success(), "stderr: {}", String::from_utf8_lossy(&output.stderr));
    let stdout = String::from_utf8(output.stdout).expect("utf8 stdout");
    let value: serde_json::Value = serde_json::from_str(&stdout).expect("json output");
    assert_eq!(value["identity_hash"], "fedcba9876543210fedcba9876543210");
    assert_eq!(value["running"], true);

    server.join().expect("mock unix rpc server");
}

#[cfg(unix)]
#[test]
fn rnstatus_rejects_tcp_and_unix_rpc_together() {
    let output = Command::new(rnstatus_bin())
        .arg("--rpc")
        .arg("127.0.0.1:4243")
        .arg("--rpc-unix")
        .arg("/tmp/rnstatus.sock")
        .output()
        .expect("run rnstatus-rs");
    assert!(!output.status.success());
    let stderr = String::from_utf8(output.stderr).expect("utf8 stderr");
    assert!(stderr.contains("cannot be used with"), "stderr: {stderr}");
}

fn rnstatus_bin() -> String {
    env!("CARGO_BIN_EXE_rnstatus-rs").to_string()
}

fn http_body(request: &[u8]) -> &[u8] {
    let marker = b"\r\n\r\n";
    let start = request
        .windows(marker.len())
        .position(|window| window == marker)
        .map(|index| index + marker.len())
        .expect("request headers");
    &request[start..]
}

fn write_rpc_response(stream: &mut impl Write, response: &RpcResponse) {
    let body = codec::encode_frame(response).expect("encode response");
    let response = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: application/msgpack\r\nContent-Length: {}\r\n\r\n",
        body.len()
    );
    stream.write_all(response.as_bytes()).expect("write response headers");
    stream.write_all(&body).expect("write response body");
}
