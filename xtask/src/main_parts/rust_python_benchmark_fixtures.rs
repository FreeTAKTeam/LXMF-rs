fn rust_lxmf_bench_text(name: &str) -> (&'static str, String) {
    if name.contains("resource_message") {
        ("wire-resource-title", "x".repeat(16_384))
    } else if name.contains("large_message") {
        ("wire-large-title", "x".repeat(2048))
    } else {
        ("wire-title", "wire-content".to_string())
    }
}

fn rust_unsigned_message(title: &str, content: &str) -> Message {
    let mut message = Message::new();
    message.destination_hash = Some([0x11; 16]);
    message.source_hash = Some([0x22; 16]);
    message.timestamp = Some(1_770_000_000.0);
    message.set_title_from_string(title);
    message.set_content_from_string(content);
    message
}

fn rust_sample_destination() -> SingleInputDestination {
    let identity = PrivateIdentity::new_from_rand(OsRng);
    SingleInputDestination::new(
        identity,
        DestinationName::new("example_utilities", "announcesample.fruits"),
    )
}

fn rust_announce_batch_packets() -> Result<Vec<rns_core::Packet>> {
    const ANNOUNCE_BATCH_SIZE: usize = 64;
    let mut packets = Vec::with_capacity(ANNOUNCE_BATCH_SIZE);
    for index in 0..ANNOUNCE_BATCH_SIZE {
        let mut destination = rust_sample_destination();
        let app_data = format!("{}-{index}", String::from_utf8_lossy(&shared_fixture_announce_data()));
        let packet = destination
            .announce(OsRng, Some(app_data.as_bytes()))
            .map_err(|err| anyhow!("announce should succeed: {err:?}"))?;
        packets.push(packet);
    }
    Ok(packets)
}

fn rust_active_link_pair() -> Result<(Link, Link, Vec<u8>)> {
    let sender = PrivateIdentity::new_from_rand(OsRng);
    let receiver = PrivateIdentity::new_from_rand(OsRng);
    let _sender = to_transport_private_identity(&sender);
    let receiver = to_transport_private_identity(&receiver);
    let destination = DestinationDesc {
        identity: *receiver.as_identity(),
        address_hash: *receiver.address_hash(),
        name: TransportDestinationName::new("lxmf", "delivery"),
    };
    let (tx, _) = tokio::sync::broadcast::channel(16);
    let mut outbound = Link::new(destination, tx.clone());
    let request = outbound.request();
    let mut inbound =
        Link::new_from_request(&request, receiver.sign_key().clone(), destination, tx)
            .map_err(|err| anyhow!("input link: {err:?}"))?;
    let proof = inbound.prove();
    let proof_iface = AddressHash::new_from_rand(OsRng);
    if !matches!(outbound.handle_packet(&proof, proof_iface), LinkHandleResult::Activated) {
        bail!("link activation did not succeed");
    }
    Ok((outbound, inbound, shared_fixture_packet_payload()))
}

fn shared_fixture_json() -> serde_json::Value {
    serde_json::from_str(include_str!("../../../tools/benchmarks/fixtures.json"))
        .expect("benchmark fixture JSON must be valid")
}

fn shared_fixture_payload(length_key: &str) -> Vec<u8> {
    let fixtures = shared_fixture_json();
    let length = fixtures["payloads"][length_key]
        .as_u64()
        .expect("benchmark payload length fixture must be an integer") as usize;
    let pattern = fixtures["payloads"]["resource_pattern_hex"]
        .as_str()
        .expect("benchmark payload pattern fixture must be a string");
    hex::decode(pattern)
        .expect("benchmark payload pattern must decode")
        .into_iter()
        .cycle()
        .take(length)
        .collect()
}

fn shared_fixture_announce_data() -> Vec<u8> {
    shared_fixture_json()["payloads"]["announce_app_data"]
        .as_str()
        .expect("benchmark announce fixture must be a string")
        .as_bytes()
        .to_vec()
}

fn shared_fixture_packet_payload() -> Vec<u8> {
    let fixtures = shared_fixture_json();
    let encoded = fixtures["payloads"]["packet_payload_hex"]
        .as_str()
        .expect("benchmark packet fixture must be a string");
    hex::decode(encoded)
        .expect("benchmark packet fixture must decode")
        .into_iter()
        .cycle()
        .take(128)
        .collect()
}

fn rust_decrypt_resource_packet(link: &Link, packet: &Packet) -> Result<Packet> {
    let mut plain_packet = packet.clone();
    let mut buffer = PacketDataBuffer::new();
    let plain_len = {
        let plaintext = link
            .decrypt(packet.data.as_slice(), buffer.accuire_buf_max())
            .map_err(|err| anyhow!("decrypt should succeed: {err:?}"))?;
        plaintext.len()
    };
    buffer.resize(plain_len);
    plain_packet.data = buffer;
    Ok(plain_packet)
}

fn rust_resource_manager_request_fixture() -> Result<(Link, ResourceManager, Packet)> {
    let (sender_link, mut receiver_link, _) = rust_active_link_pair()?;
    let mut sender_manager = ResourceManager::new();
    let mut receiver_manager = ResourceManager::new();
    let resource_data = vec![0x5a; PACKET_MDU * 6];
    let (_, advertisement_packet) = sender_manager
        .start_send(&sender_link, resource_data, None)
        .map_err(|err| anyhow!("resource send should succeed: {err:?}"))?;
    let plain_advertisement = rust_decrypt_resource_packet(&receiver_link, &advertisement_packet)?;
    let mut responses = Vec::new();
    receiver_manager.handle_packet_into(&plain_advertisement, &mut receiver_link, &mut responses);
    let request_packet = responses.pop().context("resource request packet")?;
    let plain_request = rust_decrypt_resource_packet(&sender_link, &request_packet)?;
    Ok((sender_link, sender_manager, plain_request))
}
