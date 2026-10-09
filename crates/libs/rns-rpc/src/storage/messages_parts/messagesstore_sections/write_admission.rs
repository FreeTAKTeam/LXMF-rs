const WRITE_QUEUE_COUNT: usize = 64;
const WRITE_QUEUE_BYTES: u64 = 64 * 1024 * 1024;

struct WriteReservation { bytes: u64, used: Arc<AtomicU64> }
impl Drop for WriteReservation { fn drop(&mut self) { self.used.fetch_sub(self.bytes,Ordering::AcqRel); } }
struct OutboundWriteJob { command: OutboundWriteCommand, _reservation: WriteReservation }
struct WriteSender { tx: mpsc::SyncSender<OutboundWriteJob>, used: Arc<AtomicU64> }
impl WriteSender {
    fn channel() -> (Self,mpsc::Receiver<OutboundWriteJob>) {
        let (tx,rx)=mpsc::sync_channel(WRITE_QUEUE_COUNT);
        (Self { tx,used:Arc::new(AtomicU64::new(0)) },rx)
    }
    fn reserve(&self,bytes: u64) -> Result<WriteReservation,super::broker::BrokerError> {
        let mut used = self.used.load(Ordering::Acquire);
        loop {
            let next = used.checked_add(bytes).filter(|sum| *sum <= WRITE_QUEUE_BYTES)
                .ok_or_else(|| super::broker::BrokerError {
                    code: "SDK_STORAGE_WRITE_BUSY",
                    message: "database writer byte capacity exhausted before admission".into(),
                })?;
            match self.used.compare_exchange_weak(used, next, Ordering::AcqRel, Ordering::Acquire) {
                Ok(_) => break,
                Err(actual) => used = actual,
            }
        }
        Ok(WriteReservation {bytes,used:Arc::clone(&self.used)})
    }
    fn send_reserved(&self,command: OutboundWriteCommand,reservation: WriteReservation) -> Result<(),super::broker::BrokerError> {
        self.tx.try_send(OutboundWriteJob {command,_reservation:reservation}).map_err(|error|super::broker::BrokerError {
            code:"SDK_STORAGE_WRITE_BUSY", message:match error {mpsc::TrySendError::Full(_)=>"database writer count capacity exhausted before admission",mpsc::TrySendError::Disconnected(_)=>"database writer has stopped"}.into()
        })
    }
    fn send(&self,command: OutboundWriteCommand) -> Result<(),super::broker::BrokerError> {
        let reservation=self.reserve(command.retained_bytes())?;
        self.send_reserved(command,reservation)
    }
}
fn json_retained_bytes(value: &JsonValue) -> u64 {
    let own=std::mem::size_of::<JsonValue>() as u64;
    own+match value {
        JsonValue::String(s)=>s.capacity() as u64,
        JsonValue::Array(values)=>values.capacity() as u64*own+values.iter().map(json_retained_bytes).sum::<u64>(),
        JsonValue::Object(values)=>values.iter().map(|(key,value)|128+key.capacity() as u64+json_retained_bytes(value)).sum(),
        _=>0,
    }
}
fn message_retained_bytes(record: &MessageRecord) -> u64 {
    [&record.id,&record.source,&record.destination,&record.title,&record.content,&record.direction].iter().map(|s|s.capacity() as u64).sum::<u64>()
        +record.fields.as_ref().map(json_retained_bytes).unwrap_or(0)+record.receipt_status.as_ref().map(|s|s.capacity() as u64).unwrap_or(0)+256
}
impl OutboundWriteCommand {
    fn retained_bytes(&self) -> u64 {
        let bytes=match self {
            Self::InsertMessage {record,..}=>message_retained_bytes(record),
            Self::Broker {command,..}=>match command.as_ref() {
                super::broker::BrokerCommand::Admit {owner,source,request}=>owner.capacity() as u64+source.capacity() as u64+[&request.identity,&request.operation_id,&request.destination,&request.title,&request.content].iter().map(|s|s.capacity() as u64).sum::<u64>()+request.fields.as_ref().map(json_retained_bytes).unwrap_or(0)+[&request.options.method,&request.options.ticket,&request.options.source_private_key].iter().filter_map(|s|s.as_ref()).map(|s|s.capacity() as u64).sum::<u64>(),
                super::broker::BrokerCommand::Reconcile {owner,source,request}=>owner.capacity() as u64+source.capacity() as u64+request.identity.capacity() as u64+request.operation_id.capacity() as u64,
                super::broker::BrokerCommand::Prepared {message_id,payload}=>message_id.capacity() as u64+json_retained_bytes(payload),
                super::broker::BrokerCommand::DispatchState {message_id,state}=>message_id.capacity() as u64+state.capacity() as u64,
                super::broker::BrokerCommand::Inbound {record,raw_hex}=>message_retained_bytes(record)+raw_hex.as_ref().map(|s|s.capacity() as u64).unwrap_or(0),
                super::broker::BrokerCommand::Resume {owner,destination,request}=>owner.capacity() as u64+destination.capacity() as u64+request.consumer_id.0.capacity() as u64+request.identity.capacity() as u64+request.journal_id.as_ref().map(|id|id.0.capacity() as u64).unwrap_or(0),
                super::broker::BrokerCommand::Fetch {owner,destination,request}=>owner.capacity() as u64+destination.capacity() as u64+request.consumer_id.0.capacity() as u64+request.identity.capacity() as u64,
                super::broker::BrokerCommand::Ack {owner,destination,request}=>owner.capacity() as u64+destination.capacity() as u64+request.consumer_id.0.capacity() as u64+request.identity.capacity() as u64+request.journal_id.0.capacity() as u64+request.receipt.0.capacity() as u64,
            },
            Self::UpdateMessageFields {fields_json,message_id,..}=>fields_json.as_ref().map(|s|s.capacity() as u64).unwrap_or(0)+message_id.capacity() as u64,
            Self::InsertAnnounce {record,..}=>record.app_data_hex.as_ref().map(|s|s.capacity() as u64).unwrap_or(0)+record.capabilities.iter().map(|s|s.capacity() as u64+64).sum::<u64>()+record.name.as_ref().map(|s|s.capacity() as u64).unwrap_or(0)+record.name_source.as_ref().map(|s|s.capacity() as u64).unwrap_or(0)+record.id.capacity() as u64+record.peer.capacity() as u64+1024,
            Self::ResolveReceiptStatus {message_id,candidate_status,..}=>message_id.capacity() as u64+candidate_status.capacity() as u64,
            Self::UpdateReceiptStatus {message_id,status,..}=>message_id.capacity() as u64+status.capacity() as u64,
            Self::UpsertAnnounceIdentity {peer,public_key_hex,verifying_key_hex,..}=>peer.capacity() as u64+public_key_hex.capacity() as u64+verifying_key_hex.capacity() as u64,
            Self::UpsertTicket {destination,ticket,..}|Self::UpsertOutboundTicket {destination,ticket,..}=>destination.capacity() as u64+ticket.capacity() as u64,
            Self::UpsertTicketLastDelivery {destination,..}=>destination.capacity() as u64,
            Self::PruneExpiredTickets {..}|Self::PruneMessagesToLimitBytes {..}=>0,
        };
        bytes.saturating_add(512)
    }
}

#[cfg(test)]
mod writer_admission_tests {
    use super::*;

    #[test]
    fn writer_admission_rejects_overflow_and_recovers_after_release() {
        let (sender, _receiver) = WriteSender::channel();
        let held = sender.reserve(WRITE_QUEUE_BYTES).expect("full byte reservation");
        for bytes in [1, u64::MAX] {
            let error = match sender.reserve(bytes) {
                Ok(_) => panic!("writer admitted bytes beyond its capacity"),
                Err(error) => error,
            };
            assert_eq!(error.code, "SDK_STORAGE_WRITE_BUSY");
            assert_eq!(sender.used.load(Ordering::Acquire), WRITE_QUEUE_BYTES);
        }
        drop(held);
        assert_eq!(sender.used.load(Ordering::Acquire), 0);
        let recovered = sender.reserve(WRITE_QUEUE_BYTES).expect("capacity released");
        drop(recovered);
        assert_eq!(sender.used.load(Ordering::Acquire), 0);
    }

    #[test]
    fn writer_admission_bounds_concurrent_reservations() {
        let (sender, _receiver) = WriteSender::channel();
        let sender = Arc::new(sender);
        let barrier = Arc::new(std::sync::Barrier::new(17));
        let accepted = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        std::thread::scope(|scope| {
            for _ in 0..16 {
                let sender = Arc::clone(&sender);
                let barrier = Arc::clone(&barrier);
                let accepted = Arc::clone(&accepted);
                scope.spawn(move || {
                    barrier.wait();
                    let held = sender.reserve(WRITE_QUEUE_BYTES / 8);
                    if held.is_ok() {
                        accepted.fetch_add(1, Ordering::AcqRel);
                    }
                    barrier.wait();
                    barrier.wait();
                    match held {
                        Ok(reservation) => drop(reservation),
                        Err(error) => assert_eq!(error.code, "SDK_STORAGE_WRITE_BUSY"),
                    }
                });
            }
            barrier.wait();
            barrier.wait();
            let admitted = accepted.load(Ordering::Acquire);
            let retained = sender.used.load(Ordering::Acquire);
            barrier.wait();
            assert_eq!(admitted, 8);
            assert_eq!(retained, WRITE_QUEUE_BYTES);
        });
        assert_eq!(sender.used.load(Ordering::Acquire), 0);
    }
}
