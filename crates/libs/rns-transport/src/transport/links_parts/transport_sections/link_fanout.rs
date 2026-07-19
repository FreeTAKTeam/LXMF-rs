#[derive(Clone, Copy)]
enum LinkPayloadKind {
    Data,
    Channel,
}

impl Transport {
    async fn send_link_payloads(
        &self,
        links: Vec<Arc<Mutex<Link>>>,
        destination: Option<&AddressHash>,
        payload: &[u8],
        kind: LinkPayloadKind,
        direction: &'static str,
    ) -> LinkSendReport {
        let mut report = LinkSendReport::default();
        let mut packets = Vec::new();

        // The handler lock is deliberately released by callers before any
        // link is awaited. Keeping both locks across `.await` makes fan-out
        // vulnerable to lock-order deadlocks with link lifecycle workers.
        for link in links {
            let guard = link.lock().await;
            if guard.status() != LinkStatus::Active
                || destination.is_some_and(|expected| {
                    guard.destination().address_hash != *expected
                })
            {
                continue;
            }

            report.matched_links += 1;
            let link_id = *guard.id();
            let packet = match kind {
                LinkPayloadKind::Data => guard.data_packet(payload),
                LinkPayloadKind::Channel => guard.channel_packet(payload),
            };
            drop(guard);

            match packet {
                Ok(packet) => packets.push((link, link_id, packet)),
                Err(error) => {
                    report.failed_links += 1;
                    log::warn!(
                        "tp({}): failed to build {} packet for {} link {}: {}",
                        self.name,
                        match kind {
                            LinkPayloadKind::Data => "data",
                            LinkPayloadKind::Channel => "channel",
                        },
                        direction,
                        link_id,
                        error
                    );
                }
            }
        }

        for (link, link_id, packet) in packets {
            let outcome = self.send_link_packet_on_bound_iface(&link, packet).await;
            if matches!(outcome, SendPacketOutcome::SentDirect | SendPacketOutcome::SentBroadcast)
            {
                report.sent_links += 1;
            } else {
                report.failed_links += 1;
                log::warn!(
                    "tp({}): failed to dispatch {} packet for {} link {}: {:?}",
                    self.name,
                    match kind {
                        LinkPayloadKind::Data => "data",
                        LinkPayloadKind::Channel => "channel",
                    },
                    direction,
                    link_id,
                    outcome
                );
            }
        }

        report
    }

    async fn out_links_snapshot(&self) -> Vec<Arc<Mutex<Link>>> {
        self.handler.lock().await.out_links.values().cloned().collect()
    }

    async fn in_links_snapshot(&self) -> Vec<Arc<Mutex<Link>>> {
        self.handler.lock().await.in_links.values().cloned().collect()
    }

    pub async fn send_to_all_out_links_with_report(&self, payload: &[u8]) -> LinkSendReport {
        self.send_link_payloads(
            self.out_links_snapshot().await,
            None,
            payload,
            LinkPayloadKind::Data,
            "outbound",
        )
        .await
    }

    pub async fn send_channel_to_all_out_links_with_report(
        &self,
        payload: &[u8],
    ) -> LinkSendReport {
        self.send_link_payloads(
            self.out_links_snapshot().await,
            None,
            payload,
            LinkPayloadKind::Channel,
            "outbound",
        )
        .await
    }

    pub async fn send_to_out_links_with_report(
        &self,
        destination: &AddressHash,
        payload: &[u8],
    ) -> LinkSendReport {
        self.send_link_payloads(
            self.out_links_snapshot().await,
            Some(destination),
            payload,
            LinkPayloadKind::Data,
            "outbound",
        )
        .await
    }

    pub async fn send_channel_to_out_links_with_report(
        &self,
        destination: &AddressHash,
        payload: &[u8],
    ) -> LinkSendReport {
        self.send_link_payloads(
            self.out_links_snapshot().await,
            Some(destination),
            payload,
            LinkPayloadKind::Channel,
            "outbound",
        )
        .await
    }

    pub async fn send_to_in_links_with_report(
        &self,
        destination: &AddressHash,
        payload: &[u8],
    ) -> LinkSendReport {
        self.send_link_payloads(
            self.in_links_snapshot().await,
            Some(destination),
            payload,
            LinkPayloadKind::Data,
            "inbound",
        )
        .await
    }

    pub async fn send_channel_to_in_links_with_report(
        &self,
        destination: &AddressHash,
        payload: &[u8],
    ) -> LinkSendReport {
        self.send_link_payloads(
            self.in_links_snapshot().await,
            Some(destination),
            payload,
            LinkPayloadKind::Channel,
            "inbound",
        )
        .await
    }

    /// Sends data to every active outbound link. Failures are logged; callers
    /// that need structured partial-success information should use
    /// [`Self::send_to_all_out_links_with_report`].
    pub async fn send_to_all_out_links(&self, payload: &[u8]) {
        self.send_to_all_out_links_with_report(payload).await;
    }

    pub async fn send_channel_to_all_out_links(&self, payload: &[u8]) {
        self.send_channel_to_all_out_links_with_report(payload).await;
    }

    pub async fn send_to_out_links(&self, destination: &AddressHash, payload: &[u8]) {
        let report = self.send_to_out_links_with_report(destination, payload).await;
        if report.matched_links == 0 {
            log::trace!("tp({}): no output links for {} destination", self.name, destination);
        }
    }

    pub async fn send_channel_to_out_links(&self, destination: &AddressHash, payload: &[u8]) {
        let report = self.send_channel_to_out_links_with_report(destination, payload).await;
        if report.matched_links == 0 {
            log::trace!("tp({}): no output links for {} destination", self.name, destination);
        }
    }

    pub async fn send_to_in_links(&self, destination: &AddressHash, payload: &[u8]) {
        let report = self.send_to_in_links_with_report(destination, payload).await;
        if report.matched_links == 0 {
            log::trace!("tp({}): no input links for {} destination", self.name, destination);
        }
    }

    pub async fn send_channel_to_in_links(&self, destination: &AddressHash, payload: &[u8]) {
        let report = self.send_channel_to_in_links_with_report(destination, payload).await;
        if report.matched_links == 0 {
            log::trace!("tp({}): no input links for {} destination", self.name, destination);
        }
    }
}
