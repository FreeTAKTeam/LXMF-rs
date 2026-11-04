//! Atomic standalone API composed from the same journal/receipt/physical guards as LXMF.
use crate::{storage, *};
use rusqlite::{Connection, Transaction, TransactionBehavior};
use std::path::Path;
mod processing;
mod publish;
mod schema;
#[cfg(test)]
mod tests;

/// An owned disk broker. Every operation in `transaction` commits atomically or rolls back.
/// Opening does not invalidate live claims: after their lease expires, call `expire_claims`
/// and explicitly resolve `Unknown` work. A journal ACK means custody, never processing.
pub struct Broker {
    connection: Connection,
    config: Config,
}
impl Broker {
    pub fn open(path: impl AsRef<Path>, config: Config) -> rusqlite::Result<Self> {
        config.validate()?;
        let connection = Connection::open(path)?;
        connection.busy_timeout(std::time::Duration::from_secs(5))?;
        storage::configure_profile_with_limits(&connection, config.budget_bytes, config.limits)?;
        storage::physical_guard::single_transaction(&connection, |conn| {
            let tx = conn.unchecked_transaction()?;
            schema::initialize(&tx, config)?;
            tx.commit().map_err(storage::physical_guard::map_commit_error)
        })?;
        // Reinstall the profile after schema creation so startup and ordinary commits share guards.
        storage::configure_profile_with_limits(&connection, config.budget_bytes, config.limits)?;
        Ok(Self { connection, config })
    }

    pub fn transaction<T>(
        &mut self,
        action: impl FnOnce(&mut Session<'_>) -> rusqlite::Result<T>,
    ) -> rusqlite::Result<T> {
        storage::physical_guard::single_transaction(&self.connection, |conn| {
            let tx = Transaction::new_unchecked(conn, TransactionBehavior::Immediate)?;
            let result = action(&mut Session::new(&tx, self.config)?)?;
            tx.commit().map_err(storage::physical_guard::map_commit_error)?;
            Ok(result)
        })
    }

    pub fn publish(
        &mut self,
        context: Context<'_>,
        request: &PublishRequest,
    ) -> rusqlite::Result<Publication> {
        self.transaction(|session| session.publish(context, request))
    }
    pub fn reconcile(
        &mut self,
        context: Context<'_>,
        operation_id: &str,
    ) -> rusqlite::Result<Option<Publication>> {
        self.transaction(|session| session.reconcile(context, operation_id))
    }
    /// Explicitly establish a new subscription at retained history or the current tail.
    /// This never resets an existing consumer or silently bootstraps a restored checkpoint.
    pub fn subscribe(
        &mut self,
        context: Context<'_>,
        route: &Route,
        consumer_id: ConsumerId,
        start: SubscriptionStart,
    ) -> rusqlite::Result<BrokerCheckpoint> {
        self.transaction(|session| session.subscribe(context, route, consumer_id, start))
    }
    pub fn resume(
        &mut self,
        context: Context<'_>,
        route: &Route,
        request: ResumeRequest,
    ) -> rusqlite::Result<BrokerCheckpoint> {
        self.transaction(|session| session.resume(context, route, request))
    }
    pub fn fetch(
        &mut self,
        context: Context<'_>,
        route: &Route,
        request: FetchRequest,
    ) -> rusqlite::Result<BrokerBatch> {
        self.transaction(|session| session.fetch(context, route, request))
    }
    pub fn ack(
        &mut self,
        context: Context<'_>,
        route: &Route,
        request: AckStoredRequest,
    ) -> rusqlite::Result<EventPosition> {
        self.transaction(|session| session.ack(context, route, request))
    }
}

/// Borrow an application's transaction. This cannot accidentally auto-commit partial writes.
/// Configure the shared disk profile before beginning the transaction, then call `initialize`.
/// The caller must roll back the transaction if any session method fails.
pub struct Session<'a> {
    connection: &'a Connection,
    config: Config,
}
impl<'a> Session<'a> {
    pub fn new(transaction: &'a Transaction<'_>, config: Config) -> rusqlite::Result<Self> {
        config.validate()?;
        schema::validate(transaction, config)?;
        Ok(Self { connection: transaction, config })
    }
    pub fn initialize(&mut self) -> rusqlite::Result<()> {
        schema::initialize(self.connection, self.config)
    }
    pub fn subscribe(
        &mut self,
        context: Context<'_>,
        route: &Route,
        consumer_id: ConsumerId,
        start: SubscriptionStart,
    ) -> rusqlite::Result<BrokerCheckpoint> {
        let scope = routing(context, route)?;
        storage::subscribe(self.connection, context.owner, &scope, consumer_id, start)
    }
    pub fn resume(
        &mut self,
        context: Context<'_>,
        route: &Route,
        request: ResumeRequest,
    ) -> rusqlite::Result<BrokerCheckpoint> {
        let scope = routing(context, route)?;
        storage::resume(self.connection, context.owner, &scope, request)
    }
    pub fn fetch(
        &mut self,
        context: Context<'_>,
        route: &Route,
        request: FetchRequest,
    ) -> rusqlite::Result<BrokerBatch> {
        let scope = routing(context, route)?;
        storage::fetch_with_limits(
            self.connection,
            context.owner,
            &scope,
            request,
            self.config.limits,
        )
    }
    pub fn ack(
        &mut self,
        context: Context<'_>,
        route: &Route,
        request: AckStoredRequest,
    ) -> rusqlite::Result<EventPosition> {
        let scope = routing(context, route)?;
        storage::ack(self.connection, context.owner, &scope, request)
    }
}
fn encode<T: serde::Serialize>(value: &T) -> rusqlite::Result<String> {
    serde_json::to_string(value)
        .map_err(|error| rusqlite::Error::ToSqlConversionFailure(Box::new(error)))
}
fn context_valid(context: Context<'_>) -> rusqlite::Result<()> {
    for value in [context.owner, context.scope] {
        if value.is_empty() || value.len() > 1024 {
            return Err(storage::error(
                "BROKER_CONTEXT_INVALID",
                "owner/scope must be nonempty and bounded",
            ));
        }
    }
    Ok(())
}
fn routing(context: Context<'_>, route: &Route) -> rusqlite::Result<String> {
    context_valid(context)?;
    if route.topic.is_empty()
        || route.topic.len() > 1024
        || route.subject.as_ref().is_some_and(|s| s.len() > 1024)
    {
        return Err(storage::error("BROKER_ROUTE_INVALID", "invalid bounded topic/subject"));
    }
    encode(&(context.scope, route))
}

fn validate_event(event: &EventEnvelope, config: Config) -> rusqlite::Result<()> {
    storage::validate_key(&event.event_id)?;
    if event.version != 1
        || event.occurred_at < 0
        || event.origin.is_empty()
        || event.origin.len() > 1024
        || event.event_type.is_empty()
        || event.event_type.len() > 1024
        || event.correlation_id.as_ref().is_some_and(|id| id.is_empty() || id.len() > 1024)
    {
        return Err(storage::error(
            "BROKER_EVENT_INVALID",
            "invalid version, timestamp or provenance",
        ));
    }
    if encode(&event.payload)?.len() > config.max_payload_bytes {
        return Err(storage::error("BROKER_EVENT_TOO_LARGE", "payload exceeds configured bound"));
    }
    Ok(())
}
