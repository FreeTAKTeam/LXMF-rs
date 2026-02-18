use std::sync::Once;
use std::time::Duration;

use rand_core::OsRng;
use reticulum::{
    destination::DestinationName,
    hash::AddressHash,
    identity::PrivateIdentity,
    iface::{tcp_client::TcpClient, tcp_server::TcpServer},
    transport::{Transport, TransportConfig},
};
use tokio::{task::yield_now, time};

static INIT: Once = Once::new();

fn setup() {
    INIT.call_once(|| {
        let _ = env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("warn"))
            .is_test(true)
            .try_init();
    });
}

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

async fn wait_for_destination(
    transport: &Transport,
    destination: &AddressHash,
    timeout: Duration,
) -> bool {
    let deadline = std::time::Instant::now() + timeout;
    while std::time::Instant::now() < deadline {
        if transport.knows_destination(destination).await {
            return true;
        }
        yield_now().await;
    }
    false
}

async fn build_transport_full(
    name: &str,
    server_addr: &str,
    client_addr: &[&str],
    retransmit: bool,
) -> Transport {
    let mut config = TransportConfig::new(name, &PrivateIdentity::new_from_rand(OsRng), true);

    if retransmit {
        config.set_retransmit(true);
    }

    let transport = Transport::new(config);

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

async fn build_transport(name: &str, server_addr: &str, client_addr: &[&str]) -> Transport {
    build_transport_full(name, server_addr, client_addr, false).await
}

#[tokio::test]
async fn calculate_hop_distance() {
    setup();

    let ports = reserve_ports(3);
    let addr_a = format!("127.0.0.1:{}", ports[0]);
    let addr_b = format!("127.0.0.1:{}", ports[1]);
    let addr_c = format!("127.0.0.1:{}", ports[2]);

    let mut transport_a = build_transport("a", &addr_a, &[]).await;
    let mut transport_b = build_transport("b", &addr_b, &[&addr_a]).await;
    let mut transport_c = build_transport("c", &addr_c, &[&addr_a, &addr_b]).await;

    let _id_a = PrivateIdentity::new_from_name("a");
    let id_b = PrivateIdentity::new_from_name("b");
    let id_c = PrivateIdentity::new_from_name("c");

    let dest_a = transport_a.add_destination(_id_a, DestinationName::new("test", "hop")).await;

    let _dest_b = transport_b.add_destination(id_b, DestinationName::new("test", "hop")).await;

    let _dest_c = transport_c.add_destination(id_c, DestinationName::new("test", "hop")).await;

    time::sleep(Duration::from_millis(250)).await;

    println!("======");
    transport_a.send_announce(&dest_a, None).await;

    transport_b.recv_announces().await;
    transport_c.recv_announces().await;

    time::sleep(Duration::from_millis(250)).await;
}

#[tokio::test]
async fn direct_path_request_and_response() {
    setup();

    let ports = reserve_ports(2);
    let addr_a = format!("127.0.0.1:{}", ports[0]);
    let addr_b = format!("127.0.0.1:{}", ports[1]);
    let transport_a = build_transport("a", &addr_a, &[]).await;
    let mut transport_b = build_transport("b", &addr_b, &[&addr_a]).await;

    let id_b = PrivateIdentity::new_from_name("b");

    let dest_b = transport_b.add_destination(id_b, DestinationName::new("test", "hop")).await;
    let _dest_b_hash = dest_b.lock().await.desc.address_hash;

    let deadline = std::time::Instant::now() + Duration::from_secs(2);
    while std::time::Instant::now() < deadline {
        transport_a.request_path(&_dest_b_hash, None, None).await;
        if wait_for_destination(&transport_a, &_dest_b_hash, Duration::from_millis(75)).await {
            break;
        }
    }

    assert!(transport_a.knows_destination(&_dest_b_hash).await);
}

#[tokio::test]
async fn remote_path_request_and_response() {
    setup();

    let ports = reserve_ports(3);
    let addr_a = format!("127.0.0.1:{}", ports[0]);
    let addr_b = format!("127.0.0.1:{}", ports[1]);
    let addr_c = format!("127.0.0.1:{}", ports[2]);

    let transport_a = build_transport("a", &addr_a, &[]).await;
    let mut transport_b = build_transport_full("b", &addr_b, &[&addr_a], true).await;
    let mut transport_c = build_transport("c", &addr_c, &[&addr_b]).await;

    let id_c = PrivateIdentity::new_from_name("c");
    let dest_c = transport_c.add_destination(id_c, DestinationName::new("test", "hop")).await;
    let dest_c_hash = dest_c.lock().await.desc.address_hash;

    let id_b = PrivateIdentity::new_from_name("b");
    let dest_b = transport_b.add_destination(id_b, DestinationName::new("test", "hop")).await;
    let dest_b_hash = dest_b.lock().await.desc.address_hash;

    transport_c.send_announce(&dest_c, None).await;
    assert!(
        wait_for_destination(&transport_b, &dest_c_hash, Duration::from_secs(2)).await,
        "transport b should learn destination c"
    );

    // Advance time past the announce timeout, so the regular announce of
    // destination c is not propagated to a and we can test if a's path
    // request is successful.
    time::pause();
    time::advance(time::Duration::from_secs(3600)).await;

    transport_b.send_announce(&dest_b, None).await;
    assert!(
        wait_for_destination(&transport_a, &dest_b_hash, Duration::from_secs(2)).await,
        "transport a should learn destination b before requesting c"
    );

    let deadline = std::time::Instant::now() + Duration::from_secs(2);
    while std::time::Instant::now() < deadline {
        transport_a.request_path(&dest_c_hash, None, None).await;
        if wait_for_destination(&transport_a, &dest_c_hash, Duration::from_millis(75)).await {
            break;
        }
    }

    assert!(transport_a.knows_destination(&dest_c_hash).await);
}
