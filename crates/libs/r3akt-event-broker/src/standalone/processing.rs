use super::*;
use rand_core::{OsRng, RngCore};
use rusqlite::{params, OptionalExtension};
use sha2::{Digest, Sha256};
struct ProcessingRecord {
    state: String,
    token: Option<String>,
    until: Option<i64>,
    reserve: i64,
    prior: Option<String>,
    original_route: String,
}
impl Session<'_> {
    /// Claim queued work. Caller-supplied time must come from trusted application clock policy.
    pub fn claim(
        &mut self,
        context: Context<'_>,
        operation_id: &str,
        now: i64,
        lease_seconds: i64,
    ) -> rusqlite::Result<Option<Claim>> {
        context_valid(context)?;
        storage::validate_key(operation_id)?;
        if now < 0 || lease_seconds <= 0 || lease_seconds > self.config.max_lease_seconds {
            return Err(storage::error("BROKER_LEASE_INVALID", "lease exceeds configured bounds"));
        }
        let expires_at = now
            .checked_add(lease_seconds)
            .ok_or_else(|| storage::error("BROKER_LEASE_INVALID", "lease timestamp overflow"))?;
        storage::physical_guard::mutation(self.connection, false)?;
        let mut token = [0u8; 32];
        OsRng.fill_bytes(&mut token);
        let token = hex::encode(token);
        let changed = self.connection.execute("UPDATE event_broker_operations SET state='Claimed',claim_token=?4,claim_until=?5 WHERE owner=?1 AND scope=?2 AND operation_id=?3 AND state='Queued'", params![context.owner,context.scope,operation_id,token,expires_at])?;
        Ok((changed == 1).then(|| Claim {
            operation_id: operation_id.to_string(),
            token,
            expires_at,
        }))
    }

    /// Lease expiry records uncertainty; it never retries work or declares it completed.
    pub fn expire_claims(&mut self, context: Context<'_>, now: i64) -> rusqlite::Result<usize> {
        context_valid(context)?;
        if now < 0 {
            return Err(storage::error("BROKER_LEASE_INVALID", "negative clock timestamp"));
        }
        storage::physical_guard::mutation(self.connection, false)?;
        self.connection.execute("UPDATE event_broker_operations SET state='Unknown' WHERE owner=?1 AND scope=?2 AND state='Claimed' AND claim_until<=?3", params![context.owner,context.scope,now])
    }

    /// Explicit business recovery after inspecting uncertain effects. Reservation remains intact.
    /// `Fail` releases the reservation; no business completion is fabricated in the journal.
    pub fn recover(
        &mut self,
        context: Context<'_>,
        operation_id: &str,
        recovery: Recovery,
    ) -> rusqlite::Result<Publication> {
        context_valid(context)?;
        storage::validate_key(operation_id)?;
        let reserve: Option<i64> = self.connection.query_row("SELECT completion_reserve FROM event_broker_operations WHERE owner=?1 AND scope=?2 AND operation_id=?3 AND state='Unknown'", params![context.owner,context.scope,operation_id], |row| row.get(0)).optional()?;
        let reserve = reserve.ok_or_else(|| {
            storage::error("BROKER_RECOVERY_REQUIRED", "only uncertain work can be recovered")
        })?;
        storage::physical_guard::mutation(self.connection, false)?;
        let state = match recovery {
            Recovery::Requeue => "Queued",
            Recovery::Fail => {
                storage::release_charge(self.connection, reserve)?;
                "Failed"
            }
        };
        self.connection.execute("UPDATE event_broker_operations SET state=?4,claim_token=NULL,claim_until=NULL,completion_reserve=?5 WHERE owner=?1 AND scope=?2 AND operation_id=?3", params![context.owner,context.scope,operation_id,state,if recovery == Recovery::Fail {0} else {reserve}])?;
        self.reconcile(context, operation_id)?
            .ok_or_else(|| storage::error("BROKER_INVARIANT", "recovered operation disappeared"))
    }

    /// Atomically records processing completion, distinct from any consumer's custody ACK.
    /// Repeating the same completion and claim is idempotent, including after an uncertain commit.
    pub fn complete(
        &mut self,
        context: Context<'_>,
        claim: &Claim,
        now: i64,
        completion: &EventEnvelope,
    ) -> rusqlite::Result<Publication> {
        context_valid(context)?;
        storage::validate_key(&claim.operation_id)?;
        validate_event(completion, self.config)?;
        let route = routing(context, &completion.route)?;
        let payload = serde_json::to_value(completion)
            .map_err(|error| rusqlite::Error::ToSqlConversionFailure(Box::new(error)))?;
        let fingerprint = hex::encode(Sha256::digest(encode(completion)?.as_bytes()));
        let row: Option<ProcessingRecord> = self.connection.query_row("SELECT state,claim_token,claim_until,completion_reserve,completion_fingerprint,route FROM event_broker_operations WHERE owner=?1 AND scope=?2 AND operation_id=?3", params![context.owner,context.scope,claim.operation_id], |row| Ok(ProcessingRecord { state: row.get(0)?,token: row.get(1)?,until: row.get(2)?,reserve: row.get(3)?,prior: row.get(4)?,original_route: row.get(5)? })).optional()?;
        let ProcessingRecord { state, token, until, reserve, prior, original_route } =
            row.ok_or_else(|| storage::error("BROKER_CLAIM_INVALID", "unknown operation"))?;
        if token.as_deref() != Some(&claim.token) || until != Some(claim.expires_at) {
            return Err(storage::error("BROKER_CLAIM_INVALID", "claim capability does not match"));
        }
        if state == "Completed" && prior.as_deref() == Some(&fingerprint) {
            return self.reconcile(context, &claim.operation_id)?.ok_or_else(|| {
                storage::error("BROKER_INVARIANT", "completed operation disappeared")
            });
        }
        if state != "Claimed" || now < 0 || now >= claim.expires_at {
            return Err(storage::error(
                "BROKER_CLAIM_EXPIRED",
                "work must be explicitly recovered before completion",
            ));
        }
        if route != original_route {
            return Err(storage::error(
                "BROKER_ROUTE_INVALID",
                "completion must use the operation route",
            ));
        }
        let bytes = storage::event_bytes(&route, &completion.event_type, &encode(&payload)?);
        if bytes > reserve || bytes as usize > self.config.max_completion_bytes {
            return Err(storage::error(
                "BROKER_COMPLETION_TOO_LARGE",
                "completion exceeds its admitted reservation",
            ));
        }
        let duplicate: bool = self.connection.query_row("SELECT EXISTS(SELECT 1 FROM event_broker_event_keys WHERE owner=?1 AND scope=?2 AND event_id=?3)", params![context.owner,context.scope,completion.event_id], |row| row.get(0))?;
        if duplicate {
            return Err(storage::error(
                "BROKER_EVENT_CONFLICT",
                "completion event identity already exists",
            ));
        }
        // Both event identity keys and future claim/result fields were prepaid in retained metadata.
        storage::release_charge(self.connection, reserve)?;
        self.connection.execute(
            "INSERT INTO event_broker_event_keys VALUES(?1,?2,?3)",
            params![context.owner, context.scope, completion.event_id],
        )?;
        storage::append_with_limits(
            self.connection,
            &route,
            &completion.event_type,
            &payload,
            true,
            self.config.limits,
        )?;
        self.connection.execute("UPDATE event_broker_operations SET state='Completed',completion_reserve=0,completion_fingerprint=?4 WHERE owner=?1 AND scope=?2 AND operation_id=?3", params![context.owner,context.scope,claim.operation_id,fingerprint])?;
        self.reconcile(context, &claim.operation_id)?
            .ok_or_else(|| storage::error("BROKER_INVARIANT", "completed operation disappeared"))
    }
}
