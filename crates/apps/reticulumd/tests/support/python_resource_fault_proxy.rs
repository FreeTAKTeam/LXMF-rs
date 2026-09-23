use rns_transport::buffer::OutputBuffer;
use rns_transport::iface::hdlc::Hdlc;
use rns_transport::{Packet, PacketContext, PacketType};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::task::JoinHandle;

#[derive(Clone, Copy, Debug)]
pub(super) enum ResourceFaultMode {
    DropFirst,
    DuplicateFirst,
    ReorderFirstTwo,
    DropAll,
    DropAllResourceTraffic,
    DropKeepAlive,
    DropLinkProofs,
    DuplicateChannelFirst,
    DuplicateLinkRequestFirst,
    CountLinkRequest,
    DuplicateLinkRequestProofFirst,
    CountLinkRequestProof,
    DropLinkRequest,
}

pub(super) struct PythonResourceFaultProxy {
    port: u16,
    task: JoinHandle<()>,
    matched_frames: Arc<AtomicUsize>,
}

impl PythonResourceFaultProxy {
    pub(super) async fn bind(target_port: u16, mode: ResourceFaultMode) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind resource fault proxy");
        let port = listener.local_addr().expect("resource fault proxy address").port();
        let matched_frames = Arc::new(AtomicUsize::new(0));
        let task_matched_frames = matched_frames.clone();
        let task = tokio::spawn(async move {
            let Ok((incoming, _)) = listener.accept().await else {
                return;
            };
            let Ok(outgoing) = TcpStream::connect(("127.0.0.1", target_port)).await else {
                return;
            };
            let (incoming_read, incoming_write) = incoming.into_split();
            let (outgoing_read, outgoing_write) = outgoing.into_split();
            let reverse_mode = matches!(
                mode,
                ResourceFaultMode::DropAllResourceTraffic
                    | ResourceFaultMode::DropLinkProofs
                    | ResourceFaultMode::CountLinkRequestProof
                    | ResourceFaultMode::CountLinkRequest
            )
            .then_some(mode);
            let forward_to_target = forward_frames(
                incoming_read,
                outgoing_write,
                Some(mode),
                task_matched_frames.clone(),
            );
            let forward_to_client = forward_frames(
                outgoing_read,
                incoming_write,
                reverse_mode,
                task_matched_frames.clone(),
            );
            tokio::select! {
                _ = forward_to_target => {}
                _ = forward_to_client => {}
            }
        });
        Self { port, task, matched_frames }
    }

    pub(super) fn port(&self) -> u16 {
        self.port
    }

    pub(super) fn matched_frame_count(&self) -> usize {
        self.matched_frames.load(Ordering::SeqCst)
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
    first_channel_seen: bool,
    first_link_request_seen: bool,
    first_link_request_proof_seen: bool,
    reordered_first: Option<Vec<u8>>,
}

async fn forward_frames<R, W>(
    mut reader: R,
    mut writer: W,
    mode: Option<ResourceFaultMode>,
    matched_frames: Arc<AtomicUsize>,
) where
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
            if should_match(&frame, mode) {
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
                    ResourceFaultMode::DropAll | ResourceFaultMode::DropAllResourceTraffic => {}
                    ResourceFaultMode::DropLinkProofs => {
                        matched_frames.fetch_add(1, Ordering::SeqCst);
                    }
                    ResourceFaultMode::DropKeepAlive => {}
                    ResourceFaultMode::DuplicateChannelFirst => {
                        if !state.first_channel_seen {
                            state.first_channel_seen = true;
                            write_frame(&mut writer, &frame).await;
                            write_frame(&mut writer, &frame).await;
                        } else {
                            write_frame(&mut writer, &frame).await;
                        }
                    }
                    ResourceFaultMode::DuplicateLinkRequestFirst => {
                        if !state.first_link_request_seen {
                            state.first_link_request_seen = true;
                            matched_frames.fetch_add(1, Ordering::SeqCst);
                            write_frame(&mut writer, &frame).await;
                            write_frame(&mut writer, &frame).await;
                        } else {
                            write_frame(&mut writer, &frame).await;
                        }
                    }
                    ResourceFaultMode::CountLinkRequest => {
                        matched_frames.fetch_add(1, Ordering::SeqCst);
                        write_frame(&mut writer, &frame).await;
                    }
                    ResourceFaultMode::DuplicateLinkRequestProofFirst => {
                        if !state.first_link_request_proof_seen {
                            state.first_link_request_proof_seen = true;
                            matched_frames.fetch_add(1, Ordering::SeqCst);
                            write_frame(&mut writer, &frame).await;
                            write_frame(&mut writer, &frame).await;
                        } else {
                            write_frame(&mut writer, &frame).await;
                        }
                    }
                    ResourceFaultMode::CountLinkRequestProof => {
                        matched_frames.fetch_add(1, Ordering::SeqCst);
                        write_frame(&mut writer, &frame).await;
                    }
                    ResourceFaultMode::DropLinkRequest => {}
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

fn should_match(frame: &[u8], mode: Option<ResourceFaultMode>) -> bool {
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
        Some(ResourceFaultMode::DropLinkProofs) => {
            packet.header.packet_type == PacketType::Proof && packet.context == PacketContext::None
        }
        Some(ResourceFaultMode::DuplicateChannelFirst) => packet.context == PacketContext::Channel,
        Some(ResourceFaultMode::DuplicateLinkRequestFirst)
        | Some(ResourceFaultMode::CountLinkRequest) => {
            packet.header.packet_type == PacketType::LinkRequest
        }
        Some(ResourceFaultMode::DuplicateLinkRequestProofFirst) => {
            packet.header.packet_type == PacketType::Proof
                && packet.context == PacketContext::LinkRequestProof
        }
        Some(ResourceFaultMode::CountLinkRequestProof) => {
            packet.header.packet_type == PacketType::Proof
                && packet.context == PacketContext::LinkRequestProof
        }
        Some(ResourceFaultMode::DropLinkRequest) => packet.context == PacketContext::None,
        Some(ResourceFaultMode::DropAllResourceTraffic) => {
            matches!(packet.context, PacketContext::Resource | PacketContext::ResourceRequest)
        }
        Some(_) => packet.context == PacketContext::Resource,
        None => false,
    })
}
