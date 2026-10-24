use crate::{Error, State};
use rusqlite::{params, Connection, OptionalExtension};

pub(crate) fn retire(conn: &Connection, id: &str, state: State, now: i64) -> Result<(), Error> {
    crate::storage::purge_propagation_entries_for_destination(
        conn,
        &destination(conn, id)?,
        &[id.to_owned()],
    )?;
    conn.execute(
        "UPDATE propagation_owned SET state=?2, recovery=NULL, encrypted_bytes=0, projected=0, changed_at=?3
        WHERE transient_id=?1",
        params![id, state as i64, now],
    )?;
    Ok(())
}

fn destination(conn: &Connection, id: &str) -> rusqlite::Result<String> {
    conn.query_row("SELECT destination FROM propagation_owned WHERE transient_id=?1", [id], |r| {
        r.get(0)
    })
}

pub(crate) fn expire(conn: &Connection, now: i64) -> Result<(), Error> {
    let ids = {
        let mut stmt = conn.prepare(
            "SELECT transient_id FROM propagation_owned WHERE state<4 AND expires_at<=?1",
        )?;
        let rows = stmt.query_map([now], |r| r.get::<_, String>(0))?;
        rows.collect::<rusqlite::Result<Vec<_>>>()?
    };
    for id in ids {
        retire(conn, &id, State::Expired, now)?;
    }
    Ok(())
}

pub(crate) fn supersede(
    conn: &Connection,
    recipient: &str,
    key: &str,
    now: i64,
) -> Result<(), Error> {
    let ids = {
        let mut stmt = conn.prepare(
            "SELECT transient_id FROM propagation_owned
            WHERE state<4 AND kind=2 AND destination=?1 AND replacement_key=?2",
        )?;
        let rows = stmt.query_map(params![recipient, key], |r| r.get::<_, String>(0))?;
        rows.collect::<rusqlite::Result<Vec<_>>>()?
    };
    for id in ids {
        retire(conn, &id, State::Evicted, now)?;
    }
    Ok(())
}

pub(crate) fn make_room(
    conn: &Connection,
    incoming: u64,
    capacity: u64,
    now: i64,
) -> Result<(), Error> {
    loop {
        let (bytes, count): (i64, i64) = conn.query_row(
            "SELECT COALESCE(SUM(encrypted_bytes),0), COUNT(*) FROM propagation_owned WHERE state<4",
            [], |r| Ok((r.get(0)?,r.get(1)?)))?;
        if (bytes as u64).saturating_add(incoming) <= capacity && count < crate::MAX_PENDING as i64
        {
            return Ok(());
        }
        // Within each retention class prefer a recipient occupying at least half the capacity.
        let dominant: Option<String> = conn
            .query_row(
                "SELECT destination FROM propagation_owned WHERE state<4
            GROUP BY destination HAVING SUM(encrypted_bytes)>=?1
            ORDER BY SUM(encrypted_bytes) DESC, destination LIMIT 1",
                [capacity.div_ceil(2) as i64],
                |r| r.get(0),
            )
            .optional()?;
        let id: Option<String> = conn
            .query_row(
                "SELECT transient_id FROM propagation_owned WHERE state<4
            ORDER BY CASE WHEN kind=2 THEN 0 ELSE 1 END,
            CASE WHEN destination=?1 THEN 0 ELSE 1 END, admitted_at, transient_id LIMIT 1",
                [dominant],
                |r| r.get(0),
            )
            .optional()?;
        let id = id.ok_or(Error::StoreFull)?;
        retire(conn, &id, State::Evicted, now)?;
    }
}
