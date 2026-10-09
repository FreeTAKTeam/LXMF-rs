use super::*;
pub fn execute(conn: &Connection, command: BrokerCommand) -> rusqlite::Result<Value> {
    if !enabled(conn)? {
        return Err(error(
            "SDK_BROKER_DISABLED",
            "durable broker has not been enabled by the operator",
        ));
    }
    let tx = conn.unchecked_transaction()?;
    let result = match command {
        BrokerCommand::Admit { owner, source, mut request } => {
            validate_key(&request.operation_id)?;
            // Source key material is never an input to the broker's signing authority.
            if request.options.source_private_key.is_some() {
                return Err(error(
                    "SDK_SECURITY_SOURCE_FORBIDDEN",
                    "use the registered service identity",
                ));
            }
            request.identity = source.clone();
            let canonical = encode(&request)?;
            let fingerprint = hex::encode(Sha256::digest(canonical.as_bytes()));
            let prior:Option<(String,String,String)>=tx.query_row("SELECT fingerprint,message_id,state FROM broker_operations WHERE owner=?1 AND source=?2 AND operation_id=?3",params![owner,source,request.operation_id],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?))).optional()?;
            if let Some((previous, message_id, _state)) = prior {
                if previous != fingerprint {
                    return Err(error(
                        "SDK_BROKER_OPERATION_CONFLICT",
                        "operation identifier was already admitted with different content",
                    ));
                }
                json!(OperationReceipt {
                    operation_id: request.operation_id,
                    message_id,
                    state: "DaemonStored".into()
                })
            } else {
                let message_id = hex::encode(Sha256::digest(
                    encode(&(&owner, &source, &request.operation_id))?.as_bytes(),
                ));
                let record = MessageRecord {
                    id: message_id.clone(),
                    source: source.clone(),
                    destination: request.destination,
                    title: request.title,
                    content: request.content,
                    timestamp: now(),
                    direction: "out".into(),
                    fields: request.fields,
                    receipt_status: None,
                };
                let options = encode(&request.options)?;
                let prepared_reserve = canonical.len().saturating_mul(6).saturating_add(65536);
                if prepared_reserve > MAX_BATCH_BYTES {
                    return Err(error("SDK_VALIDATION_BROKER_MESSAGE_TOO_LARGE","request cannot fit its bounded durable signed and propagation representations"));
                }
                let budget: i64 =
                    tx.query_row("SELECT budget FROM broker_meta", [], |r| r.get(0))?;
                physical_admission(&tx, prepared_reserve as u64, budget as u64)?;
                charge(&tx, prepared_reserve as i64)?;
                charge(
                    &tx,
                    (owner.len()
                        + source.len()
                        + request.operation_id.len()
                        + options.len()
                        + fingerprint.len()
                        + message_id.len()
                        + 512) as i64,
                )?;
                insert_with_operation(&tx, &record, None, Some(&request.operation_id))?;
                tx.execute("INSERT INTO broker_operations(owner,source,operation_id,fingerprint,message_id,state,options,prepared,prepared_reserve) VALUES(?1,?2,?3,?4,?5,'queued',?6,NULL,?7)",params![owner,source,request.operation_id,fingerprint,message_id,options,prepared_reserve as i64])?;
                json!(OperationReceipt {
                    operation_id: request.operation_id,
                    message_id,
                    state: "DaemonStored".into()
                })
            }
        }
        BrokerCommand::Reconcile { owner, source, request } => {
            validate_key(&request.operation_id)?;
            let record:Option<(String,String)>=tx.query_row("SELECT message_id,state FROM broker_operations WHERE owner=?1 AND source=?2 AND operation_id=?3",params![owner,source,request.operation_id],|r|Ok((r.get(0)?,r.get(1)?))).optional()?;
            match record {
                Some((message_id, _state)) => json!(Some(OperationReceipt {
                    operation_id: request.operation_id,
                    message_id,
                    state: "DaemonStored".into()
                })),
                None => Value::Null,
            }
        }
        BrokerCommand::Prepared { message_id, mut payload } => {
            let lxmf = payload.get("lxmf_hex").and_then(Value::as_str).ok_or_else(|| {
                error("SDK_VALIDATION_PREPARED_WIRE", "missing signed LXMF bytes")
            })?;
            if lxmf.len() > MAX_BATCH_BYTES {
                return Err(error(
                    "SDK_VALIDATION_PREPARED_WIRE",
                    "signed wire exceeds storage byte bound",
                ));
            }
            let bytes = hex::decode(lxmf)
                .map_err(|e| error("SDK_VALIDATION_PREPARED_WIRE", e.to_string()))?;
            payload["wire_hash"] = json!(hex::encode(Sha256::digest(&bytes)));
            let previous: Option<Option<String>> = tx
                .query_row(
                    "SELECT prepared FROM broker_operations WHERE message_id=?1",
                    [&message_id],
                    |r| r.get(0),
                )
                .optional()?;
            let Some(previous) = previous else {
                return Err(error(
                    "SDK_BROKER_OPERATION_NOT_FOUND",
                    "not a durable outbound operation",
                ));
            };
            let encoded = encode(&payload)?;
            if encoded.len() > MAX_BATCH_BYTES {
                return Err(error(
                    "SDK_STORAGE_BROKER_EVENT_TOO_LARGE",
                    "prepared payload exceeds storage admission bound",
                ));
            }
            if previous.as_deref() == Some(encoded.as_str()) {
                return Ok(json!({"prepared":true}));
            }
            let (reserve,budget):(i64,i64)=tx.query_row("SELECT prepared_reserve,(SELECT budget FROM broker_meta) FROM broker_operations WHERE message_id=?1",[&message_id],|r|Ok((r.get(0)?,r.get(1)?)))?;
            let prior_size = if let Some(previous) = previous {
                if previous == encoded {
                    previous.len() as i64
                } else {
                    let old: Value = decode(&previous)?;
                    if old.get("lxmf_hex") != payload.get("lxmf_hex")
                        || !old.get("propagation").is_some_and(Value::is_null)
                        || payload.get("propagation").is_none_or(Value::is_null)
                    {
                        return Err(error(
                            "SDK_BROKER_OPERATION_CONFLICT",
                            "prepared signed wire payload is immutable",
                        ));
                    }
                    previous.len() as i64
                }
            } else {
                0
            };
            if encoded.len() as i64 > reserve.saturating_add(prior_size) {
                return Err(error(
                    "SDK_STORAGE_BROKER_EVENT_TOO_LARGE",
                    "prepared wire bytes exceeded their reserved capacity",
                ));
            }
            physical_admission(&tx, encoded.len() as u64, budget as u64)?;
            tx.execute("UPDATE broker_meta SET used_bytes=used_bytes-?1", [reserve + prior_size])?;
            charge(&tx, encoded.len() as i64)?;
            // Keep unused reservation for a direct-to-propagation fallback.
            let remaining = reserve.saturating_add(prior_size).saturating_sub(encoded.len() as i64);
            charge(&tx, remaining)?;
            tx.execute(
                "UPDATE broker_operations SET prepared=?1,prepared_reserve=?3 WHERE message_id=?2",
                params![encoded, message_id, remaining],
            )?;
            json!({"prepared":true})
        }
        BrokerCommand::DispatchState { message_id, state } => {
            if !matches!(state.as_str(), "queued" | "scheduled") {
                return Err(error("SDK_VALIDATION_DISPATCH_STATE", "invalid dispatch state"));
            }
            let previous: Option<String> = tx
                .query_row(
                    "SELECT state FROM broker_operations WHERE message_id=?1",
                    [&message_id],
                    |r| r.get(0),
                )
                .optional()?;
            if previous.as_deref()
                != Some(if state == "scheduled" { "queued" } else { "scheduled" })
            {
                return Ok(json!({"claimed":false}));
            }
            mutation_admission(&tx, state == "scheduled")?;
            let changed = if state == "scheduled" {
                tx.execute("UPDATE broker_operations SET state='scheduled' WHERE message_id=?1 AND state='queued'",[&message_id])?
            } else {
                tx.execute("UPDATE broker_operations SET state='queued',next_attempt_at=unixepoch()+5 WHERE message_id=?1 AND state='scheduled'",[&message_id])?
            };
            json!({"claimed":changed==1})
        }
        BrokerCommand::Inbound { record, raw_hex } => {
            let inserted = insert_atomic(&tx, &record, raw_hex.as_deref())?;
            json!({"inserted": inserted})
        }
        BrokerCommand::Resume { owner, destination, request } => {
            validate_key(&request.consumer_id.0)?;
            let (journal, floor, tail): (String, i64, i64) = tx.query_row(
                "SELECT journal_id,floor,next_position-1 FROM broker_meta",
                [],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )?;
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
            let existing = consumer(&tx, &request.consumer_id.0, &owner, &destination)?;
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
                    let count: i64 =
                        tx.query_row("SELECT COUNT(*) FROM broker_consumers", [], |r| r.get(0))?;
                    if count >= 256 {
                        return Err(error(
                            "SDK_STORAGE_BROKER_FULL",
                            "consumer capacity exhausted",
                        ));
                    }
                    charge(
                        &tx,
                        (request.consumer_id.0.len() + owner.len() + destination.len() + 256)
                            as i64,
                    )?;
                    tx.execute(
                        "INSERT INTO broker_consumers VALUES(?1,?2,?3,0,NULL)",
                        params![request.consumer_id.0, owner, destination],
                    )?;
                    0
                }
            };
            json!(BrokerCheckpoint {
                journal_id: JournalId(journal),
                consumer_id: request.consumer_id,
                stored: EventPosition(broker_stored as u64),
                floor: EventPosition(floor as u64),
                tail: EventPosition(tail as u64)
            })
        }
        BrokerCommand::Fetch { owner, destination, request } => {
            let (stored, issued) =
                require_consumer(&tx, &request.consumer_id.0, &owner, &destination)?;
            if let Some(issued) = issued {
                let range: IssuedRange = decode(&issued)?;
                json!(replay_batch(&tx, &request.consumer_id, &destination, range)?)
            } else {
                if request.max_events == 0
                    || request.max_events > MAX_BATCH_EVENTS
                    || request.max_bytes < 4096
                    || request.max_bytes > MAX_BATCH_BYTES
                {
                    return Err(error(
                        "SDK_VALIDATION_BROKER_LIMIT",
                        "invalid fetch count or byte limit",
                    ));
                }
                let count = request.max_events;
                let byte_limit = request.max_bytes;
                let mut events = Vec::new();
                let mut end = stored;
                let journal: String =
                    tx.query_row("SELECT journal_id FROM broker_meta", [], |r| r.get(0))?;
                // Bound scans as well as visible results. Empty batches advance invisible positions.
                let mut stmt = tx.prepare("SELECT position,local_destination,event_type,created_at,payload FROM broker_events WHERE position>?1 ORDER BY position LIMIT ?2")?;
                let mut rows = stmt.query(params![stored, MAX_BATCH_EVENTS as i64])?;
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
                        if bytes + size > MAX_BATCH_BYTES {
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
                let receipt = receipt(
                    &tx,
                    &journal,
                    &request.consumer_id.0,
                    &owner,
                    &destination,
                    stored,
                    end,
                )?;
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
                    charge(&tx, descriptor.len() as i64)?;
                    tx.execute(
                        "UPDATE broker_consumers SET issued=?1 WHERE consumer_id=?2",
                        params![descriptor, request.consumer_id.0],
                    )?;
                }
                json!(batch)
            }
        }
        BrokerCommand::Ack { owner, destination, request } => {
            let (stored, issued) =
                require_consumer(&tx, &request.consumer_id.0, &owner, &destination)?;
            let journal: String =
                tx.query_row("SELECT journal_id FROM broker_meta", [], |r| r.get(0))?;
            if journal != request.journal_id.0 {
                return Err(error(
                    "SDK_BROKER_RECOVERY_REQUIRED",
                    "ACK belongs to another journal",
                ));
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
                &tx,
                &journal,
                &request.consumer_id.0,
                &owner,
                &destination,
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
                mutation_admission(&tx, false)?;
                tx.execute(
                    "UPDATE broker_meta SET used_bytes=used_bytes-?1",
                    [issued.as_ref().expect("issued batch verified").len() as i64],
                )?;
                tx.execute(
                    "UPDATE broker_consumers SET stored=?1,issued=NULL WHERE consumer_id=?2",
                    params![end, request.consumer_id.0],
                )?;
                prune(&tx)?;
            }
            json!({"stored": end.max(stored)})
        }
    };
    tx.commit()?;
    Ok(result)
}
