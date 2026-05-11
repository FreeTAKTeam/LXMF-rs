use super::*;
use crate::destination::link::LinkPacketContext;
use crate::resource::ResourceRequest;
use tokio::sync::{mpsc, oneshot};

pub(super) const RESOURCE_MANAGER_LANE_CAPACITY: usize = 128;

#[derive(Clone)]
pub(super) struct ResourceManagerLane {
    manager: Arc<Mutex<ResourceManager>>,
    tx: mpsc::Sender<ResourceManagerCommand>,
}

#[allow(clippy::large_enum_variant)]
enum ResourceManagerCommand {
    HandleLinkPacket {
        packet: Packet,
        link: LinkPacketContext,
        reply: oneshot::Sender<ResourcePacketResult>,
    },
    FinishCompletion {
        completion: ResourceCompletion,
        reply: oneshot::Sender<(Packet, Vec<ResourceEvent>)>,
    },
    CommitPreparedSend {
        prepared: PreparedResourceSend,
        reply: oneshot::Sender<(Hash, Packet)>,
    },
    ConfirmOutboundDispatch {
        resource_hash: Hash,
        sent: bool,
        reply: oneshot::Sender<()>,
    },
    RetryPoll {
        now: Instant,
        #[allow(clippy::type_complexity)]
        reply: oneshot::Sender<(Vec<(AddressHash, ResourceRequest)>, Vec<(AddressHash, Packet)>)>,
    },
    RemoveLinks {
        link_ids: Vec<AddressHash>,
        reply: oneshot::Sender<()>,
    },
}

pub(super) struct ResourcePacketResult {
    pub completion_job: Option<ResourceCompletionJob>,
    pub responses: Vec<Packet>,
    pub events: Vec<ResourceEvent>,
}

impl ResourceManagerLane {
    pub fn spawn(manager: Arc<Mutex<ResourceManager>>) -> Self {
        Self::spawn_with_capacity(manager, RESOURCE_MANAGER_LANE_CAPACITY)
    }

    #[cfg(test)]
    pub(super) fn spawn_with_capacity(
        manager: Arc<Mutex<ResourceManager>>,
        capacity: usize,
    ) -> Self {
        Self::spawn_with_capacity_inner(manager, capacity)
    }

    #[cfg(not(test))]
    fn spawn_with_capacity(manager: Arc<Mutex<ResourceManager>>, capacity: usize) -> Self {
        Self::spawn_with_capacity_inner(manager, capacity)
    }

    fn spawn_with_capacity_inner(manager: Arc<Mutex<ResourceManager>>, capacity: usize) -> Self {
        let (tx, mut rx) = mpsc::channel(capacity);
        let worker_manager = manager.clone();
        tokio::spawn(async move {
            while let Some(command) = rx.recv().await {
                match command {
                    ResourceManagerCommand::HandleLinkPacket { packet, link, reply } => {
                        let mut manager = worker_manager.lock().await;
                        let mut responses = Vec::new();
                        let completion_job = if packet.context == PacketContext::Resource {
                            manager.take_completion_job_for_part(&packet, &link, &mut responses)
                        } else {
                            manager.handle_packet_with_context(&packet, &link, &mut responses);
                            None
                        };
                        let events = manager.drain_events();
                        let _ =
                            reply.send(ResourcePacketResult { completion_job, responses, events });
                    }
                    ResourceManagerCommand::FinishCompletion { completion, reply } => {
                        let mut manager = worker_manager.lock().await;
                        let proof_packet = completion.proof_packet;
                        manager.finish_resource_completion(completion);
                        let events = manager.drain_events();
                        let _ = reply.send((proof_packet, events));
                    }
                    ResourceManagerCommand::CommitPreparedSend { prepared, reply } => {
                        let mut manager = worker_manager.lock().await;
                        let _ = reply.send(manager.commit_prepared_send(prepared));
                    }
                    ResourceManagerCommand::ConfirmOutboundDispatch {
                        resource_hash,
                        sent,
                        reply,
                    } => {
                        let mut manager = worker_manager.lock().await;
                        manager.confirm_outbound_dispatch(resource_hash, sent);
                        let _ = reply.send(());
                    }
                    ResourceManagerCommand::RetryPoll { now, reply } => {
                        let mut manager = worker_manager.lock().await;
                        let requests = manager.retry_requests(now);
                        let advertisements = manager.poll_outgoing(now);
                        let _ = reply.send((requests, advertisements));
                    }
                    ResourceManagerCommand::RemoveLinks { link_ids, reply } => {
                        let mut manager = worker_manager.lock().await;
                        for link_id in link_ids {
                            manager.remove_link_state(link_id);
                        }
                        let _ = reply.send(());
                    }
                }
            }
        });
        Self { manager, tx }
    }

    #[cfg(test)]
    pub fn manager_handle(&self) -> Arc<Mutex<ResourceManager>> {
        self.manager.clone()
    }

    #[cfg(test)]
    pub fn try_enqueue_link_packet_for_test(
        &self,
        packet: Packet,
        link: Arc<Mutex<Link>>,
    ) -> Result<oneshot::Receiver<ResourcePacketResult>, ()> {
        let link_context = link.try_lock().map_err(|_| ())?.packet_context();
        let (reply, rx) = oneshot::channel();
        self.tx
            .try_send(ResourceManagerCommand::HandleLinkPacket {
                packet,
                link: link_context,
                reply,
            })
            .map_err(|_| ())?;
        Ok(rx)
    }

    pub async fn handle_link_packet(
        &self,
        packet: Packet,
        link: Arc<Mutex<Link>>,
    ) -> ResourcePacketResult {
        let link_context = match link.try_lock() {
            Ok(link) => link.packet_context(),
            Err(_) => {
                log::debug!("resource: skipping packet while link context is busy");
                return ResourcePacketResult {
                    completion_job: None,
                    responses: Vec::new(),
                    events: Vec::new(),
                };
            }
        };
        let (reply, rx) = oneshot::channel();
        match self.tx.try_send(ResourceManagerCommand::HandleLinkPacket {
            packet,
            link: link_context.clone(),
            reply,
        }) {
            Ok(()) => {
                if let Ok(result) = rx.await {
                    return result;
                }
                ResourcePacketResult {
                    completion_job: None,
                    responses: Vec::new(),
                    events: Vec::new(),
                }
            }
            Err(mpsc::error::TrySendError::Full(_)) => {
                log::debug!("resource: skipping packet while manager lane is full");
                ResourcePacketResult {
                    completion_job: None,
                    responses: Vec::new(),
                    events: Vec::new(),
                }
            }
            Err(mpsc::error::TrySendError::Closed(command)) => {
                let ResourceManagerCommand::HandleLinkPacket { packet, link, .. } = command else {
                    unreachable!("unexpected resource manager command")
                };
                let mut manager = self.manager.lock().await;
                let mut responses = Vec::new();
                let completion_job = if packet.context == PacketContext::Resource {
                    manager.take_completion_job_for_part(&packet, &link, &mut responses)
                } else {
                    manager.handle_packet_with_context(&packet, &link, &mut responses);
                    None
                };
                let events = manager.drain_events();
                ResourcePacketResult { completion_job, responses, events }
            }
        }
    }

    pub async fn finish_completion(
        &self,
        completion: ResourceCompletion,
    ) -> (Packet, Vec<ResourceEvent>) {
        let proof_packet = completion.proof_packet;
        let (reply, rx) = oneshot::channel();
        let completion = match self
            .tx
            .try_send(ResourceManagerCommand::FinishCompletion { completion, reply })
        {
            Ok(()) => {
                if let Ok(result) = rx.await {
                    return result;
                }
                return (proof_packet, Vec::new());
            }
            Err(mpsc::error::TrySendError::Full(command)) => {
                let ResourceManagerCommand::FinishCompletion { completion, .. } = command else {
                    unreachable!("unexpected resource manager command")
                };
                let Ok(mut manager) = self.manager.try_lock() else {
                    log::debug!("resource: deferring completion while manager lane is full");
                    return (proof_packet, Vec::new());
                };
                manager.finish_resource_completion(completion);
                let events = manager.drain_events();
                return (proof_packet, events);
            }
            Err(mpsc::error::TrySendError::Closed(command)) => match command {
                ResourceManagerCommand::FinishCompletion { completion, .. } => completion,
                _ => unreachable!("unexpected resource manager command"),
            },
        };

        let mut manager = self.manager.lock().await;
        manager.finish_resource_completion(completion);
        let events = manager.drain_events();
        (proof_packet, events)
    }

    pub async fn commit_prepared_send(
        &self,
        prepared: PreparedResourceSend,
    ) -> Result<(Hash, Packet), RnsError> {
        let (reply, rx) = oneshot::channel();
        match self.tx.try_send(ResourceManagerCommand::CommitPreparedSend { prepared, reply }) {
            Ok(()) => {
                if let Ok(result) = rx.await {
                    return Ok(result);
                }
            }
            Err(mpsc::error::TrySendError::Full(command)) => {
                let ResourceManagerCommand::CommitPreparedSend { prepared, .. } = command else {
                    unreachable!("unexpected resource manager command")
                };
                let Ok(mut manager) = self.manager.try_lock() else {
                    log::debug!("resource: dropping prepared send while manager lane is full");
                    return Err(RnsError::ConnectionError);
                };
                return Ok(manager.commit_prepared_send(prepared));
            }
            Err(mpsc::error::TrySendError::Closed(command)) => {
                let ResourceManagerCommand::CommitPreparedSend { prepared, .. } = command else {
                    unreachable!("unexpected resource manager command")
                };
                let mut manager = self.manager.lock().await;
                return Ok(manager.commit_prepared_send(prepared));
            }
        }

        Err(RnsError::ConnectionError)
    }

    pub async fn confirm_outbound_dispatch(&self, resource_hash: Hash, sent: bool) {
        let (reply, rx) = oneshot::channel();
        match self.tx.try_send(ResourceManagerCommand::ConfirmOutboundDispatch {
            resource_hash,
            sent,
            reply,
        }) {
            Ok(()) => {
                if rx.await.is_ok() {
                    return;
                }
            }
            Err(mpsc::error::TrySendError::Full(command)) => {
                let ResourceManagerCommand::ConfirmOutboundDispatch { resource_hash, sent, .. } =
                    command
                else {
                    unreachable!("unexpected resource manager command")
                };
                let Ok(mut manager) = self.manager.try_lock() else {
                    log::debug!("resource: deferring outbound dispatch confirmation while manager lane is full");
                    return;
                };
                manager.confirm_outbound_dispatch(resource_hash, sent);
                return;
            }
            Err(mpsc::error::TrySendError::Closed(command)) => {
                let ResourceManagerCommand::ConfirmOutboundDispatch { resource_hash, sent, .. } =
                    command
                else {
                    unreachable!("unexpected resource manager command")
                };
                let mut manager = self.manager.lock().await;
                manager.confirm_outbound_dispatch(resource_hash, sent);
                return;
            }
        }

        let mut manager = self.manager.lock().await;
        manager.confirm_outbound_dispatch(resource_hash, sent);
    }

    pub async fn retry_poll(
        &self,
        now: Instant,
    ) -> (Vec<(AddressHash, ResourceRequest)>, Vec<(AddressHash, Packet)>) {
        let (reply, rx) = oneshot::channel();
        match self.tx.try_send(ResourceManagerCommand::RetryPoll { now, reply }) {
            Ok(()) => {
                if let Ok(result) = rx.await {
                    return result;
                }
            }
            Err(mpsc::error::TrySendError::Full(_)) => {
                log::debug!("resource: skipping retry poll while manager lane is full");
                return (Vec::new(), Vec::new());
            }
            Err(mpsc::error::TrySendError::Closed(command)) => {
                let ResourceManagerCommand::RetryPoll { now, .. } = command else {
                    unreachable!("unexpected resource manager command")
                };
                let mut manager = self.manager.lock().await;
                let requests = manager.retry_requests(now);
                let advertisements = manager.poll_outgoing(now);
                return (requests, advertisements);
            }
        }

        let mut manager = self.manager.lock().await;
        let requests = manager.retry_requests(now);
        let advertisements = manager.poll_outgoing(now);
        (requests, advertisements)
    }

    pub async fn remove_link_state(&self, link_ids: Vec<AddressHash>) {
        if link_ids.is_empty() {
            return;
        }
        let (reply, rx) = oneshot::channel();
        match self
            .tx
            .try_send(ResourceManagerCommand::RemoveLinks { link_ids: link_ids.clone(), reply })
        {
            Ok(()) => {
                if rx.await.is_ok() {
                    return;
                }
            }
            Err(mpsc::error::TrySendError::Full(_)) => {
                log::debug!("resource: deferring link-state cleanup while manager lane is full");
                return;
            }
            Err(mpsc::error::TrySendError::Closed(command)) => {
                let ResourceManagerCommand::RemoveLinks { link_ids, .. } = command else {
                    unreachable!("unexpected resource manager command")
                };
                let mut manager = self.manager.lock().await;
                for link_id in link_ids {
                    manager.remove_link_state(link_id);
                }
                return;
            }
        }

        let mut manager = self.manager.lock().await;
        for link_id in link_ids {
            manager.remove_link_state(link_id);
        }
    }
}
