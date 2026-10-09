#[cfg(test)]
mod test {
    use bytes::Bytes;
    use futures::{select, FutureExt};
    use zeromq::__async_rt as async_rt;
    use zeromq::prelude::*;
    use zeromq::{proxy, XPubSocket, XSubSocket, ZmqMessage};

    use std::convert::TryFrom;
    use std::time::Duration;

    fn extract_port(endpoint: &str) -> u16 {
        endpoint
            .rsplit(':')
            .next()
            .unwrap()
            .parse()
            .expect("Failed to parse port")
    }

    #[async_rt::test]
    async fn test_xsub_basic_pubsub_and_topic_filter() {
        pretty_env_logger::try_init().ok();

        let mut xpub_socket = XPubSocket::new();
        let bound_to = xpub_socket
            .bind("tcp://127.0.0.1:0")
            .await
            .expect("Failed to bind");

        let bound_addr = bound_to.to_string();
        let xsub_handle = async_rt::task::spawn(async move {
            let mut xsub_socket = XSubSocket::new();
            xsub_socket
                .connect(&bound_addr)
                .await
                .expect("Failed to connect");
            xsub_socket
                .subscribe("topic1")
                .await
                .expect("Failed to subscribe");

            async_rt::task::sleep(Duration::from_millis(200)).await;

            let msg = xsub_socket.recv().await.expect("Failed to receive");
            String::from_utf8(msg.get(0).unwrap().to_vec()).unwrap()
        });

        let sub_msg = async_rt::task::timeout(Duration::from_secs(2), xpub_socket.recv())
            .await
            .expect("Timeout waiting for subscription")
            .expect("Failed to receive subscription");
        let data = sub_msg.get(0).unwrap();
        assert_eq!(data[0], 1);
        assert_eq!(&data[1..], b"topic1");

        async_rt::task::sleep(Duration::from_millis(100)).await;

        xpub_socket
            .send(ZmqMessage::from("topic2-message"))
            .await
            .expect("Failed to send");
        xpub_socket
            .send(ZmqMessage::from("topic1-message"))
            .await
            .expect("Failed to send");

        let received = xsub_handle.await.expect("XSUB task failed");
        assert_eq!(received, "topic1-message");
    }

    #[async_rt::test]
    async fn test_xsub_receives_unsubscribe() {
        pretty_env_logger::try_init().ok();

        let mut xpub_socket = XPubSocket::new();
        let bound_to = xpub_socket
            .bind("tcp://127.0.0.1:0")
            .await
            .expect("Failed to bind");

        let bound_addr = bound_to.to_string();
        let handle = async_rt::task::spawn(async move {
            let mut xsub_socket = XSubSocket::new();
            xsub_socket
                .connect(&bound_addr)
                .await
                .expect("Failed to connect");
            xsub_socket
                .subscribe("test")
                .await
                .expect("Failed to subscribe");
            async_rt::task::sleep(Duration::from_millis(100)).await;
            xsub_socket
                .unsubscribe("test")
                .await
                .expect("Failed to unsubscribe");
        });

        let sub_msg = async_rt::task::timeout(Duration::from_secs(2), xpub_socket.recv())
            .await
            .expect("Timeout waiting for subscribe")
            .expect("Failed to receive subscribe");
        assert_eq!(sub_msg.get(0).unwrap().as_ref(), b"\x01test");

        let unsub_msg = async_rt::task::timeout(Duration::from_secs(2), xpub_socket.recv())
            .await
            .expect("Timeout waiting for unsubscribe")
            .expect("Failed to receive unsubscribe");
        assert_eq!(unsub_msg.get(0).unwrap().as_ref(), b"\x00test");

        handle.await.expect("Task failed");
    }

    #[async_rt::test]
    async fn test_xsub_sends_arbitrary_messages_to_xpub() {
        pretty_env_logger::try_init().ok();

        let mut xpub_socket = XPubSocket::new();
        let endpoint = xpub_socket
            .bind("tcp://127.0.0.1:0")
            .await
            .expect("Failed to bind");

        let mut xsub_socket = XSubSocket::new();
        xsub_socket
            .connect(&endpoint.to_string())
            .await
            .expect("Failed to connect");
        async_rt::task::sleep(Duration::from_millis(200)).await;

        xsub_socket
            .send(ZmqMessage::from("plain-message"))
            .await
            .expect("Failed to send plain message");
        let plain = async_rt::task::timeout(Duration::from_secs(2), xpub_socket.recv())
            .await
            .expect("Timeout waiting for plain message")
            .expect("Failed to receive plain message");
        assert_eq!(plain.get(0).unwrap().as_ref(), b"plain-message");

        let multipart = ZmqMessage::try_from(vec![
            Bytes::from_static(b"frame-1"),
            Bytes::from_static(b"frame-2"),
        ])
        .expect("Failed to build multipart message");
        xsub_socket
            .send(multipart.clone())
            .await
            .expect("Failed to send multipart message");
        let received = async_rt::task::timeout(Duration::from_secs(2), xpub_socket.recv())
            .await
            .expect("Timeout waiting for multipart message")
            .expect("Failed to receive multipart message");
        assert_eq!(received.len(), 2);
        assert_eq!(received.get(0).unwrap().as_ref(), b"frame-1");
        assert_eq!(received.get(1).unwrap().as_ref(), b"frame-2");
    }

    #[async_rt::test]
    async fn test_xsub_fanout_to_multiple_xpub_peers() {
        pretty_env_logger::try_init().ok();

        let mut xpub_a = XPubSocket::new();
        let mut xpub_b = XPubSocket::new();
        let endpoint_a = xpub_a.bind("tcp://127.0.0.1:0").await.expect("bind a");
        let endpoint_b = xpub_b.bind("tcp://127.0.0.1:0").await.expect("bind b");

        let mut xsub = XSubSocket::new();
        xsub.connect(&endpoint_a.to_string())
            .await
            .expect("connect a");
        xsub.connect(&endpoint_b.to_string())
            .await
            .expect("connect b");

        async_rt::task::sleep(Duration::from_millis(200)).await;
        xsub.subscribe("fanout").await.expect("subscribe failed");

        let sub_a = async_rt::task::timeout(Duration::from_secs(2), xpub_a.recv())
            .await
            .expect("Timeout waiting for subscription on a")
            .expect("Failed to receive subscription on a");
        let sub_b = async_rt::task::timeout(Duration::from_secs(2), xpub_b.recv())
            .await
            .expect("Timeout waiting for subscription on b")
            .expect("Failed to receive subscription on b");
        assert_eq!(sub_a.get(0).unwrap().as_ref(), b"\x01fanout");
        assert_eq!(sub_b.get(0).unwrap().as_ref(), b"\x01fanout");

        xsub.send(ZmqMessage::from("fanout-message"))
            .await
            .expect("Failed to fanout message");

        let msg_a = async_rt::task::timeout(Duration::from_secs(2), xpub_a.recv())
            .await
            .expect("Timeout waiting for message on a")
            .expect("Failed to receive message on a");
        let msg_b = async_rt::task::timeout(Duration::from_secs(2), xpub_b.recv())
            .await
            .expect("Timeout waiting for message on b")
            .expect("Failed to receive message on b");
        assert_eq!(msg_a.get(0).unwrap().as_ref(), b"fanout-message");
        assert_eq!(msg_b.get(0).unwrap().as_ref(), b"fanout-message");
    }

    #[async_rt::test]
    async fn test_xsub_reconnects_to_restarted_pub() {
        pretty_env_logger::try_init().ok();

        let ctx = zmq2::Context::new();
        let their_pub = ctx.socket(zmq2::PUB).expect("Couldn't make pub socket");
        their_pub.bind("tcp://127.0.0.1:0").expect("Failed to bind");
        let bind_endpoint = their_pub.get_last_endpoint().unwrap().unwrap();
        let port = extract_port(&bind_endpoint);

        let mut our_xsub = XSubSocket::new();
        our_xsub
            .connect(&bind_endpoint)
            .await
            .expect("Failed to connect");
        our_xsub.subscribe("").await.expect("Failed to subscribe");
        async_rt::task::sleep(Duration::from_millis(200)).await;

        their_pub
            .send("initial-message", 0)
            .expect("Failed to send initial");
        let initial = async_rt::task::timeout(Duration::from_secs(2), our_xsub.recv())
            .await
            .expect("Timeout waiting for initial message")
            .expect("Failed to receive initial message");
        assert_eq!(initial.get(0).unwrap().as_ref(), b"initial-message");

        drop(their_pub);
        async_rt::task::sleep(Duration::from_millis(300)).await;

        let their_pub_new = ctx.socket(zmq2::PUB).expect("Couldn't make new pub socket");
        their_pub_new
            .bind(&format!("tcp://127.0.0.1:{}", port))
            .expect("Failed to rebind");

        let _ = async_rt::task::timeout(Duration::from_millis(500), our_xsub.recv()).await;
        async_rt::task::sleep(Duration::from_millis(500)).await;

        their_pub_new
            .send("reconnected-message", 0)
            .expect("Failed to send reconnected message");
        let reconnected = async_rt::task::timeout(Duration::from_secs(3), our_xsub.recv())
            .await
            .expect("Timeout waiting for reconnected message")
            .expect("Failed to receive reconnected message");
        assert_eq!(reconnected.get(0).unwrap().as_ref(), b"reconnected-message");
    }

    #[async_rt::test]
    async fn test_xsub_resubscribes_to_restarted_xpub() {
        pretty_env_logger::try_init().ok();

        let ctx = zmq2::Context::new();
        let their_xpub = ctx.socket(zmq2::XPUB).expect("Couldn't make xpub socket");
        their_xpub
            .set_xpub_verbose(true)
            .expect("Failed to enable XPUB verbose");
        their_xpub
            .bind("tcp://127.0.0.1:0")
            .expect("Failed to bind");
        let bind_endpoint = their_xpub.get_last_endpoint().unwrap().unwrap();
        let port = extract_port(&bind_endpoint);

        let mut our_xsub = XSubSocket::new();
        our_xsub
            .connect(&bind_endpoint)
            .await
            .expect("Failed to connect");
        our_xsub
            .subscribe("topic")
            .await
            .expect("Failed to subscribe");

        let first_sub = their_xpub.recv_bytes(0).expect("Failed to recv first sub");
        assert_eq!(first_sub.as_slice(), b"\x01topic");

        drop(their_xpub);
        async_rt::task::sleep(Duration::from_millis(300)).await;

        let their_xpub_new = ctx
            .socket(zmq2::XPUB)
            .expect("Couldn't make new xpub socket");
        their_xpub_new
            .set_xpub_verbose(true)
            .expect("Failed to enable XPUB verbose");
        their_xpub_new
            .set_rcvtimeo(3000)
            .expect("Failed to set recv timeout");
        their_xpub_new
            .bind(&format!("tcp://127.0.0.1:{}", port))
            .expect("Failed to rebind");

        let _ = async_rt::task::timeout(Duration::from_millis(500), our_xsub.recv()).await;

        let resent_sub = their_xpub_new
            .recv_bytes(0)
            .expect("Failed to recv resent subscription");
        assert_eq!(resent_sub.as_slice(), b"\x01topic");

        their_xpub_new
            .send("topic-message", 0)
            .expect("Failed to send topic message");
        let msg = async_rt::task::timeout(Duration::from_secs(3), our_xsub.recv())
            .await
            .expect("Timeout waiting for topic message")
            .expect("Failed to receive topic message");
        assert_eq!(msg.get(0).unwrap().as_ref(), b"topic-message");
    }

    #[async_rt::test]
    async fn test_xsub_xpub_proxy_end_to_end() {
        pretty_env_logger::try_init().ok();

        let mut frontend = XSubSocket::new();
        let mut backend = XPubSocket::new();
        let frontend_endpoint = frontend.bind("tcp://127.0.0.1:0").await.expect("bind xsub");
        let backend_endpoint = backend.bind("tcp://127.0.0.1:0").await.expect("bind xpub");

        let proxy_fut = proxy(frontend, backend, None).fuse();
        let test_fut = async move {
            async_rt::task::sleep(Duration::from_millis(100)).await;

            let mut downstream_sub = zeromq::SubSocket::new();
            downstream_sub
                .connect(&backend_endpoint.to_string())
                .await
                .expect("Failed to connect downstream sub");
            downstream_sub
                .subscribe("topic")
                .await
                .expect("Failed to subscribe downstream");

            async_rt::task::sleep(Duration::from_millis(100)).await;

            let mut upstream_xpub = XPubSocket::new();
            upstream_xpub
                .connect(&frontend_endpoint.to_string())
                .await
                .expect("Failed to connect upstream xpub");

            let upstream_sub =
                async_rt::task::timeout(Duration::from_secs(3), upstream_xpub.recv())
                    .await
                    .expect("Timeout waiting for upstream subscription")
                    .expect("Failed to receive upstream subscription");
            assert_eq!(upstream_sub.get(0).unwrap().as_ref(), b"\x01topic");

            upstream_xpub
                .send("topic-through-proxy".into())
                .await
                .expect("Failed to send through proxy");
            let msg = async_rt::task::timeout(Duration::from_secs(3), downstream_sub.recv())
                .await
                .expect("Timeout waiting for proxy message")
                .expect("Failed to receive proxy message");
            assert_eq!(msg.get(0).unwrap().as_ref(), b"topic-through-proxy");
        }
        .fuse();

        futures::pin_mut!(proxy_fut, test_fut);
        select! {
            proxy_result = proxy_fut => panic!("proxy exited early: {:?}", proxy_result),
            _ = test_fut => {}
        }
    }
}
