#[derive(Default)]
struct TestBackend {
    negotiated_mtu: Option<u16>,
    notifications: VecDeque<Vec<u8>>,
}

impl RnodeBleBackend for TestBackend {
    async fn connect(&mut self) -> Result<(), String> {
        Ok(())
    }

    async fn subscribe_notifications(&mut self) -> Result<(), String> {
        Ok(())
    }

    async fn write(&mut self, _write: RnodeBleWrite) -> Result<(), String> {
        Ok(())
    }

    async fn next_notification(&mut self) -> Result<Option<Vec<u8>>, String> {
        Ok(self.notifications.pop_front())
    }

    fn negotiated_mtu(&self) -> Option<u16> {
        self.negotiated_mtu
    }
}

#[tokio::test]
async fn startup_caps_max_write_len_to_negotiated_att_payload() {
    let backend = TestBackend { negotiated_mtu: Some(23), notifications: VecDeque::new() };
    let config = RnodeBleKissConfig {
        mtu: 508,
        max_write_len: 512,
        ..RnodeBleKissConfig::default()
    };
    let mut runtime = RnodeBleKissRuntime::new(backend, config);

    runtime.startup().await.expect("startup should succeed");

    assert_eq!(
        runtime.session.config.max_write_len,
        20,
        "startup should clamp writes to the negotiated ATT payload length"
    );
}
