use zeromq::__async_rt as async_rt;
use zeromq::prelude::*;
use zeromq::XSubSocket;

use std::time::Duration;

#[cfg(test)]
mod test {
    use super::*;

    #[async_rt::test]
    async fn test_their_pub_our_xsub() {
        pretty_env_logger::try_init().ok();

        let ctx = zmq2::Context::new();
        let their_pub = ctx.socket(zmq2::PUB).expect("Couldn't make pub socket");
        their_pub.bind("tcp://127.0.0.1:0").expect("Failed to bind");
        let endpoint = their_pub.get_last_endpoint().unwrap().unwrap();

        let mut our_xsub = XSubSocket::new();
        our_xsub
            .connect(&endpoint)
            .await
            .expect("Failed to connect");
        our_xsub
            .subscribe("topic")
            .await
            .expect("Failed to subscribe");

        async_rt::task::sleep(Duration::from_millis(200)).await;

        their_pub
            .send("other-message", 0)
            .expect("Failed to send filtered message");
        their_pub
            .send("topic-message", 0)
            .expect("Failed to send matching message");

        let msg = async_rt::task::timeout(Duration::from_secs(2), our_xsub.recv())
            .await
            .expect("Timeout waiting for message")
            .expect("Failed to receive message");
        assert_eq!(msg.get(0).unwrap().as_ref(), b"topic-message");
    }

    #[async_rt::test]
    async fn test_our_xsub_their_xpub() {
        pretty_env_logger::try_init().ok();

        let ctx = zmq2::Context::new();
        let their_xpub = ctx.socket(zmq2::XPUB).expect("Couldn't make xpub socket");
        their_xpub
            .set_xpub_verbose(true)
            .expect("Failed to enable xpub verbose");
        their_xpub
            .bind("tcp://127.0.0.1:0")
            .expect("Failed to bind");
        let endpoint = their_xpub.get_last_endpoint().unwrap().unwrap();

        let mut our_xsub = XSubSocket::new();
        our_xsub
            .connect(&endpoint)
            .await
            .expect("Failed to connect");
        our_xsub.subscribe("").await.expect("Failed to subscribe");

        let sub_msg = their_xpub
            .recv_bytes(0)
            .expect("Failed to receive subscribe");
        assert_eq!(sub_msg.as_slice(), b"\x01");

        our_xsub
            .send("hello-xpub".into())
            .await
            .expect("Failed to send to xpub");
        let app_msg = their_xpub
            .recv_bytes(0)
            .expect("Failed to receive app message");
        assert_eq!(app_msg.as_slice(), b"hello-xpub");

        their_xpub
            .send("hello-xsub", 0)
            .expect("Failed to send to xsub");
        let reply = async_rt::task::timeout(Duration::from_secs(2), our_xsub.recv())
            .await
            .expect("Timeout waiting for xpub reply")
            .expect("Failed to receive xpub reply");
        assert_eq!(reply.get(0).unwrap().as_ref(), b"hello-xsub");
    }
}
