use std::future::pending;
use std::time::Duration;

use crate::iface::InterfaceManager;

use super::*;

enum OpenBehavior {
    Fail,
    Block,
}

struct FailingCloseBackend {
    open_behavior: OpenBehavior,
}

impl RnodeBearerBackend for FailingCloseBackend {
    async fn open(&mut self) -> Result<RnodeBearerInfo, String> {
        match self.open_behavior {
            OpenBehavior::Fail => Err("open failed".to_string()),
            OpenBehavior::Block => pending().await,
        }
    }

    async fn read(&mut self) -> Result<Option<Vec<u8>>, String> {
        Ok(None)
    }

    async fn write(&mut self, _payload: Vec<u8>) -> Result<(), String> {
        Ok(())
    }

    async fn close(&mut self) -> Result<(), String> {
        Err("close failed".to_string())
    }
}

fn test_interface(
    open_behavior: OpenBehavior,
) -> (InterfaceContext<RnodeBearerKissInterface<FailingCloseBackend>>, RnodeBearerRuntimeStatusHandle)
{
    let interface = RnodeBearerKissInterface::new(
        "test-rnode",
        "test-endpoint",
        FailingCloseBackend { open_behavior },
        RnodeBleKissConfig::default(),
        LoraConfig::us915_default(),
    );
    let status = interface.runtime_status_handle();
    let mut manager = InterfaceManager::new(4);
    (manager.new_context(interface), status)
}

#[tokio::test]
async fn failed_startup_records_close_failure_without_losing_open_error() {
    let (context, status) = test_interface(OpenBehavior::Fail);

    RnodeBearerKissInterface::spawn(context).await;

    let snapshot = status.to_json();
    let error = snapshot["last_command_error"].as_str().expect("startup failure status");
    assert!(error.contains("open failed"));
    assert!(error.contains("iface=test-rnode"));
    assert!(error.contains("phase=startup_failure"));
    assert!(error.contains("close failed"));
}

#[tokio::test]
async fn cancelled_startup_records_close_failure() {
    let (context, status) = test_interface(OpenBehavior::Block);
    let cancel = context.cancel.clone();
    let task = tokio::spawn(RnodeBearerKissInterface::spawn(context));
    tokio::task::yield_now().await;

    cancel.cancel();
    timeout(Duration::from_secs(1), task)
        .await
        .expect("cancelled startup timeout")
        .expect("cancelled startup task");

    let snapshot = status.to_json();
    let error = snapshot["last_command_error"].as_str().expect("cancelled startup failure status");
    assert!(error.contains("iface=test-rnode"));
    assert!(error.contains("phase=startup_cancellation"));
    assert!(error.contains("close failed"));
}
