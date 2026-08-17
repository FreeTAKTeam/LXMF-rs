//! Bearer-neutral RNode transport contract.
//!
//! Platform owners implement raw, ordered byte I/O here. RNode probing,
//! configuration, KISS framing, MTU validation, and flow control remain in
//! this crate's shared protocol runtime.

use super::rnode_ble::{
    RnodeBleBackend, RnodeBleKissConfig, RnodeBleKissError, RnodeBleKissRuntime,
    RnodeBleKissStatus, RnodeBleNotification, RnodeBleWrite,
};

pub use super::rnode_bearer_interface::{RnodeBearerKissInterface, RnodeBearerRuntimeStatusHandle};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RnodeBearerKind {
    Ble,
    BluetoothClassic,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RnodeBearerInfo {
    pub kind: RnodeBearerKind,
    pub negotiated_mtu: Option<u16>,
}

/// Cancellation-safe, single-attempt raw RNode bearer.
///
/// Dropping an in-flight operation must not prevent a subsequent `close` from
/// releasing the native resource. `close` must be idempotent and must unblock
/// any pending read or write. Retry and backoff belong to the caller.
#[allow(async_fn_in_trait)]
pub trait RnodeBearerBackend {
    async fn open(&mut self) -> Result<RnodeBearerInfo, String>;

    /// Read the next available chunk.
    ///
    /// `Ok(None)` means no bytes are currently available. A closed or failed
    /// transport must return `Err` so the single-attempt interface can stop.
    async fn read(&mut self) -> Result<Option<Vec<u8>>, String>;

    async fn write(&mut self, payload: Vec<u8>) -> Result<(), String>;

    async fn close(&mut self) -> Result<(), String>;
}

struct RnodeBearerAdapter<B> {
    backend: B,
    info: Option<RnodeBearerInfo>,
}

impl<B> RnodeBearerAdapter<B> {
    fn new(backend: B) -> Self {
        Self { backend, info: None }
    }
}

impl<B> RnodeBleBackend for RnodeBearerAdapter<B>
where
    B: RnodeBearerBackend,
{
    async fn connect(&mut self) -> Result<(), String> {
        self.info = Some(self.backend.open().await?);
        Ok(())
    }

    async fn subscribe_notifications(&mut self) -> Result<(), String> {
        // The platform backend returns from open only after its byte stream is ready.
        Ok(())
    }

    async fn write(&mut self, write: RnodeBleWrite) -> Result<(), String> {
        self.backend.write(write.payload).await
    }

    async fn next_notification(&mut self) -> Result<Option<Vec<u8>>, String> {
        self.backend.read().await
    }

    async fn close(&mut self) -> Result<(), String> {
        let result = self.backend.close().await;
        self.info = None;
        result
    }

    fn negotiated_mtu(&self) -> Option<u16> {
        self.info.and_then(|info| info.negotiated_mtu)
    }
}

/// Shared KISS/RNode runtime for BLE and Bluetooth Classic bearers.
pub struct RnodeBearerKissRuntime<B> {
    inner: RnodeBleKissRuntime<RnodeBearerAdapter<B>>,
}

impl<B> RnodeBearerKissRuntime<B>
where
    B: RnodeBearerBackend,
{
    #[must_use]
    pub fn new(backend: B, config: RnodeBleKissConfig) -> Self {
        Self { inner: RnodeBleKissRuntime::new(RnodeBearerAdapter::new(backend), config) }
    }

    pub async fn startup(&mut self) -> Result<RnodeBearerInfo, RnodeBleKissError> {
        self.inner.startup().await?;
        self.inner.backend().info.ok_or_else(|| RnodeBleKissError::Backend {
            operation: "open",
            message: "bearer opened without connection information".to_string(),
        })
    }

    pub async fn send_packet(&mut self, payload: &[u8]) -> Result<(), RnodeBleKissError> {
        self.inner.send_packet(payload).await
    }

    pub async fn send_deferred_frames(&mut self) -> Result<(), RnodeBleKissError> {
        self.inner.send_deferred_frames().await
    }

    pub async fn send_id_beacon(&mut self) -> Result<(), RnodeBleKissError> {
        self.inner.send_id_beacon().await
    }

    pub async fn send_management_frame(&mut self, frame: Vec<u8>) -> Result<(), RnodeBleKissError> {
        self.inner.send_management_frame(frame).await
    }

    pub async fn poll(&mut self) -> Result<Option<RnodeBleNotification>, RnodeBleKissError> {
        self.inner.poll_optional_notification_events().await
    }

    pub async fn shutdown(&mut self) -> Result<(), RnodeBleKissError> {
        self.inner.shutdown().await
    }

    pub async fn shutdown_with_prefix_frames(
        &mut self,
        prefix_frames: Vec<Vec<u8>>,
    ) -> Result<(), RnodeBleKissError> {
        self.inner.shutdown_with_prefix_frames(prefix_frames).await
    }

    pub async fn close(&mut self) -> Result<(), RnodeBleKissError> {
        self.inner.close().await
    }

    #[must_use]
    pub fn status(&self) -> RnodeBleKissStatus {
        self.inner.status()
    }

    #[must_use]
    pub fn negotiated_mtu(&self) -> Option<u16> {
        self.inner.negotiated_mtu()
    }

    #[must_use]
    pub fn into_backend(self) -> B {
        self.inner.into_backend().backend
    }
}

#[cfg(test)]
mod tests {
    use std::collections::VecDeque;
    use std::future::pending;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Arc;
    use std::time::Duration;

    use super::*;
    use crate::kiss::{encode_data_frame, KissFrame};

    #[derive(Default)]
    struct TestBackend {
        opens: usize,
        closes: usize,
        reads: VecDeque<Vec<u8>>,
        writes: Vec<Vec<u8>>,
    }

    impl RnodeBearerBackend for TestBackend {
        async fn open(&mut self) -> Result<RnodeBearerInfo, String> {
            self.opens += 1;
            Ok(RnodeBearerInfo {
                kind: RnodeBearerKind::BluetoothClassic,
                negotiated_mtu: Some(64),
            })
        }

        async fn read(&mut self) -> Result<Option<Vec<u8>>, String> {
            Ok(self.reads.pop_front())
        }

        async fn write(&mut self, payload: Vec<u8>) -> Result<(), String> {
            self.writes.push(payload);
            Ok(())
        }

        async fn close(&mut self) -> Result<(), String> {
            self.closes += 1;
            Ok(())
        }
    }

    #[tokio::test]
    async fn both_bearers_use_shared_kiss_runtime_and_close() {
        let mut backend = TestBackend::default();
        backend.reads.push_back(encode_data_frame(b"hello"));
        let mut runtime = RnodeBearerKissRuntime::new(backend, RnodeBleKissConfig::default());

        let info = runtime.startup().await.expect("startup");
        assert_eq!(info.kind, RnodeBearerKind::BluetoothClassic);
        assert_eq!(runtime.negotiated_mtu(), Some(64));
        let notification = runtime.poll().await.expect("poll").expect("notification");
        assert_eq!(notification.packets, vec![b"hello".to_vec()]);
        runtime.send_packet(b"world").await.expect("send packet");
        runtime.shutdown().await.expect("shutdown");

        let backend = runtime.into_backend();
        assert_eq!(backend.opens, 1);
        assert_eq!(backend.closes, 1);
        assert!(!backend.writes.is_empty());
        let decoded = crate::kiss::decode_frames(
            &backend.writes.into_iter().flatten().collect::<Vec<_>>(),
            508,
        )
        .expect("decode writes");
        assert!(decoded.contains(&KissFrame::Data(b"world".to_vec())));
    }

    #[tokio::test]
    async fn close_is_attempted_when_shutdown_write_fails() {
        struct FailingBackend {
            closes: usize,
        }

        impl RnodeBearerBackend for FailingBackend {
            async fn open(&mut self) -> Result<RnodeBearerInfo, String> {
                Ok(RnodeBearerInfo { kind: RnodeBearerKind::Ble, negotiated_mtu: None })
            }

            async fn read(&mut self) -> Result<Option<Vec<u8>>, String> {
                Ok(None)
            }

            async fn write(&mut self, _payload: Vec<u8>) -> Result<(), String> {
                Err("write failed".to_string())
            }

            async fn close(&mut self) -> Result<(), String> {
                self.closes += 1;
                Ok(())
            }
        }

        let config = RnodeBleKissConfig {
            shutdown_frames: vec![vec![0xc0, 0xff, 0xc0]],
            ..RnodeBleKissConfig::default()
        };
        let mut runtime = RnodeBearerKissRuntime::new(FailingBackend { closes: 0 }, config);
        let error = runtime.shutdown().await.expect_err("shutdown write should fail");
        assert!(matches!(error, RnodeBleKissError::Backend { operation: "shutdown_write", .. }));
        assert_eq!(runtime.into_backend().closes, 1);
    }

    #[tokio::test]
    async fn empty_bearer_reads_remain_distinct_from_empty_notifications() {
        let backend = TestBackend::default();
        let mut runtime = RnodeBearerKissRuntime::new(backend, RnodeBleKissConfig::default());

        runtime.startup().await.expect("startup");

        assert_eq!(runtime.poll().await.expect("empty poll"), None);
    }

    #[derive(Clone, Copy)]
    enum BlockOperation {
        Open,
        Read,
        Write,
    }

    struct BlockingBackend {
        operation: BlockOperation,
        closed: Arc<AtomicBool>,
        block_write: Arc<AtomicBool>,
    }

    impl RnodeBearerBackend for BlockingBackend {
        async fn open(&mut self) -> Result<RnodeBearerInfo, String> {
            if matches!(self.operation, BlockOperation::Open) {
                pending::<()>().await;
            }
            Ok(RnodeBearerInfo { kind: RnodeBearerKind::Ble, negotiated_mtu: Some(185) })
        }

        async fn read(&mut self) -> Result<Option<Vec<u8>>, String> {
            if matches!(self.operation, BlockOperation::Read) {
                pending::<()>().await;
            }
            Ok(None)
        }

        async fn write(&mut self, _payload: Vec<u8>) -> Result<(), String> {
            if matches!(self.operation, BlockOperation::Write)
                && self.block_write.load(Ordering::Acquire)
            {
                pending::<()>().await;
            }
            Ok(())
        }

        async fn close(&mut self) -> Result<(), String> {
            self.closed.store(true, Ordering::Release);
            Ok(())
        }
    }

    #[tokio::test]
    async fn cancellation_during_open_still_allows_idempotent_close() {
        let closed = Arc::new(AtomicBool::new(false));
        let backend = BlockingBackend {
            operation: BlockOperation::Open,
            closed: closed.clone(),
            block_write: Arc::new(AtomicBool::new(false)),
        };
        let mut runtime = RnodeBearerKissRuntime::new(backend, RnodeBleKissConfig::default());

        let completed = tokio::select! {
            _ = runtime.startup() => true,
            () = tokio::time::sleep(Duration::from_millis(5)) => false,
        };
        assert!(!completed, "open unexpectedly completed");
        runtime.close().await.expect("close after cancelled open");
        runtime.close().await.expect("second close is safe");

        assert!(closed.load(Ordering::Acquire));
    }

    #[tokio::test]
    async fn cancellation_during_read_still_allows_close() {
        let closed = Arc::new(AtomicBool::new(false));
        let backend = BlockingBackend {
            operation: BlockOperation::Read,
            closed: closed.clone(),
            block_write: Arc::new(AtomicBool::new(false)),
        };
        let mut runtime = RnodeBearerKissRuntime::new(backend, RnodeBleKissConfig::default());
        runtime.startup().await.expect("startup");

        let completed = tokio::select! {
            _ = runtime.poll() => true,
            () = tokio::time::sleep(Duration::from_millis(5)) => false,
        };
        assert!(!completed, "read unexpectedly completed");
        runtime.close().await.expect("close after cancelled read");

        assert!(closed.load(Ordering::Acquire));
    }

    #[tokio::test]
    async fn cancellation_during_write_still_allows_close() {
        let closed = Arc::new(AtomicBool::new(false));
        let block_write = Arc::new(AtomicBool::new(false));
        let backend = BlockingBackend {
            operation: BlockOperation::Write,
            closed: closed.clone(),
            block_write: block_write.clone(),
        };
        let mut runtime = RnodeBearerKissRuntime::new(backend, RnodeBleKissConfig::default());
        runtime.startup().await.expect("startup");
        block_write.store(true, Ordering::Release);

        let completed = tokio::select! {
            _ = runtime.send_packet(b"blocked") => true,
            () = tokio::time::sleep(Duration::from_millis(5)) => false,
        };
        assert!(!completed, "write unexpectedly completed");
        runtime.close().await.expect("close after cancelled write");

        assert!(closed.load(Ordering::Acquire));
    }
}
