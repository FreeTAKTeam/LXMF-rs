//! The stamp and ticket members of `LXMessage`: the message id a stamp is
//! derived from, `get_stamp`, and `LXMRouter.handle_outbound`'s
//! `include_ticket`.

use super::{Message, Payload, WireMessage};
use crate::constants::FIELD_TICKET;
use crate::error::LxmfError;
use crate::stamp::{
    generate_stamp_with_value_until_cancelled, ticket_stamp, COST_TICKET, TICKET_LENGTH,
};

impl Message {
    /// `LXMessage.pack`'s message id: the hash of destination, source and the
    /// payload without its stamp, which a delivery stamp is then derived
    /// from. The timestamp has to be set — [`Message::to_wire`] fills a
    /// missing one at pack time, which would make the id unknowable before
    /// the message is sent.
    pub fn message_id(&self) -> Result<[u8; 32], LxmfError> {
        let destination =
            self.destination_hash.ok_or_else(|| LxmfError::Encode("missing destination".into()))?;
        let source = self.source_hash.ok_or_else(|| LxmfError::Encode("missing source".into()))?;
        let timestamp =
            self.timestamp.ok_or_else(|| LxmfError::Encode("missing timestamp".into()))?;
        let payload = Payload::new(
            timestamp,
            Some(self.content.clone()),
            Some(self.title.clone()),
            self.fields.clone(),
            None,
        );
        WireMessage::new(destination, source, payload).try_message_id()
    }

    /// `LXMRouter.handle_outbound` with `include_ticket`: carries a ticket the
    /// recipient may spend on their reply, as `FIELD_TICKET => [expires_at,
    /// ticket]`. `expires_at` is seconds since the epoch — the float
    /// `time.time() + TICKET_EXPIRY` the reference writes. Replaces a ticket
    /// field that is already there.
    pub fn include_ticket(&mut self, expires_at: f64, ticket: &[u8]) {
        let key = rmpv::Value::from(FIELD_TICKET);
        let entry = rmpv::Value::Array(alloc::vec![
            rmpv::Value::F64(expires_at),
            rmpv::Value::Binary(ticket.to_vec()),
        ]);
        match &mut self.fields {
            Some(rmpv::Value::Map(items)) => {
                if let Some(existing) = items.iter_mut().find(|(field, _)| *field == key) {
                    existing.1 = entry;
                } else {
                    items.push((key, entry));
                }
            }
            other => *other = Some(rmpv::Value::Map(alloc::vec![(key, entry)])),
        }
    }

    /// `LXMessage.get_stamp`: with an outbound ticket of [`TICKET_LENGTH`]
    /// bytes the stamp is the ticket's and worth [`COST_TICKET`]; otherwise a
    /// stamp is mined at `stamp_cost`, or none is owed. Sets the stamp and
    /// returns its value. Fails when the timestamp is missing, the cost is
    /// unattainable, or `cancelled` stopped the search — never leaving a
    /// message to go out unstamped by accident.
    pub fn stamp_for_delivery(
        &mut self,
        stamp_cost: Option<u32>,
        outbound_ticket: Option<&[u8]>,
        cancelled: impl FnMut() -> bool,
    ) -> Result<Option<u32>, LxmfError> {
        if let Some(ticket) = outbound_ticket.filter(|ticket| ticket.len() == TICKET_LENGTH) {
            let message_id = self.message_id()?;
            self.stamp = Some(ticket_stamp(ticket, &message_id));
            return Ok(Some(COST_TICKET));
        }
        let Some(stamp_cost) = stamp_cost else {
            return Ok(None);
        };
        let message_id = self.message_id()?;
        let mined = generate_stamp_with_value_until_cancelled(&message_id, stamp_cost, cancelled);
        let (stamp, value) = mined.ok_or_else(|| {
            LxmfError::Encode(
                "failed to generate LXMF stamp: the search was stopped before reaching the cost"
                    .into(),
            )
        })?;
        self.stamp = Some(stamp);
        Ok(Some(value))
    }
}
