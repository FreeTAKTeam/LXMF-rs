#![cfg(feature = "mobile-ble-adapter")]
use lxmf_sdk::{
    validate_mobile_ble_event_payload_bounds as validate_event_payload_bounds,
    validate_mobile_ble_event_sequence as validate_event_sequence, MobileBleConnectRequest,
    MobileBleEventKind, MobileBleHostAdapter, MobileBleWriteRequest, SharedMobileBleAdapter,
};
use rns_bluetooth::*;
use std::sync::{Arc, Mutex};

struct Backend {
    queue: EventQueue,
    registry: SessionRegistry,
    write: Mutex<Option<WriteRequest>>,
    connect: Mutex<Option<ConnectRequest>>,
    pause: Mutex<Option<Arc<std::sync::Barrier>>>,
}
impl Backend {
    fn new() -> Arc<Self> {
        Arc::new(Self {
            queue: EventQueue::new(OwnerId("sdk".into()), limits())
                .expect("synthetic adapter fixture"),
            registry: SessionRegistry::new(2).expect("synthetic adapter fixture"),
            write: Mutex::new(None),
            connect: Mutex::new(None),
            pause: Mutex::new(None),
        })
    }
}
fn limits() -> Limits {
    Limits { max_sessions: 2, max_events: 16, max_payload_bytes: 200 }
}
fn unsupported<T>() -> Result<T> {
    Err(BluetoothError::new(ErrorKind::Unsupported, "synthetic"))
}
impl GattClient for Backend {
    fn capabilities(&self) -> BackendCapabilities {
        BackendCapabilities {
            gatt_client: true,
            write_without_response: true,
            ..Default::default()
        }
    }
    fn limits(&self) -> Limits {
        limits()
    }
    fn discover(&self, _: DiscoveryRequest) -> Result<Vec<DeviceId>> {
        unsupported()
    }
    fn connect(&self, req: ConnectRequest) -> Result<SessionInfo> {
        let id = self.registry.reserve(req.owner.clone(), req.device.clone())?;
        self.queue.push(id.clone(), Some(req.operation_id.clone()), EventKind::Connected)?;
        let info = SessionInfo {
            id,
            device: req.device.clone(),
            att_mtu: Some(247),
            max_write_bytes: 200,
            max_notification_bytes: 200,
        };
        for channel in &req.subscriptions {
            self.subscribe(&info.id, channel, req.deadline)?;
        }
        let pause = self.pause.lock().expect("pause").clone();
        if let Some(pause) = pause {
            pause.wait();
            pause.wait();
        }
        *self.connect.lock().expect("synthetic adapter fixture") = Some(req);
        Ok(info)
    }
    fn characteristics(&self, _: &SessionId, _: Deadline) -> Result<Vec<CharacteristicProperties>> {
        unsupported()
    }
    fn subscribe(
        &self,
        session: &SessionId,
        characteristic: &CharacteristicId,
        _: Deadline,
    ) -> Result<()> {
        self.registry.device(session)?;
        self.queue.push(
            session.clone(),
            None,
            EventKind::Notification { characteristic: characteristic.clone(), payload: vec![42] },
        )
    }
    fn read(&self, request: ReadRequest) -> Result<Vec<u8>> {
        self.registry.device(&request.session)?;
        Ok(vec![7])
    }
    fn write(&self, req: WriteRequest) -> Result<usize> {
        self.registry.device(&req.session)?;
        let size = req.payload.len();
        self.queue.push(
            req.session.clone(),
            Some(req.operation_id.clone()),
            EventKind::WriteComplete { bytes_written: size },
        )?;
        *self.write.lock().expect("synthetic adapter fixture") = Some(req);
        Ok(size)
    }
    fn poll_event(&self, _: &OwnerId, deadline: Deadline) -> Result<Option<BluetoothEvent>> {
        self.queue.poll(deadline)
    }
    fn cancel(&self, _: &OwnerId, _: &str) -> Result<()> {
        unsupported()
    }
    fn close(&self, session: &SessionId) -> Result<()> {
        if self.registry.release(session)? {
            self.queue.push(session.clone(), None, EventKind::Disconnected)?;
        }
        Ok(())
    }
}
fn request() -> MobileBleConnectRequest {
    MobileBleConnectRequest {
        operation_id: "open-1".into(),
        peripheral_id: "synthetic".into(),
        service_uuid: "service".into(),
        write_char_uuid: "write".into(),
        notify_char_uuid: "notify".into(),
        connect_timeout_ms: 100,
    }
}
#[test]
fn shared_adapter_preserves_existing_envelope_and_selected_channels() {
    let backend = Backend::new();
    let adapter = SharedMobileBleAdapter::new(backend.clone(), OwnerId("sdk".into()))
        .expect("synthetic adapter fixture");
    let session = adapter.connect(request()).expect("synthetic adapter fixture");
    let ack = adapter
        .write(MobileBleWriteRequest {
            operation_id: "write-1".into(),
            session_id: session.session_id.clone(),
            payload: vec![1, 2],
            require_response: false,
            write_timeout_ms: 100,
        })
        .expect("synthetic adapter fixture");
    assert_eq!(ack.bytes_written, 2);
    assert_eq!(ack.operation_id, "write-1");
    let written = backend
        .write
        .lock()
        .expect("synthetic adapter fixture")
        .clone()
        .expect("synthetic adapter fixture");
    assert_eq!(written.characteristic.characteristic, "write");
    assert_eq!(written.mode, WriteMode::WithoutResponse);
    adapter.disconnect(&session.session_id).expect("synthetic adapter fixture");
    let events = (0..4)
        .map(|_| {
            adapter
                .poll_event(100)
                .expect("synthetic adapter fixture")
                .expect("synthetic adapter fixture")
        })
        .collect::<Vec<_>>();
    validate_event_sequence(&events).expect("synthetic adapter fixture");
    validate_event_payload_bounds(&events, &adapter.capabilities())
        .expect("synthetic adapter fixture");
    assert_eq!(events[1].payload, Some(vec![42]));
    assert_eq!(events[2].operation_id.as_deref(), Some("write-1"));
    let json = serde_json::to_value(&events[1]).expect("synthetic adapter fixture");
    assert_eq!(json.as_object().expect("synthetic adapter fixture").len(), 6);
    assert_eq!(events[3].kind, MobileBleEventKind::Disconnected);
}
#[test]
fn bounds_and_cancellation_capability_are_checked_before_native_io() {
    let backend = Backend::new();
    let adapter = SharedMobileBleAdapter::new(backend.clone(), OwnerId("sdk".into()))
        .expect("synthetic adapter fixture");
    let session = adapter.connect(request()).expect("synthetic adapter fixture");
    assert!(adapter.cancel_operation("open-1").is_err());
    assert!(adapter
        .write(MobileBleWriteRequest {
            operation_id: "w".into(),
            session_id: session.session_id,
            payload: vec![0; 201],
            require_response: true,
            write_timeout_ms: 100
        })
        .is_err());
    assert!(backend.write.lock().expect("synthetic adapter fixture").is_none());
}

#[test]
fn reconnect_admission_does_not_depend_on_polling_disconnected_events() {
    let backend = Backend::new();
    let adapter = SharedMobileBleAdapter::new(backend, OwnerId("sdk".into())).expect("adapter");
    for _ in 0..3 {
        let session = adapter.connect(request()).expect("backend has capacity");
        adapter.disconnect(&session.session_id).expect("disconnect");
    }
    let events =
        (0..9).map(|_| adapter.poll_event(100).expect("poll").expect("event")).collect::<Vec<_>>();
    validate_event_sequence(&events).expect("preserved session history");
}

#[test]
fn slow_open_does_not_block_existing_session_io_or_event_projection() {
    let backend = Backend::new();
    let adapter = Arc::new(
        SharedMobileBleAdapter::new(backend.clone(), OwnerId("sdk".into())).expect("adapter"),
    );
    let first = adapter.connect(request()).expect("first");
    let pause = Arc::new(std::sync::Barrier::new(2));
    *backend.pause.lock().expect("pause") = Some(pause.clone());
    let second = {
        let adapter = adapter.clone();
        std::thread::spawn(move || {
            let mut req = request();
            req.operation_id = "open-2".into();
            req.peripheral_id = "synthetic-2".into();
            adapter.connect(req).map_err(|error| error.to_string())
        })
    };
    pause.wait();
    let (tx, rx) = std::sync::mpsc::channel();
    let traffic = {
        let adapter = adapter.clone();
        std::thread::spawn(move || {
            let result = adapter.write(MobileBleWriteRequest {
                operation_id: "w".into(),
                session_id: first.session_id,
                payload: vec![1],
                require_response: true,
                write_timeout_ms: 100,
            });
            let events = (0..5)
                .map(|_| adapter.poll_event(10).map_err(|error| error.to_string()))
                .collect::<Vec<_>>();
            tx.send((result, events)).expect("result");
        })
    };
    let observed = rx.recv_timeout(std::time::Duration::from_secs(1));
    pause.wait();
    second.join().expect("thread").expect("second open");
    traffic.join().expect("traffic");
    let (result, events) = observed.expect("traffic must progress while open is blocked");
    result.expect("existing-session write");
    let events = events.into_iter().map(|r| r.expect("poll").expect("event")).collect::<Vec<_>>();
    validate_event_sequence(&events).expect("connect-before-notification preserved during open");
}
