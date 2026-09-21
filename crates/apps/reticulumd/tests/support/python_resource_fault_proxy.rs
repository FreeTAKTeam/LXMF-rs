use rns_transport::buffer::OutputBuffer;
use rns_transport::iface::hdlc::Hdlc;
use rns_transport::{Packet, PacketContext};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::task::JoinHandle;

#[derive(Clone, Copy, Debug)]
pub(super) enum ResourceFaultMode {
    DropFirst,
    DuplicateFirst,
    ReorderFirstTwo,
    DropAll,
    DropKeepAlive,
}

pub(super) struct PythonResourceFaultProxy {
    port: u16,
    task: JoinHandle<()>,
}

impl PythonResourceFaultProxy {
    pub(super) async fn bind(target_port: u16, mode: ResourceFaultMode) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind resource fault proxy");
        let port = listener.local_addr().expect("resource fault proxy address").port();
        let task = tokio::spawn(async move {
            let Ok((incoming, _)) = listener.accept().await else {
                return;
            };
            let Ok(outgoing) = TcpStream::connect(("127.0.0.1", target_port)).await else {
                return;
            };
            let (incoming_read, incoming_write) = incoming.into_split();
            let (outgoing_read, outgoing_write) = outgoing.into_split();
            let forward_to_target = forward_frames(incoming_read, outgoing_write, Some(mode));
            let forward_to_client = forward_frames(outgoing_read, incoming_write, None);
            tokio::select! {
                _ = forward_to_target => {}
                _ = forward_to_client => {}
            }
        });
        Self { port, task }
    }

    pub(super) fn port(&self) -> u16 {
        self.port
    }
}

impl Drop for PythonResourceFaultProxy {
    fn drop(&mut self) {
        self.task.abort();
    }
}

#[derive(Default)]
struct FaultState {
    first_resource_seen: bool,
    reordered_first: Option<Vec<u8>>,
}

async fn forward_frames<R, W>(mut reader: R, mut writer: W, mode: Option<ResourceFaultMode>)
where
    R: AsyncRead + Unpin,
    W: AsyncWrite + Unpin,
{
    let mut read_buffer = [0u8; 16 * 1024];
    let mut pending = Vec::new();
    let mut state = FaultState::default();

    loop {
        let read = match reader.read(&mut read_buffer).await {
            Ok(0) | Err(_) => break,
            Ok(read) => read,
        };
        pending.extend_from_slice(&read_buffer[..read]);

        while let Some((start, end)) = Hdlc::find(&pending) {
            if start > 0 {
                pending.drain(..start);
            }
            let frame = pending.drain(..=end - start).collect::<Vec<_>>();
            if should_fault(&frame, mode) {
                match mode.expect("fault mode present") {
                    ResourceFaultMode::DropFirst => {
                        if !state.first_resource_seen {
                            state.first_resource_seen = true;
                            continue;
                        }
                        write_frame(&mut writer, &frame).await;
                    }
                    ResourceFaultMode::DuplicateFirst => {
                        if !state.first_resource_seen {
                            state.first_resource_seen = true;
                            write_frame(&mut writer, &frame).await;
                            write_frame(&mut writer, &frame).await;
                        } else {
                            write_frame(&mut writer, &frame).await;
                        }
                    }
                    ResourceFaultMode::ReorderFirstTwo => {
                        if state.reordered_first.is_none() {
                            state.reordered_first = Some(frame);
                        } else if let Some(first) = state.reordered_first.take() {
                            write_frame(&mut writer, &frame).await;
                            write_frame(&mut writer, &first).await;
                            state.first_resource_seen = true;
                        }
                    }
                    ResourceFaultMode::DropAll => {}
                    ResourceFaultMode::DropKeepAlive => {}
                }
            } else {
                write_frame(&mut writer, &frame).await;
            }
        }
    }

    if let Some(frame) = state.reordered_first.take() {
        write_frame(&mut writer, &frame).await;
    }
}

async fn write_frame<W>(writer: &mut W, frame: &[u8])
where
    W: AsyncWrite + Unpin,
{
    let _ = writer.write_all(frame).await;
}

fn should_fault(frame: &[u8], mode: Option<ResourceFaultMode>) -> bool {
    if mode.is_none() {
        return false;
    }
    let mut decoded = vec![0u8; frame.len()];
    let mut output = OutputBuffer::new(decoded.as_mut_slice());
    if Hdlc::decode(frame, &mut output).is_err() {
        return false;
    }
    Packet::from_bytes(output.as_slice()).is_ok_and(|packet| match mode {
        Some(ResourceFaultMode::DropKeepAlive) => packet.context == PacketContext::KeepAlive,
        Some(_) => packet.context == PacketContext::Resource,
        None => false,
    })
}
