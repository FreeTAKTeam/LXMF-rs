struct EofIdleBackend {
    stream_ended: bool,
    reads: VecDeque<FakeBleRead>,
}

enum FakeBleRead {
    Idle,
    Data(Vec<u8>),
    Eof,
}

impl RnodeBleBackend for EofIdleBackend {
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
        match self.reads.pop_front().expect("scripted BLE read") {
            FakeBleRead::Idle => Ok(None),
            FakeBleRead::Data(data) => Ok(Some(data)),
            FakeBleRead::Eof => {
                self.stream_ended = true;
                Ok(None)
            }
        }
    }

    fn notification_stream_ends_on_none(&self) -> bool {
        self.stream_ended
    }
}

#[tokio::test]
async fn native_stream_eof_is_distinct_from_idle_and_idle_read_recovers() {
    let backend = EofIdleBackend {
        stream_ended: false,
        reads: VecDeque::from([
            FakeBleRead::Idle,
            FakeBleRead::Data(encode_data_frame(b"after-idle")),
            FakeBleRead::Eof,
        ]),
    };
    let mut runtime = RnodeBleKissRuntime::new(backend, RnodeBleKissConfig::default());
    runtime.startup().await.expect("backend startup");

    let idle = runtime
        .poll_notification_events()
        .await
        .expect("idle timeout is not a stream failure");
    assert!(idle.packets.is_empty());
    assert!(runtime.status().connected, "an idle read preserves the live session");

    let resumed = runtime
        .poll_notification_events()
        .await
        .expect("notification after idle");
    assert_eq!(resumed.packets, vec![b"after-idle".to_vec()]);
    assert!(runtime.status().connected);

    let error = runtime
        .poll_notification_events()
        .await
        .expect_err("native stream EOF must be visible to the caller");
    assert!(matches!(
        error,
        RnodeBleKissError::Backend { operation: "next_notification", .. }
    ));
    assert!(!runtime.status().connected, "EOF resets the active session");
}
