use crate::*;

/// Synchronous platform boundary, intended for an I/O worker, never a UI thread.
/// Methods may run concurrently: close/cancel MUST unblock in-flight operations.
/// Implementations must use native deadlines, never retry or replay writes,
/// and own cleanup even if the waiting caller abandons an operation.
pub trait GattClient: Send + Sync {
    fn capabilities(&self) -> BackendCapabilities;
    fn limits(&self) -> Limits;
    fn discover(&self, request: DiscoveryRequest) -> Result<Vec<DeviceId>>;
    /// Connection and requested subscriptions are one cancellable native operation.
    /// Connected carries the open operation_id and is emitted after setup, before
    /// notifications. Every accepted open/close emits its lifecycle event; bounded
    /// event overflow fails the stream and must not permit continued silent I/O.
    fn connect(&self, request: ConnectRequest) -> Result<SessionInfo>;
    fn characteristics(
        &self,
        session: &SessionId,
        deadline: Deadline,
    ) -> Result<Vec<CharacteristicProperties>>;
    fn subscribe(
        &self,
        session: &SessionId,
        characteristic: &CharacteristicId,
        deadline: Deadline,
    ) -> Result<()>;
    fn read(&self, request: ReadRequest) -> Result<Vec<u8>>;
    /// Completion means native transport submission, never device/application delivery.
    fn write(&self, request: WriteRequest) -> Result<usize>;
    /// None means idle. A failed/closed event stream returns an error.
    /// Consumers poll by owner so another plugin cannot drain their events.
    fn poll_event(&self, owner: &OwnerId, deadline: Deadline) -> Result<Option<BluetoothEvent>>;
    fn cancel(&self, owner: &OwnerId, operation_id: &str) -> Result<()>;
    fn close(&self, session: &SessionId) -> Result<()>;
}

/// Server requests retain remote device and characteristic identity.
/// Read responses, characteristic values and peer identification belong to the caller.
pub trait GattServer: Send + Sync {
    fn capabilities(&self) -> BackendCapabilities;
    fn start(&self, definition: ServerDefinition) -> Result<SessionId>;
    fn poll_request(&self, server: &SessionId, deadline: Deadline)
    -> Result<Option<ServerRequest>>;
    fn respond(&self, server: &SessionId, request_id: u64, value: Result<Vec<u8>>) -> Result<()>;
    fn notify(
        &self,
        server: &SessionId,
        peer: &DeviceId,
        characteristic: &CharacteristicId,
        payload: &[u8],
        deadline: Deadline,
    ) -> Result<()>;
    fn stop(&self, server: &SessionId) -> Result<()>;
}

#[derive(Clone, Debug)]
pub struct ServerRequest {
    pub request_id: u64,
    pub peer: DeviceId,
    pub characteristic: CharacteristicId,
    pub capacity: usize,
    pub kind: ServerRequestKind,
}
#[derive(Clone, Debug)]
pub enum ServerRequestKind {
    Read { offset: u16 },
    Write { offset: u16, payload: Vec<u8> },
    Subscribed,
    Disconnected,
}

pub trait ClassicStream: Send + Sync {
    fn open(
        &self,
        owner: OwnerId,
        device: DeviceId,
        service: &str,
        deadline: Deadline,
    ) -> Result<SessionInfo>;
    /// None is bounded idle; EOF is ErrorKind::Closed, never idle.
    fn read(
        &self,
        session: &SessionId,
        max_bytes: usize,
        deadline: Deadline,
    ) -> Result<Option<Vec<u8>>>;
    fn write(&self, session: &SessionId, payload: &[u8], deadline: Deadline) -> Result<usize>;
    fn close(&self, session: &SessionId) -> Result<()>;
}
