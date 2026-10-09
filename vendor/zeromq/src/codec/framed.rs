use crate::codec::ZmqCodec;

use asynchronous_codec::{FramedRead, FramedWrite};
use futures::{AsyncRead, AsyncWrite};

// Enables us to have multiple bounds on the dyn trait in `InnerFramed`
pub(crate) struct ConnectionLease;
static CONNECTIONS: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
impl ConnectionLease {
    pub(crate) fn acquire() -> Option<Self> {
        CONNECTIONS
            .fetch_update(
                std::sync::atomic::Ordering::AcqRel,
                std::sync::atomic::Ordering::Acquire,
                |n| (n < 64).then_some(n + 1),
            )
            .ok()
            .map(|_| Self)
    }
}
impl Drop for ConnectionLease {
    fn drop(&mut self) {
        CONNECTIONS.fetch_sub(1, std::sync::atomic::Ordering::AcqRel);
    }
}
struct LeasedRead {
    inner: Box<dyn FrameableRead>,
    _lease: std::sync::Arc<ConnectionLease>,
}
struct LeasedWrite {
    inner: Box<dyn FrameableWrite>,
    _lease: std::sync::Arc<ConnectionLease>,
}
impl AsyncWrite for LeasedWrite {
    fn poll_write(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buffer: &[u8],
    ) -> std::task::Poll<std::io::Result<usize>> {
        std::pin::Pin::new(&mut self.inner).poll_write(cx, buffer)
    }
    fn poll_flush(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        std::pin::Pin::new(&mut self.inner).poll_flush(cx)
    }
    fn poll_close(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        std::pin::Pin::new(&mut self.inner).poll_close(cx)
    }
}
impl AsyncRead for LeasedRead {
    fn poll_read(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buffer: &mut [u8],
    ) -> std::task::Poll<std::io::Result<usize>> {
        std::pin::Pin::new(&mut self.inner).poll_read(cx, buffer)
    }
}
pub trait FrameableRead: AsyncRead + Unpin + Send + Sync {}
impl<T> FrameableRead for T where T: AsyncRead + Unpin + Send + Sync {}
pub trait FrameableWrite: AsyncWrite + Unpin + Send + Sync {}
impl<T> FrameableWrite for T where T: AsyncWrite + Unpin + Send + Sync {}

pub(crate) type ZmqFramedRead = asynchronous_codec::FramedRead<Box<dyn FrameableRead>, ZmqCodec>;
pub(crate) type ZmqFramedWrite = asynchronous_codec::FramedWrite<Box<dyn FrameableWrite>, ZmqCodec>;

/// Equivalent to [`asynchronous_codec::Framed<T, ZmqCodec>`]
pub struct FramedIo {
    pub read_half: ZmqFramedRead,
    pub write_half: ZmqFramedWrite,
}

impl FramedIo {
    pub fn new(read_half: Box<dyn FrameableRead>, write_half: Box<dyn FrameableWrite>) -> Self {
        let read_half = FramedRead::new(read_half, ZmqCodec::new());
        let write_half = FramedWrite::new(write_half, ZmqCodec::new());
        Self {
            read_half,
            write_half,
        }
    }

    pub(crate) fn with_lease(
        read: Box<dyn FrameableRead>,
        write: Box<dyn FrameableWrite>,
        lease: ConnectionLease,
    ) -> Self {
        let lease = std::sync::Arc::new(lease);
        Self::new(
            Box::new(LeasedRead {
                inner: read,
                _lease: lease.clone(),
            }),
            Box::new(LeasedWrite {
                inner: write,
                _lease: lease,
            }),
        )
    }
    pub fn into_parts(self) -> (ZmqFramedRead, ZmqFramedWrite) {
        (self.read_half, self.write_half)
    }
}
