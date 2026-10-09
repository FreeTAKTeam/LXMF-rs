impl MessagesStore {
    /// Must be called at startup before accepting traffic. Reopening preserves the profile.
    pub fn enable_durable_broker(&self, budget_bytes: i64) -> rusqlite::Result<()> {
        self.with_control_conn(|conn| super::broker::enable(conn, budget_bytes))
    }
    pub fn durable_broker_enabled(&self) -> rusqlite::Result<bool> {
        self.with_read_conn(super::broker::enabled)
    }
    pub fn broker_command(&self, command: super::broker::BrokerCommand) -> rusqlite::Result<JsonValue> {
        let (reply, receive) = mpsc::channel();
        self.outbound_write_tx.send(OutboundWriteCommand::Broker { command: Box::new(command), reply })
            .map_err(|e| rusqlite::Error::ToSqlConversionFailure(Box::new(e)))?;
        receive.recv().map_err(|e| rusqlite::Error::ToSqlConversionFailure(Box::new(e)))?
    }
}

impl MessagesStore {
    pub fn broker_owns_message(&self,id:&str)->rusqlite::Result<bool> {
        if !self.durable_broker_enabled()? { return Ok(false); }
        self.with_read_conn(|c|c.query_row("SELECT EXISTS(SELECT 1 FROM broker_operations WHERE message_id=?1)",[id],|r|r.get(0)))
    }
    pub fn broker_prepared(&self,id:&str)->rusqlite::Result<Option<Option<JsonValue>>> {
        self.with_read_conn(|conn| {
            let row:Option<Option<String>>=conn.query_row("SELECT prepared FROM broker_operations WHERE message_id=?1",[id],|r|r.get(0)).optional()?;
            row.map(|value|value.map(|s|serde_json::from_str(&s).map_err(|e|rusqlite::Error::ToSqlConversionFailure(Box::new(e)))).transpose()).transpose()
        })
    }
    pub fn broker_pending_dispatch(&self)->rusqlite::Result<Vec<(MessageRecord,crate::rpc::OutboundDeliveryOptions)>> {
        if !self.durable_broker_enabled()? {return Ok(Vec::new());}
        self.with_read_conn(|conn| {
            let mut stmt=conn.prepare("SELECT m.id,m.source,m.destination,m.title,m.content,m.timestamp,m.direction,m.fields,m.receipt_status,o.options FROM broker_operations o JOIN messages m ON m.id=o.message_id WHERE o.state='queued' AND o.next_attempt_at<=unixepoch() ORDER BY m.timestamp,m.id LIMIT 1")?;
            let result=stmt.query_map([],|row| {
                let options:String=row.get(9)?;
                Ok((super::broker::record_from_row(row)?,serde_json::from_str(&options).map_err(|e|rusqlite::Error::ToSqlConversionFailure(Box::new(e)))?))
            })?.collect();
            result
        })
    }
    /// Called once at daemon startup; scheduled external sends have Unknown execution certainty.
    pub fn broker_recover_dispatch(&self)->rusqlite::Result<()> {
        if self.durable_broker_enabled()? {self.with_write_conn(|conn|conn.execute("UPDATE broker_operations SET state='queued' WHERE state='scheduled'",[]).map(|_|()))?;}
        Ok(())
    }
}

impl MessagesStore {
    pub fn broker_message_owner(&self,id:&str)->rusqlite::Result<Option<(String,String)>> {
        self.with_read_conn(|c|c.query_row("SELECT owner,source FROM broker_operations WHERE message_id=?1",[id],|r|Ok((r.get(0)?,r.get(1)?))).optional())
    }
}

impl MessagesStore {
    pub fn broker_inbound(&self,record:&MessageRecord,raw:Option<&[u8]>)->rusqlite::Result<JsonValue> {
        if raw.is_some_and(|bytes|bytes.len()>4*1024*1024) {return Err(super::broker::error("SDK_STORAGE_BROKER_EVENT_TOO_LARGE","raw LXMF bytes exceed durable input bound"));}
        let bytes=message_retained_bytes(record).saturating_add(raw.map_or(0,|b|(b.len() as u64).saturating_mul(2))).saturating_add(512);
        let reservation=self.outbound_write_tx.reserve(bytes).map_err(|e|rusqlite::Error::ToSqlConversionFailure(Box::new(e)))?;
        let (reply,rx)=mpsc::channel();
        self.outbound_write_tx.send_reserved(OutboundWriteCommand::Broker {command:Box::new(super::broker::BrokerCommand::Inbound {record:record.clone(),raw_hex:raw.map(hex::encode)}),reply},reservation).map_err(|e|rusqlite::Error::ToSqlConversionFailure(Box::new(e)))?;
        rx.recv().map_err(|e|rusqlite::Error::ToSqlConversionFailure(Box::new(e)))?
    }
}

impl MessagesStore {
    /// Startup-only, explicit restore declaration. This invalidates old issued receipts;
    /// it preserves message/operation IDs and reports recovery rather than replaying a tail.
    pub fn declare_broker_backup_restore(&self) -> rusqlite::Result<()> {
        use rand_core::RngCore;
        self.with_write_conn(|conn| {
            if !super::broker::enabled(conn)? { return Err(super::broker::error("SDK_BROKER_DISABLED","restore declaration requires durable storage")); }
            let tx=conn.unchecked_transaction()?;
            let mut secret=[0u8;32];rand_core::OsRng.fill_bytes(&mut secret);
            let mut journal=[0u8;16];rand_core::OsRng.fill_bytes(&mut journal);
            tx.execute("UPDATE broker_meta SET journal_id=?1,secret=?2",params![hex::encode(journal),secret.as_slice()])?;
            tx.execute("UPDATE broker_consumers SET issued=NULL",[])?;
            tx.commit()
        })
    }
}
