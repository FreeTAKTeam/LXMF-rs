use rand_core::OsRng;
use reticulum::{
    identity::PrivateIdentity,
    iface::{tcp_client::TcpClient, tcp_server::TcpServer},
    packet::Packet,
    transport::{Transport, TransportConfig},
};
use tokio_util::sync::CancellationToken;

fn reserve_ports(count: usize) -> Vec<u16> {
    let mut listeners = Vec::with_capacity(count);
    let mut ports = Vec::with_capacity(count);
    for _ in 0..count {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind ephemeral port");
        let port = listener.local_addr().expect("ephemeral addr").port();
        listeners.push(listener);
        ports.push(port);
    }
    drop(listeners);
    ports
}

async fn build_transport(name: &str, server_addr: &str, client_addr: &[&str]) -> Transport {
    let transport =
        Transport::new(TransportConfig::new(name, &PrivateIdentity::new_from_rand(OsRng), true));

    transport
        .iface_manager()
        .lock()
        .await
        .spawn(TcpServer::new(server_addr, transport.iface_manager()), TcpServer::spawn);

    for &addr in client_addr {
        transport.iface_manager().lock().await.spawn(TcpClient::new(addr), TcpClient::spawn);
    }

    log::info!("test: transport {} created", name);

    transport
}

#[tokio::test]
#[ignore = "stress test; run explicitly with --ignored"]
async fn packet_overload() {
    let _ = env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("error"))
        .is_test(true)
        .try_init();

    let ports = reserve_ports(2);
    let addr_a = format!("127.0.0.1:{}", ports[0]);
    let addr_b = format!("127.0.0.1:{}", ports[1]);
    let transport_a = build_transport("a", &addr_a, &[]).await;
    let transport_b = build_transport("b", &addr_b, &[&addr_a]).await;

    let stop = CancellationToken::new();

    let producer_task = {
        let stop = stop.clone();
        tokio::spawn(async move {
            let mut tx_counter = 0;

            let mut payload_size = 0;
            loop {
                tokio::select! {
                    _ = stop.cancelled() => {
                            break;
                    },
                    _ = tokio::time::sleep(std::time::Duration::from_micros(50)) => {

                        let mut packet = Packet::default();

                        packet.data.resize(payload_size);

                        payload_size += 1;
                        if payload_size >= 3072 {
                            payload_size = 0;
                        }

                        transport_a.send_packet(packet).await;
                        tx_counter += 1;
                    },
                };
            }

            tx_counter
        })
    };

    let consumer_task = {
        let stop = stop.clone();
        let mut messages = transport_b.iface_rx();
        tokio::spawn(async move {
            let mut rx_counter = 0;
            loop {
                tokio::select! {
                    _ = stop.cancelled() => {
                            break;
                    },
                    Ok(_) = messages.recv() => {
                        rx_counter += 1;
                    },
                };
            }

            rx_counter
        })
    };

    tokio::time::sleep(std::time::Duration::from_secs(2)).await;

    stop.cancel();

    let tx_counter = producer_task.await.unwrap();
    let rx_counter = consumer_task.await.unwrap();

    log::info!("TX: {}, RX: {}", tx_counter, rx_counter);
    assert!(tx_counter > 0, "producer should send packets");
}
