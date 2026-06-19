#[cfg(test)]
mod tests {
    use super::*;
    use crate::destination::link::{Link, LinkHandleResult};
    use crate::destination::{DestinationDesc, DestinationName};
    use crate::hash::AddressHash;
    use crate::identity::PrivateIdentity;
    use crate::transport::{Transport, TransportConfig};
    use rand_core::OsRng;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::mpsc;
    use std::sync::Mutex as StdMutex;
    use tokio::sync::Mutex;
    use tokio::time::timeout;

    #[test]
    fn stream_data_message_roundtrips_compressed_payloads() {
        let payload = vec![b'A'; 256];
        let (message, processed) =
            RawChannelWriter::encode_chunk(7, payload.as_slice(), false).expect("chunk");
        assert_eq!(processed, payload.len());
        assert!(message.compressed);

        let decoded = StreamDataMessage::decode(&message.encode()).expect("decode");
        assert_eq!(decoded.stream_id, 7);
        assert_eq!(decoded.data, payload);
        assert!(!decoded.eof);
    }

    #[test]
    fn stream_data_message_rejects_oversized_compressed_payloads() {
        let payload = vec![b'A'; MAX_CHUNK_LEN + 1];
        let mut encoder = BzEncoder::new(Vec::new(), Compression::default());
        encoder.write_all(payload.as_slice()).expect("compress");
        let compressed = encoder.finish().expect("finish");

        let message = StreamDataMessage::new(7, compressed, false, true).expect("message");
        assert!(matches!(
            StreamDataMessage::decode(&message.encode()),
            Err(ChannelError::InvalidFrame)
        ));
    }

    #[tokio::test]
    async fn stream_data_message_rejects_out_of_range_stream_ids() {
        assert!(StreamDataMessage::new(STREAM_ID_MAX + 1, Vec::new(), false, false).is_err());
        let transport = test_transport();
        let channel = transport.channel(AddressHash::new_from_rand(OsRng));
        assert!(RawChannelWriter::new(STREAM_ID_MAX + 1, channel).is_err());
    }

    #[tokio::test]
    async fn raw_channel_reader_buffers_matching_stream_messages() {
        let transport = test_transport();
        let (outbound, mut inbound, iface, channel) = linked_channel(&transport).await;
        let reader = RawChannelReader::attach(23, channel).await.expect("reader");

        let ready = Arc::new(StdMutex::new(Vec::new()));
        let ready_clone = ready.clone();
        let (tx, rx) = mpsc::channel();
        reader.add_ready_callback(move |count| {
            ready_clone.lock().expect("lock").push(count);
            tx.send(count).expect("callback signal");
        });

        let message =
            StreamDataMessage::new(23, b"hello-channel".to_vec(), false, false).expect("message");
        let (_sequence, packet) = inbound
            .send_channel_message(StreamDataMessage::MSG_TYPE, message.encode())
            .expect("channel message");

        let result = outbound.lock().await.handle_packet(&packet, iface);
        assert!(matches!(result, LinkHandleResult::Proof(_)));
        assert_eq!(rx.recv_timeout(Duration::from_secs(1)).expect("ready callback"), 13);
        assert_eq!(reader.ready_len(), b"hello-channel".len());
        assert_eq!(reader.read(5).expect("chunk"), b"hello".to_vec());
        assert_eq!(reader.read(32).expect("chunk"), b"-channel".to_vec());
        assert_eq!(ready.lock().expect("lock").as_slice(), &[13]);
    }

    #[tokio::test]
    async fn raw_channel_reader_eof_only_triggers_ready_callback_with_zero() {
        let transport = test_transport();
        let (outbound, mut inbound, iface, channel) = linked_channel(&transport).await;
        let reader = RawChannelReader::attach(24, channel).await.expect("reader");

        let (tx, rx) = mpsc::channel();
        reader.add_ready_callback(move |count| {
            tx.send(count).expect("callback signal");
        });

        let message = StreamDataMessage::new(24, Vec::new(), true, false).expect("message");
        let (_sequence, packet) = inbound
            .send_channel_message(StreamDataMessage::MSG_TYPE, message.encode())
            .expect("channel message");

        let result = outbound.lock().await.handle_packet(&packet, iface);
        assert!(matches!(result, LinkHandleResult::Proof(_)));
        assert_eq!(rx.recv_timeout(Duration::from_secs(1)).expect("ready callback"), 0);
        assert_eq!(reader.read(64).expect("eof"), Vec::<u8>::new());
        assert!(reader.is_eof());
    }

    #[tokio::test]
    async fn raw_channel_reader_callbacks_run_detached_from_receive_lock() {
        let transport = test_transport();
        let (outbound, mut inbound, iface, channel) = linked_channel(&transport).await;
        let reader = RawChannelReader::attach(25, channel).await.expect("reader");

        let callback_started = Arc::new(AtomicBool::new(false));
        let callback_started_clone = callback_started.clone();
        let (tx, rx) = mpsc::channel();
        reader.add_ready_callback(move |count| {
            callback_started_clone.store(true, Ordering::SeqCst);
            std::thread::sleep(Duration::from_millis(100));
            tx.send(count).expect("callback signal");
        });

        let message = StreamDataMessage::new(25, b"async".to_vec(), false, false).expect("message");
        let (_sequence, packet) = inbound
            .send_channel_message(StreamDataMessage::MSG_TYPE, message.encode())
            .expect("channel message");

        let result = outbound.lock().await.handle_packet(&packet, iface);
        assert!(matches!(result, LinkHandleResult::Proof(_)));
        assert_eq!(reader.ready_len(), b"async".len());
        assert_eq!(reader.read(32).expect("chunk"), b"async".to_vec());
        assert!(timeout(Duration::from_secs(1), async move {
            loop {
                if callback_started.load(Ordering::SeqCst) {
                    break;
                }
                tokio::task::yield_now().await;
            }
        })
        .await
        .is_ok());
        assert_eq!(rx.recv_timeout(Duration::from_secs(1)).expect("ready callback"), 5);
    }

    #[tokio::test]
    async fn raw_channel_reader_callbacks_can_reenter_reader_without_deadlock() {
        let transport = test_transport();
        let (outbound, mut inbound, iface, channel) = linked_channel(&transport).await;
        let reader = RawChannelReader::attach(26, channel).await.expect("reader");
        let callback_reader = reader.clone();
        let (tx, rx) = std::sync::mpsc::sync_channel(1);
        reader.add_ready_callback(move |_| {
            tx.send(callback_reader.ready_len()).expect("send ready len");
        });

        let message =
            StreamDataMessage::new(26, b"reenter".to_vec(), false, false).expect("message");
        let (_sequence, packet) = inbound
            .send_channel_message(StreamDataMessage::MSG_TYPE, message.encode())
            .expect("channel message");

        let result = outbound.lock().await.handle_packet(&packet, iface);
        assert!(matches!(result, LinkHandleResult::Proof(_)));
        assert_eq!(rx.recv_timeout(Duration::from_secs(1)).expect("callback"), 7);
    }

    #[tokio::test]
    async fn raw_channel_reader_ready_callbacks_run_in_registration_order() {
        let transport = test_transport();
        let (outbound, mut inbound, iface, channel) = linked_channel(&transport).await;
        let reader = RawChannelReader::attach(27, channel).await.expect("reader");

        let calls = Arc::new(StdMutex::new(Vec::new()));
        let (tx, rx) = mpsc::channel();

        for label in ["first", "second", "third"] {
            let calls = calls.clone();
            let tx = tx.clone();
            reader.add_ready_callback(move |count| {
                calls.lock().expect("lock").push((label, count));
                tx.send(label).expect("callback signal");
            });
        }
        drop(tx);

        let message =
            StreamDataMessage::new(27, b"ordered".to_vec(), false, false).expect("message");
        let (_sequence, packet) = inbound
            .send_channel_message(StreamDataMessage::MSG_TYPE, message.encode())
            .expect("channel message");

        let result = outbound.lock().await.handle_packet(&packet, iface);
        assert!(matches!(result, LinkHandleResult::Proof(_)));

        for _ in 0..3 {
            rx.recv_timeout(Duration::from_secs(1)).expect("ready callback");
        }
        assert_eq!(
            calls.lock().expect("lock").as_slice(),
            &[("first", 7), ("second", 7), ("third", 7)]
        );
    }

    #[tokio::test]
    async fn raw_channel_reader_close_unregisters_handler() {
        let transport = test_transport();
        let (outbound, mut inbound, iface, channel) = linked_channel(&transport).await;
        let reader = RawChannelReader::attach(9, channel).await.expect("reader");
        assert!(reader.close().await.expect("close"));

        let message =
            StreamDataMessage::new(9, b"after-close".to_vec(), false, false).expect("message");
        let (_sequence, packet) = inbound
            .send_channel_message(StreamDataMessage::MSG_TYPE, message.encode())
            .expect("channel message");

        let result = outbound.lock().await.handle_packet(&packet, iface);
        assert!(matches!(result, LinkHandleResult::Proof(_)));
        assert!(reader.read(64).is_none());
    }

    #[test]
    fn raw_channel_writer_encode_chunk_accepts_large_prefix() {
        let payload = vec![b'Z'; STREAM_DATA_MAX_LEN * 2 + 17];
        let (message, processed) =
            RawChannelWriter::encode_chunk(11, payload.as_slice(), false).expect("chunk");

        assert!(processed > 0);
        assert!(processed <= payload.len());
        assert!(message.encode().len() <= PACKET_MDU);
    }

    #[tokio::test]
    async fn raw_channel_writer_write_all_returns_zero_without_ready_link() {
        let transport = test_transport();
        let (_outbound, _inbound, _iface, channel) = linked_channel(&transport).await;
        let writer = RawChannelWriter::new(11, channel).expect("writer");
        let payload = vec![b'Z'; STREAM_DATA_MAX_LEN * 2 + 17];

        let written = writer.write_all(payload.as_slice()).await.expect("write all");
        assert_eq!(written, 0);
    }

    #[tokio::test]
    async fn raw_channel_writer_returns_zero_when_link_not_ready() {
        let transport = test_transport();
        let (_outbound, _inbound, _iface, channel) = linked_channel(&transport).await;
        let writer = RawChannelWriter::new(12, channel).expect("writer");
        let payload = vec![b'Q'; STREAM_DATA_MAX_LEN];

        assert_eq!(writer.write(payload.as_slice()).await.expect("backpressure"), 0);
    }

    #[tokio::test]
    async fn raw_channel_writer_close_is_best_effort_under_backpressure() {
        let transport = test_transport();
        let (_outbound, _inbound, _iface, channel) = linked_channel(&transport).await;
        let mut writer = RawChannelWriter::new(13, channel).expect("writer");

        writer.close().await.expect("close");
        assert!(writer.eof_sent.load(Ordering::Acquire));
    }

    #[tokio::test]
    async fn raw_channel_writer_refuses_writes_after_eof() {
        let transport = test_transport();
        let channel = transport.channel(AddressHash::new_from_rand(OsRng));
        let writer = RawChannelWriter::new(13, channel).expect("writer");
        writer.eof_sent.store(true, Ordering::Release);

        assert_eq!(writer.write(b"after-eof").await.expect("write"), 0);
        assert_eq!(writer.write_all(b"after-eof").await.expect("write all"), 0);
    }

    #[tokio::test]
    async fn buffer_create_bidirectional_buffer_builds_reader_and_writer() {
        let transport = test_transport();
        let (_outbound, _inbound, _iface, channel) = linked_channel(&transport).await;

        let pair = Buffer::create_bidirectional_buffer_with_callback(21, 22, channel, |_ready| {})
            .await
            .expect("pair");

        assert_eq!(pair.reader.stream_id(), 21);
        assert_eq!(pair.writer.stream_id(), 22);
    }

    fn test_transport() -> Transport {
        let identity = PrivateIdentity::new_from_rand(OsRng);
        let config = TransportConfig::new("test", &identity, true);
        Transport::new(config)
    }

    async fn linked_channel(
        transport: &Transport,
    ) -> (Arc<Mutex<Link>>, Link, AddressHash, TransportChannel) {
        let signer = PrivateIdentity::new_from_rand(OsRng);
        let identity = *signer.as_identity();
        let destination = DestinationDesc {
            identity,
            address_hash: identity.address_hash,
            name: DestinationName::new("lxmf", "delivery"),
        };
        let outbound = transport.link(destination).await;
        let request = outbound.lock().await.request();
        let (tx, _) = tokio::sync::broadcast::channel(8);
        let mut inbound =
            Link::new_from_request(&request, signer.sign_key().clone(), destination, tx)
                .expect("link request should parse");
        let iface = AddressHash::new_from_rand(OsRng);
        assert!(matches!(
            outbound.lock().await.handle_packet(&inbound.prove(), iface),
            LinkHandleResult::Activated
        ));

        let link_id = *outbound.lock().await.id();
        let channel = transport.channel(link_id);

        (outbound, inbound, iface, channel)
    }
}
