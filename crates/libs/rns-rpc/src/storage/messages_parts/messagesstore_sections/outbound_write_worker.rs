impl MessagesStore {
    fn send_outbound_write_reply<T>(
        reply: mpsc::Sender<rusqlite::Result<T>>,
        result: rusqlite::Result<T>,
        operation: &'static str,
    ) {
        if reply.send(result).is_err() {
            log::debug!(
                "[messages-store] write completed after requester disconnected operation={operation}"
            );
        }
    }

    fn spawn_outbound_write_worker(
        write_state: Arc<WriteState>,
        rx: mpsc::Receiver<OutboundWriteCommand>,
    ) {
        std::thread::Builder::new()
            .name("messages-outbound-writer".to_string())
            .spawn(move || {
                while let Ok(command) = rx.recv() {
                    Self::handle_outbound_write_command(write_state.as_ref(), command);
                }
            })
            .expect("spawn messages outbound writer");
    }

    fn handle_outbound_write_command(write_state: &WriteState, command: OutboundWriteCommand) {
        match command {
            OutboundWriteCommand::InsertMessage { record, reply } => Self::send_outbound_write_reply(
                reply,
                Self::insert_message_direct(write_state, &record),
                "insert_message",
            ),
            OutboundWriteCommand::ResolveReceiptStatus {
                message_id,
                candidate_status,
                reply,
            } => Self::send_outbound_write_reply(
                reply,
                Self::resolve_receipt_status_direct(
                    write_state,
                    message_id.as_str(),
                    candidate_status.as_str(),
                ),
                "resolve_receipt_status",
            ),
            OutboundWriteCommand::PruneMessagesToLimitBytes { limit_bytes, reply } => {
                let result = Self::prune_messages_to_limit_bytes_direct(write_state, limit_bytes);
                if let Some(reply) = reply {
                    Self::send_outbound_write_reply(
                        reply,
                        result,
                        "prune_messages_to_limit_bytes",
                    );
                }
            }
            OutboundWriteCommand::UpdateReceiptStatus { message_id, status, reply } => {
                Self::send_outbound_write_reply(
                    reply,
                    Self::update_receipt_status_direct(
                        write_state,
                        message_id.as_str(),
                        status.as_str(),
                    ),
                    "update_receipt_status",
                );
            }
            OutboundWriteCommand::UpdateMessageFields { message_id, fields_json, reply } => {
                Self::send_outbound_write_reply(
                    reply,
                    Self::update_message_fields_direct(
                        write_state,
                        message_id.as_str(),
                        fields_json.as_deref(),
                    ),
                    "update_message_fields",
                );
            }
            OutboundWriteCommand::UpsertAnnounceIdentity {
                peer,
                public_key_hex,
                verifying_key_hex,
                updated_at,
                reply,
            } => Self::send_outbound_write_reply(
                reply,
                Self::upsert_announce_identity_direct(
                    write_state,
                    peer.as_str(),
                    public_key_hex.as_str(),
                    verifying_key_hex.as_str(),
                    updated_at,
                ),
                "upsert_announce_identity",
            ),
            OutboundWriteCommand::InsertAnnounce { record, reply } => Self::send_outbound_write_reply(
                reply,
                Self::insert_announce_direct(write_state, &record),
                "insert_announce",
            ),
            OutboundWriteCommand::UpsertTicket {
                destination,
                ticket,
                expires_at,
                reply,
            } => Self::send_outbound_write_reply(
                reply,
                Self::upsert_ticket_direct(
                    write_state,
                    destination.as_str(),
                    ticket.as_str(),
                    expires_at,
                ),
                "upsert_ticket",
            ),
            OutboundWriteCommand::PruneExpiredTickets { now, inbound_grace_secs, reply } => {
                Self::send_outbound_write_reply(
                    reply,
                    Self::prune_expired_tickets_direct(write_state, now, inbound_grace_secs),
                    "prune_expired_tickets",
                );
            }
            OutboundWriteCommand::UpsertOutboundTicket {
                destination,
                ticket,
                expires_at,
                reply,
            } => Self::send_outbound_write_reply(
                reply,
                Self::upsert_outbound_ticket_direct(
                    write_state,
                    destination.as_str(),
                    ticket.as_str(),
                    expires_at,
                ),
                "upsert_outbound_ticket",
            ),
            OutboundWriteCommand::UpsertTicketLastDelivery {
                destination,
                delivered_at,
                reply,
            } => Self::send_outbound_write_reply(
                reply,
                Self::upsert_ticket_last_delivery_direct(
                    write_state,
                    destination.as_str(),
                    delivered_at,
                ),
                "upsert_ticket_last_delivery",
            ),
        }
    }
}
