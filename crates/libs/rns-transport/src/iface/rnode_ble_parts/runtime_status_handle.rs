#[derive(Clone)]
pub struct RnodeBleRuntimeStatusHandle {
    inner: Arc<Mutex<serde_json::Value>>,
}

impl RnodeBleRuntimeStatusHandle {
    #[must_use]
    pub fn new(inner: Arc<Mutex<serde_json::Value>>) -> Self {
        Self { inner }
    }

    #[must_use]
    pub fn to_json(&self) -> serde_json::Value {
        self.inner.lock().expect("RNode BLE status mutex poisoned").clone()
    }

    pub(crate) fn set_worker_error(&self, error: &str) {
        if let Some(status) = self.inner.lock().expect("RNode BLE status mutex poisoned").as_object_mut() {
            status.insert("worker_error".to_string(), serde_json::Value::String(error.to_string()));
        }
    }

    pub(crate) fn update_runtime_status(&self, mut value: serde_json::Value) {
        let mut current = self.inner.lock().expect("RNode BLE status mutex poisoned");
        if let (Some(current), Some(next)) = (current.as_object(), value.as_object_mut()) {
            if let Some(error) = current.get("worker_error") {
                next.insert("worker_error".to_string(), error.clone());
            }
        }
        *current = value;
    }
}
