//! Tests for error handling paths that return errors instead of panicking.

#[cfg(test)]
mod test {
    use zeromq::__async_rt as async_rt;
    use zeromq::prelude::*;
    use zeromq::ZmqMessage;

    use std::time::Duration;

    #[async_rt::test]
    async fn test_router_send_requires_identity_frame() {
        pretty_env_logger::try_init().ok();

        let mut router = zeromq::RouterSocket::new();
        let endpoint = router.bind("tcp://localhost:0").await.unwrap();

        let mut dealer = zeromq::DealerSocket::new();
        dealer.connect(endpoint.to_string().as_str()).await.unwrap();

        async_rt::task::sleep(Duration::from_millis(100)).await;

        // Sending a single-frame message should fail (needs identity + payload)
        let result = router.send(ZmqMessage::from("just payload")).await;
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(
            err.to_string().contains("at least 2 frames"),
            "Expected error about 2 frames, got: {}",
            err
        );
    }

    #[async_rt::test]
    async fn test_router_split_send_requires_identity_frame() {
        pretty_env_logger::try_init().ok();

        let mut router = zeromq::RouterSocket::new();
        let endpoint = router.bind("tcp://localhost:0").await.unwrap();

        let mut dealer = zeromq::DealerSocket::new();
        dealer.connect(endpoint.to_string().as_str()).await.unwrap();

        async_rt::task::sleep(Duration::from_millis(100)).await;

        let (mut send_half, _recv_half) = router.split();

        // Sending a single-frame message should fail
        let result = send_half.send(ZmqMessage::from("just payload")).await;
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(
            err.to_string().contains("at least 2 frames"),
            "Expected error about 2 frames, got: {}",
            err
        );
    }
}
