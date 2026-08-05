impl RpcDaemon {
    pub fn ensure_ticket(
        &self,
        destination: &str,
        ttl_secs: Option<u64>,
    ) -> Result<TicketRecord, std::io::Error> {
        self.issue_ticket(destination, ttl_secs)
    }

    pub fn generate_ticket(
        &self,
        destination: &str,
        ttl_secs: Option<u64>,
    ) -> Result<Option<TicketRecord>, std::io::Error> {
        if self.ticket_interval_active(destination) {
            return Ok(None);
        }
        self.issue_ticket(destination, ttl_secs).map(Some)
    }
}
