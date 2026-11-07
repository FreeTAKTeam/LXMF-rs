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
                super::receipt::ensure_custody_reserve(&tx, &message_id)?;
                tx.commit()?;
                return Ok(json!({"prepared":true}));
            }
            let (reserve,budget):(i64,i64)=tx.query_row("SELECT prepared_reserve,(SELECT budget FROM broker_meta) FROM broker_operations WHERE message_id=?1",[&message_id],|r|Ok((r.get(0)?,r.get(1)?)))?;
            let custody_reserve = super::receipt::required_custody_reserve(&tx, &message_id)?;
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
            if (encoded.len() as i64).saturating_add(custody_reserve)
                > reserve.saturating_add(prior_size)
            {
                return Err(error(
                    "SDK_STORAGE_BROKER_EVENT_TOO_LARGE",
                    "prepared wire bytes would consume propagation custody capacity",
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
        BrokerCommand::ReservePropagationCustody { message_id } => {
            super::receipt::ensure_custody_reserve(&tx, &message_id)?;
            json!({"reserved":true})
        }
        BrokerCommand::PropagationCustody { message_id, representation } => {
            super::receipt::commit_custody(&tx, &message_id, representation)?
        }
        BrokerCommand::Inbound { record, raw_hex } => {
            let inserted = insert_atomic(&tx, &record, raw_hex.as_deref())?;
            json!({"inserted": inserted})
        }
        BrokerCommand::Resume { owner, destination, request } => {
            json!(r3akt_event_broker::storage::resume(&tx, &owner, &destination, request)?)
        }
        BrokerCommand::Fetch { owner, destination, request } => {
            json!(r3akt_event_broker::storage::fetch(&tx, &owner, &destination, request)?)
        }
        BrokerCommand::Ack { owner, destination, request } => {
            let stored = r3akt_event_broker::storage::ack(&tx, &owner, &destination, request)?;
            json!({"stored": stored.0})
        }
    };
    tx.commit()?;
    Ok(result)
}
