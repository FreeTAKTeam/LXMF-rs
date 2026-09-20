use rns_transport::iface::{
    rnode_bearer::{RnodeBearerBackend, RnodeBearerInfo, RnodeBearerKind, RnodeBearerKissRuntime},
    rnode_ble::RnodeBleKissConfig,
};
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc,
};

struct IdleBearer {
    closes: Arc<AtomicUsize>,
}

impl RnodeBearerBackend for IdleBearer {
    async fn open(&mut self) -> Result<RnodeBearerInfo, String> {
        Ok(RnodeBearerInfo { kind: RnodeBearerKind::BluetoothClassic, negotiated_mtu: None })
    }
    async fn read(&mut self) -> Result<Option<Vec<u8>>, String> {
        Ok(None)
    }
    async fn write(&mut self, _payload: Vec<u8>) -> Result<(), String> {
        Ok(())
    }
    async fn close(&mut self) -> Result<(), String> {
        self.closes.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }
}

#[tokio::test]
async fn rns_1_5_4_bearer_idle_read_preserves_the_mobile_contract() {
    let closes = Arc::new(AtomicUsize::new(0));
    let backend = IdleBearer { closes: closes.clone() };
    let mut runtime = RnodeBearerKissRuntime::new(backend, RnodeBleKissConfig::default());
    runtime.startup().await.expect("startup");
    assert!(runtime.poll().await.expect("idle read").is_none());
    assert!(runtime.status().connected, "bearer None means idle, not BLE stream EOF");
    assert!(runtime.status().subscribed);
    runtime.close().await.expect("close");
    runtime.close().await.expect("idempotent close");
    assert_eq!(closes.load(Ordering::SeqCst), 1);
}
