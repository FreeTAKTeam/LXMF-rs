use super::*;
use rusqlite::{params, OptionalExtension};
use sha2::{Digest, Sha256};
impl Session<'_> {
    pub fn publish(
        &mut self,
        context: Context<'_>,
        request: &PublishRequest,
    ) -> rusqlite::Result<Publication> {
        context_valid(context)?;
        storage::validate_key(&request.operation_id)?;
        validate_event(&request.event, self.config)?;
        let route = routing(context, &request.event.route)?;
        let payload = serde_json::to_value(&request.event)
            .map_err(|error| rusqlite::Error::ToSqlConversionFailure(Box::new(error)))?;
        if request.completion_reserve > self.config.max_completion_bytes {
            return Err(storage::error(
                "BROKER_COMPLETION_TOO_LARGE",
                "completion reservation exceeds configured bounds",
            ));
        }
        let fingerprint = hex::encode(Sha256::digest(encode(request)?.as_bytes()));
        let previous: Option<String> = self.connection.query_row("SELECT fingerprint FROM event_broker_operations WHERE owner=?1 AND scope=?2 AND operation_id=?3", params![context.owner,context.scope,request.operation_id], |row| row.get(0)).optional()?;
        if let Some(previous) = previous {
            if previous != fingerprint {
                return Err(storage::error(
                    "BROKER_OPERATION_CONFLICT",
                    "operation already published with different immutable content",
                ));
            }
            return self.reconcile(context, &request.operation_id)?.ok_or_else(|| {
                storage::error("BROKER_INVARIANT", "retained operation disappeared")
            });
        }
        let duplicate: bool = self.connection.query_row("SELECT EXISTS(SELECT 1 FROM event_broker_event_keys WHERE owner=?1 AND scope=?2 AND event_id=?3)", params![context.owner,context.scope,request.event.event_id], |row| row.get(0))?;
        if duplicate {
            return Err(storage::error(
                "BROKER_EVENT_CONFLICT",
                "event identity belongs to another operation",
            ));
        }
        // Retained immutable identity and future completion both count against admission.
        let retained = context.owner.len()
            + context.scope.len()
            + request.operation_id.len()
            + request.event.event_id.len()
            + route.len()
            + fingerprint.len()
            + 1024;
        // Prepay both retained event identity rows, including a worst-case 128-byte completion ID.
        let retained = retained
            + 2 * (context.owner.len() + context.scope.len())
            + request.event.event_id.len()
            + 128
            + 256;
        let charge = retained
            .checked_add(request.completion_reserve)
            .and_then(|bytes| i64::try_from(bytes).ok())
            .ok_or_else(|| storage::error("BROKER_EVENT_TOO_LARGE", "accounting overflow"))?;
        let budget: i64 =
            self.connection.query_row("SELECT budget FROM broker_meta", [], |row| row.get(0))?;
        storage::physical_admission(self.connection, charge as u64, budget as u64)?;
        storage::charge(self.connection, charge)?;
        let position = storage::append_with_limits(
            self.connection,
            &route,
            &request.event.event_type,
            &payload,
            false,
            self.config.limits,
        )?;
        self.connection.execute(
            "INSERT INTO event_broker_event_keys VALUES(?1,?2,?3)",
            params![context.owner, context.scope, request.event.event_id],
        )?;
        self.connection.execute("INSERT INTO event_broker_operations(owner,scope,operation_id,event_id,fingerprint,route,position,state,completion_reserve) VALUES(?1,?2,?3,?4,?5,?6,?7,'Queued',?8)", params![context.owner,context.scope,request.operation_id,request.event.event_id,fingerprint,route,position,request.completion_reserve as i64])?;
        Ok(Publication {
            operation_id: request.operation_id.clone(),
            event_id: request.event.event_id.clone(),
            position: EventPosition(position as u64),
            state: ProcessingState::Queued,
        })
    }
    pub fn reconcile(
        &mut self,
        context: Context<'_>,
        operation_id: &str,
    ) -> rusqlite::Result<Option<Publication>> {
        context_valid(context)?;
        storage::validate_key(operation_id)?;
        self.connection.query_row("SELECT event_id,position,state FROM event_broker_operations WHERE owner=?1 AND scope=?2 AND operation_id=?3", params![context.owner,context.scope,operation_id], |row| {
            let state: String = row.get(2)?;
            let state = match state.as_str() {
                "Queued" => ProcessingState::Queued,
                "Claimed" => ProcessingState::Claimed,
                "Unknown" => ProcessingState::Unknown,
                "Completed" => ProcessingState::Completed,
                "Failed" => ProcessingState::Failed,
                _ => return Err(storage::error("BROKER_INVARIANT", "unknown processing state")),
            };
            Ok(Publication { operation_id: operation_id.to_string(), event_id: row.get(0)?, position: EventPosition(row.get::<_,i64>(1)? as u64), state })
        }).optional()
    }
}
