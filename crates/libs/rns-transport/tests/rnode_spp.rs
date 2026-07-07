use std::time::Duration;

use rns_transport::iface::rnode_spp::RnodeSppKissError;
use rns_transport::iface::rnode_spp::{RnodeSppBackend, RnodeSppKissConfig, RnodeSppKissRuntime};
use rns_transport::kiss::{encode_command_frame, encode_data_frame, CMD_READY, CMD_TXDELAY};

#[derive(Default)]
struct TestSppBackend {
    events: Vec<&'static str>,
    writes: Vec<Vec<u8>>,
    reads: std::collections::VecDeque<Option<Vec<u8>>>,
    connect_delay: Duration,
}

impl RnodeSppBackend for TestSppBackend {
    async fn connect(&mut self) -> Result<(), String> {
        self.events.push("connect");
        if !self.connect_delay.is_zero() {
            tokio::time::sleep(self.connect_delay).await;
        }
        Ok(())
    }

    async fn write(&mut self, payload: Vec<u8>) -> Result<(), String> {
        self.events.push("write");
        self.writes.push(payload);
        Ok(())
    }

    async fn read(&mut self) -> Result<Option<Vec<u8>>, String> {
        self.events.push("read");
        Ok(self.reads.pop_front().flatten())
    }
}

#[tokio::test]
async fn rnode_spp_startup_enforces_connect_timeout() {
    let backend = TestSppBackend { connect_delay: Duration::from_secs(1), ..Default::default() };
    let mut runtime = RnodeSppKissRuntime::new(
        backend,
        RnodeSppKissConfig { connect_timeout: Duration::from_millis(5), ..Default::default() },
    );

    let err = runtime.startup().await.expect_err("startup should timeout");

    assert!(matches!(
        err,
        RnodeSppKissError::ConnectTimeout { timeout }
            if timeout == Duration::from_millis(5)
    ));
    assert!(!runtime.status().connected);
    assert_eq!(runtime.backend().events, vec!["connect"]);
    assert!(runtime.backend().writes.is_empty());
}

#[tokio::test]
async fn rnode_spp_runtime_writes_kiss_frames_as_stream_bytes() {
    let backend = TestSppBackend::default();
    let mut runtime = RnodeSppKissRuntime::new(backend, RnodeSppKissConfig::default());

    runtime.startup().await.expect("startup");
    runtime.send_packet(&[0xAA, 0xBB]).await.expect("packet write");

    assert_eq!(
        runtime.backend().events,
        vec!["connect", "write", "write", "write", "write", "write", "write"]
    );
    assert_eq!(runtime.backend().writes[0], encode_command_frame(CMD_TXDELAY, &[35]));
    assert_eq!(runtime.backend().writes.last(), Some(&encode_data_frame(&[0xAA, 0xBB])));
}

#[tokio::test]
async fn rnode_spp_runtime_reads_stream_bytes_into_packets() {
    let backend = TestSppBackend {
        reads: vec![Some(encode_data_frame(&[0x01, 0x02]))].into(),
        ..Default::default()
    };
    let mut runtime = RnodeSppKissRuntime::new(backend, RnodeSppKissConfig::default());

    runtime.startup().await.expect("startup");
    let packets = runtime.poll_read().await.expect("read");

    assert_eq!(packets, vec![vec![0x01, 0x02]]);
}

#[tokio::test]
async fn rnode_spp_flow_control_flushes_pending_packet_on_ready() {
    let backend = TestSppBackend {
        reads: vec![Some(encode_command_frame(CMD_READY, &[1]))].into(),
        ..Default::default()
    };
    let mut runtime = RnodeSppKissRuntime::new(
        backend,
        RnodeSppKissConfig {
            kiss: rns_transport::iface::kiss::KissConfig {
                flow_control: true,
                ..Default::default()
            },
            ..Default::default()
        },
    );

    runtime.startup().await.expect("startup");
    runtime.send_packet(&[0xAA]).await.expect("queued");
    assert_ne!(runtime.backend().writes.last(), Some(&encode_data_frame(&[0xAA])));

    let packets = runtime.poll_read().await.expect("ready");

    assert!(packets.is_empty());
    assert_eq!(runtime.backend().writes.last(), Some(&encode_data_frame(&[0xAA])));
}
