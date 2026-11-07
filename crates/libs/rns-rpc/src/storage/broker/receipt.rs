use super::*;

fn receipt_payload(
    id: &str,
    source: &str,
    destination: &str,
    direction: &str,
    status: &str,
    truncated: bool,
) -> Value {
    json!({"message_id":id,"status":status,"reason_truncated":truncated,"message":{"id":id,"source":source,"destination":destination,"direction":direction}})
}

struct ReceiptRecord {
    source: String,
    destination: String,
    direction: String,
    status: Option<String>,
}

fn receipt_record(conn: &Connection, id: &str) -> rusqlite::Result<ReceiptRecord> {
    conn.query_row(
        "SELECT source,destination,direction,receipt_status FROM messages WHERE id=?1",
        [id],
        |r| {
            Ok(ReceiptRecord {
                source: r.get(0)?,
                destination: r.get(1)?,
                direction: r.get(2)?,
                status: r.get(3)?,
            })
        },
    )
}

fn custody_payload(id: &str, record: &ReceiptRecord, representation: PropagationCustody) -> Value {
    let mut payload = receipt_payload(
        id,
        &record.source,
        &record.destination,
        &record.direction,
        representation.status(),
        false,
    );
    payload["dispatch_complete"] = json!("propagation_custody");
    payload
}

pub(super) fn required_custody_reserve(conn: &Connection, id: &str) -> rusqlite::Result<i64> {
    let record = receipt_record(conn, id)?;
    let destination = record.source.to_ascii_lowercase();
    [PropagationCustody::Packet, PropagationCustody::Resource]
        .into_iter()
        .map(|representation| {
            encode(&custody_payload(id, &record, representation))
                .map(|payload| event_bytes(&destination, "receipt", &payload))
        })
        .try_fold(0, |max, bytes| bytes.map(|bytes| max.max(bytes)))
}

/// Repair older reservations through the shared writer before exposing restored bytes.
pub(super) fn ensure_custody_reserve(conn: &Connection, id: &str) -> rusqlite::Result<()> {
    let operation: Option<(String, i64)> = conn
        .query_row(
            "SELECT state,prepared_reserve FROM broker_operations WHERE message_id=?1",
            [id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()?;
    if let Some((state, reserve)) = operation {
        if matches!(state.as_str(), "queued" | "scheduled") {
            reserve_shortfall(conn, id, reserve, required_custody_reserve(conn, id)?)?;
        }
    }
    Ok(())
}

fn reserve_shortfall(
    conn: &Connection,
    id: &str,
    reserve: i64,
    required: i64,
) -> rusqlite::Result<i64> {
    let shortfall = required.saturating_sub(reserve).max(0);
    if shortfall > 0 {
        let budget: i64 = conn.query_row("SELECT budget FROM broker_meta", [], |r| r.get(0))?;
        physical_admission(conn, shortfall as u64, budget as u64)?;
        charge(conn, shortfall)?;
        conn.execute(
            "UPDATE broker_operations SET prepared_reserve=prepared_reserve+?1 WHERE message_id=?2",
            params![shortfall, id],
        )?;
    }
    Ok(reserve + shortfall)
}

pub(super) fn commit_custody(
    conn: &Connection,
    id: &str,
    representation: PropagationCustody,
) -> rusqlite::Result<Value> {
    let record = receipt_record(conn, id)?;
    let (state, reserve): (String, i64) = conn.query_row(
        "SELECT state,prepared_reserve FROM broker_operations WHERE message_id=?1",
        [id],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )?;
    if state == "custody"
        || state == "terminal"
        || record
            .status
            .as_deref()
            .is_some_and(crate::storage::messages::MessagesStore::is_terminal_receipt_status)
    {
        return Ok(json!({"committed":false,"status":record.status}));
    }
    if record.direction != "out" || !matches!(state.as_str(), "queued" | "scheduled") {
        return Err(error(
            "SDK_VALIDATION_DISPATCH_STATE",
            "propagation custody requires an outbound dispatch operation",
        ));
    }
    let destination = record.source.to_ascii_lowercase();
    let payload = custody_payload(id, &record, representation);
    // Prior-run transfers can arrive before a restored preparation is read. Repair
    // only this actual event's shortfall, atomically with its custody commit.
    let reserve = reserve_shortfall(
        conn,
        id,
        reserve,
        event_bytes(&destination, "receipt", &encode(&payload)?),
    )?;
    mutation_admission(conn, false)?;
    conn.execute(
        "UPDATE messages SET receipt_status=?1 WHERE id=?2",
        params![representation.status(), id],
    )?;
    conn.execute("UPDATE broker_meta SET used_bytes=used_bytes-?1", [reserve])?;
    conn.execute(
        "UPDATE broker_operations SET state='custody',prepared_reserve=0 WHERE message_id=?1",
        [id],
    )?;
    // The prepared payload remains charged. Recipient-terminal capacity remains
    // in broker_completions; this dispatch completion spends only its own reserve.
    append_budgeted(conn, &destination, "receipt", &payload, true)?;
    Ok(json!({"committed":true,"status":representation.status()}))
}

pub(crate) fn status_atomic(conn: &Connection, id: &str, status: &str) -> rusqlite::Result<()> {
    let record = conn.query_row("SELECT id,source,destination,title,content,timestamp,direction,fields,receipt_status FROM messages WHERE id=?1",[id],record_from_row).optional()?;
    if let Some(mut record) = record {
        if record.receipt_status.as_deref().is_some_and(|existing| {
            existing == status
                || crate::storage::messages::MessagesStore::should_preserve_receipt_status(
                    existing, status,
                )
        }) {
            return Ok(());
        }
        record.receipt_status = Some(status.into());
        let terminal = crate::storage::messages::MessagesStore::is_terminal_receipt_status(status);
        let reservation: Option<i64> = conn
            .query_row("SELECT bytes FROM broker_completions WHERE message_id=?1", [id], |r| {
                r.get(0)
            })
            .optional()?;
        mutation_admission(conn, !(terminal && reservation.is_some()))?;
        conn.execute("UPDATE messages SET receipt_status=?1 WHERE id=?2", params![status, id])?;
        if terminal {
            let unused: Option<i64> = conn
                .query_row(
                    "SELECT prepared_reserve FROM broker_operations WHERE message_id=?1",
                    [id],
                    |r| r.get(0),
                )
                .optional()?;
            if let Some(unused) = unused {
                conn.execute("UPDATE broker_meta SET used_bytes=used_bytes-?1", [unused])?;
            }
            conn.execute("UPDATE broker_operations SET state='terminal',prepared_reserve=0 WHERE message_id=?1",[id])?;
            if let Some(bytes) = reservation {
                conn.execute("UPDATE broker_meta SET used_bytes=used_bytes-?1", [bytes])?;
                conn.execute("DELETE FROM broker_completions WHERE message_id=?1", [id])?;
            }
        }
        let reason: String = status.chars().take(512).collect();
        let payload = receipt_payload(
            id,
            &record.source,
            &record.destination,
            &record.direction,
            &reason,
            reason.len() != status.len(),
        );
        append_budgeted(
            conn,
            &local_destination(&record),
            "receipt",
            &payload,
            terminal && reservation.is_some(),
        )?;
    }
    Ok(())
}
