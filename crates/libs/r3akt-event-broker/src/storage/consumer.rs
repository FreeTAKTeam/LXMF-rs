use super::*;
pub fn resume(
    conn: &Connection,
    owner: &str,
    destination: &str,
    request: ResumeRequest,
) -> rusqlite::Result<BrokerCheckpoint> {
    validate_key(&request.consumer_id.0)?;
    let (journal, floor, tail): (String, i64, i64) =
        conn.query_row("SELECT journal_id,floor,next_position-1 FROM broker_meta", [], |r| {
            Ok((r.get(0)?, r.get(1)?, r.get(2)?))
        })?;
    let stored = position(request.stored)?;
    if stored > tail {
        return Err(error(
            "SDK_BROKER_RECOVERY_REQUIRED",
            "local checkpoint exceeds this journal's tail",
        ));
    }
    if request.journal_id.as_ref().is_some_and(|id| id.0 != journal) {
        return Err(error(
            "SDK_BROKER_RECOVERY_REQUIRED",
            "local inbox belongs to another journal",
        ));
    }
    let existing = consumer(conn, &request.consumer_id.0, owner, destination)?;
    let broker_stored = match existing {
        Some((b, issued)) => {
            if request.journal_id.is_none() && (stored != 0 || b != 0) {
                return Err(error(
                    "SDK_BROKER_RECOVERY_REQUIRED",
                    "existing custody checkpoint requires its journal identifier",
                ));
            }
            if stored > b {
                let range: IssuedRange =
                    issued.as_deref().map(decode).transpose()?.ok_or_else(|| {
                        error(
                            "SDK_BROKER_RECOVERY_REQUIRED",
                            "local custody is ahead of any issued batch",
                        )
                    })?;
                if position(range.end)? != stored {
                    return Err(error(
                        "SDK_BROKER_RECOVERY_REQUIRED",
                        "local custody does not match the outstanding issued range",
                    ));
                }
            }
            if b > stored {
                return Err(error(
                    "SDK_BROKER_RECOVERY_REQUIRED",
                    "daemon checkpoint is ahead of restored RCH custody",
                ));
            }
            b
        }
        None => {
            if stored != 0 || request.journal_id.is_some() || floor != 0 {
                return Err(error(
                    "SDK_BROKER_RECOVERY_REQUIRED",
                    "explicit bootstrap is required for this journal",
                ));
            }
            create_consumer(conn, &request.consumer_id, owner, destination, 0)?;
            0
        }
    };
    Ok(BrokerCheckpoint {
        journal_id: JournalId(journal),
        consumer_id: request.consumer_id,
        stored: EventPosition(broker_stored as u64),
        floor: EventPosition(floor as u64),
        tail: EventPosition(tail as u64),
    })
}
pub fn fetch(
    conn: &Connection,
    owner: &str,
    destination: &str,
    request: FetchRequest,
) -> rusqlite::Result<BrokerBatch> {
    fetch_with_limits(conn, owner, destination, request, Limits::default())
}
pub fn fetch_with_limits(
    conn: &Connection,
    owner: &str,
    destination: &str,
    request: FetchRequest,
    limits: Limits,
) -> rusqlite::Result<BrokerBatch> {
    limits.validate()?;
    let (stored, issued) = require_consumer(conn, &request.consumer_id.0, owner, destination)?;
    if let Some(issued) = issued {
        let range: IssuedRange = decode(&issued)?;
        replay_batch(conn, &request.consumer_id, destination, range)
    } else {
        if request.max_events == 0
            || request.max_events > limits.max_batch_events
            || request.max_bytes < 4096
            || request.max_bytes > limits.max_batch_bytes
        {
            return Err(error("SDK_VALIDATION_BROKER_LIMIT", "invalid fetch count or byte limit"));
        }
        let count = request.max_events;
        let byte_limit = request.max_bytes;
        let mut events = Vec::new();
        let mut end = stored;
        let journal: String =
            conn.query_row("SELECT journal_id FROM broker_meta", [], |r| r.get(0))?;
        // Bound scans as well as visible results. Empty batches advance invisible positions.
        let mut stmt = conn.prepare("SELECT position,local_destination,event_type,created_at,payload FROM broker_events WHERE position>?1 ORDER BY position LIMIT ?2")?;
        let mut rows = stmt.query(params![stored, limits.max_batch_events as i64])?;
        let mut bytes = 2048usize;
        while let Some(row) = rows.next()? {
            let pos: i64 = row.get(0)?;
            let scope: String = row.get(1)?;
            if scope == destination {
                let payload: String = row.get(4)?;
                let event = BrokerEvent {
                    version: 1,
                    position: EventPosition(pos as u64),
                    event_type: row.get(2)?,
                    created_at: row.get(3)?,
                    payload: decode(&payload)?,
                };
                let size = encode(&event)?.len() + 1;
                if bytes + size > byte_limit {
                    if events.is_empty() {
                        return Err(error(
                            "SDK_BROKER_BATCH_LIMIT_TOO_SMALL",
                            "increase fetch byte limit to admit the next complete event",
                        ));
                    }
                    break;
                }
                if bytes + size > limits.max_batch_bytes {
                    return Err(error(
                        "SDK_STORAGE_BROKER_EVENT_TOO_LARGE",
                        "single event exceeds response budget",
                    ));
                }
                bytes += size;
                events.push(event);
            }
            end = pos;
            if events.len() == count {
                break;
            }
        }
        let receipt =
            receipt(conn, &journal, &request.consumer_id.0, owner, destination, stored, end)?;
        let batch = BrokerBatch {
            journal_id: JournalId(journal),
            consumer_id: request.consumer_id.clone(),
            start: EventPosition(stored as u64),
            end: EventPosition(end as u64),
            events,
            receipt: StoredReceipt(receipt),
        };
        let encoded = encode(&batch)?;
        if encoded.len() > byte_limit {
            return Err(error(
                "SDK_BROKER_BATCH_LIMIT_TOO_SMALL",
                "encoded batch exceeds requested response budget",
            ));
        }
        if end > stored {
            let descriptor = encode(&IssuedRange {
                start: batch.start,
                end: batch.end,
                receipt: batch.receipt.clone(),
            })?;
            charge(conn, descriptor.len() as i64)?;
            conn.execute(
                "UPDATE broker_consumers SET issued=?1 WHERE consumer_id=?2",
                params![descriptor, request.consumer_id.0],
            )?;
        }
        Ok(batch)
    }
}
pub fn ack(
    conn: &Connection,
    owner: &str,
    destination: &str,
    request: AckStoredRequest,
) -> rusqlite::Result<EventPosition> {
    let (stored, issued) = require_consumer(conn, &request.consumer_id.0, owner, destination)?;
    let journal: String = conn.query_row("SELECT journal_id FROM broker_meta", [], |r| r.get(0))?;
    if journal != request.journal_id.0 {
        return Err(error("SDK_BROKER_RECOVERY_REQUIRED", "ACK belongs to another journal"));
    }
    // The receipt encodes its exact issued range; never trust the caller's end alone.
    let parts: Vec<&str> = request.receipt.0.split(':').collect();
    if parts.len() != 3 {
        return Err(error("SDK_BROKER_INVALID_RECEIPT", "invalid receipt"));
    }
    let start: i64 = parts[0]
        .parse()
        .map_err(|_| error("SDK_BROKER_INVALID_RECEIPT", "invalid receipt range"))?;
    let end: i64 = parts[1]
        .parse()
        .map_err(|_| error("SDK_BROKER_INVALID_RECEIPT", "invalid receipt range"))?;
    if start < 0 || end < start || end != position(request.end)? {
        return Err(error("SDK_BROKER_INVALID_RECEIPT", "receipt range mismatch"));
    }
    verify_receipt(
        conn,
        &journal,
        &request.consumer_id.0,
        owner,
        destination,
        (start, end),
        parts[2],
    )?;
    if end > stored {
        let batch: IssuedRange = issued
            .as_deref()
            .map(decode)
            .transpose()?
            .ok_or_else(|| error("SDK_BROKER_INVALID_RECEIPT", "no issued batch"))?;
        if batch.receipt != request.receipt || batch.end != request.end || start != stored {
            return Err(error(
                "SDK_BROKER_INVALID_RECEIPT",
                "ACK is not the outstanding issued batch",
            ));
        }
        mutation_admission(conn, false)?;
        conn.execute(
            "UPDATE broker_meta SET used_bytes=used_bytes-?1",
            [issued.as_ref().expect("issued batch verified").len() as i64],
        )?;
        conn.execute(
            "UPDATE broker_consumers SET stored=?1,issued=NULL WHERE consumer_id=?2",
            params![end, request.consumer_id.0],
        )?;
        prune(conn)?;
    }
    Ok(EventPosition(end.max(stored) as u64))
}

/// Explicitly bootstrap a new consumer after pruning; never reset an existing checkpoint.
/// The application authorizes the scope and bootstrap policy before this call.
pub fn subscribe(
    conn: &Connection,
    owner: &str,
    destination: &str,
    consumer_id: ConsumerId,
    start: SubscriptionStart,
) -> rusqlite::Result<BrokerCheckpoint> {
    validate_key(&consumer_id.0)?;
    if consumer(conn, &consumer_id.0, owner, destination)?.is_some() {
        return Err(error(
            "BROKER_CONSUMER_EXISTS",
            "existing consumer must resume its checkpoint",
        ));
    }
    let (journal, floor, tail): (String, i64, i64) =
        conn.query_row("SELECT journal_id,floor,next_position-1 FROM broker_meta", [], |row| {
            Ok((row.get(0)?, row.get(1)?, row.get(2)?))
        })?;
    let stored = match start {
        SubscriptionStart::EarliestAvailable => floor,
        SubscriptionStart::Latest => tail,
    };
    create_consumer(conn, &consumer_id, owner, destination, stored)?;
    Ok(BrokerCheckpoint {
        journal_id: JournalId(journal),
        consumer_id,
        stored: EventPosition(stored as u64),
        floor: EventPosition(floor as u64),
        tail: EventPosition(tail as u64),
    })
}

fn create_consumer(
    conn: &Connection,
    consumer_id: &ConsumerId,
    owner: &str,
    destination: &str,
    stored: i64,
) -> rusqlite::Result<()> {
    let count: i64 =
        conn.query_row("SELECT COUNT(*) FROM broker_consumers", [], |row| row.get(0))?;
    if count >= 256 {
        return Err(error("SDK_STORAGE_BROKER_FULL", "consumer capacity exhausted"));
    }
    charge(conn, (consumer_id.0.len() + owner.len() + destination.len() + 256) as i64)?;
    conn.execute(
        "INSERT INTO broker_consumers VALUES(?1,?2,?3,?4,NULL)",
        params![consumer_id.0, owner, destination, stored],
    )?;
    Ok(())
}
