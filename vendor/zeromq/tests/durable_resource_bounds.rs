#![cfg(feature = "tokio-runtime")]
use tokio::io::AsyncWriteExt;
use tokio::time::{timeout, Duration};
use zeromq::{PullSocket, PushSocket, Socket, SocketOptions, SocketRecv, SocketSend};

#[tokio::test]
async fn malformed_handshake_and_connection_churn_leave_healthy_client_working() {
    let mut receiver = PullSocket::new();
    let endpoint = receiver
        .bind("tcp://127.0.0.1:0")
        .await
        .unwrap()
        .to_string();
    let address = endpoint.strip_prefix("tcp://").unwrap();
    for _ in 0..100 {
        let mut malformed = tokio::net::TcpStream::connect(address).await.unwrap();
        malformed.write_all(&[0; 64]).await.unwrap();
        drop(malformed);
        // Keep this deterministic and allow the accept owner to release each failed handshake.
        tokio::task::yield_now().await;
    }
    let mut healthy = PushSocket::new();
    timeout(Duration::from_secs(6), healthy.connect(&endpoint))
        .await
        .unwrap()
        .unwrap();
    healthy.send("Test1234".into()).await.unwrap();
    let message = timeout(Duration::from_secs(3), receiver.recv())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(message.get(0).unwrap().as_ref(), b"Test1234");
}
#[tokio::test]
async fn replacing_the_same_peer_identity_does_not_disconnect_the_new_stream() {
    let mut receiver = PullSocket::new();
    let endpoint = receiver
        .bind("tcp://127.0.0.1:0")
        .await
        .unwrap()
        .to_string();
    let mut options = SocketOptions::default();
    options.peer_identity(vec![1, 2, 3].try_into().unwrap());
    let mut first = PushSocket::with_options(options);
    first.connect(&endpoint).await.unwrap();
    first.send("before".into()).await.unwrap();
    timeout(Duration::from_secs(2), receiver.recv())
        .await
        .unwrap()
        .unwrap();
    let mut options = SocketOptions::default();
    options.peer_identity(vec![1, 2, 3].try_into().unwrap());
    let mut second = PushSocket::with_options(options);
    second.connect(&endpoint).await.unwrap();
    drop(first);
    for _ in 0..50 {
        second.send("after".into()).await.unwrap();
        let message = timeout(Duration::from_secs(2), receiver.recv())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(message.get(0).unwrap().as_ref(), b"after");
    }
}
