// Outbound Link selection, publication and establishment timing.

impl Transport {
    /// Reuses the current non-Closed outbound Link or publishes one replacement.
    /// Concurrent callers share the published Link; explicit reset can still
    /// invalidate a returned handle, whose native lifecycle remains authoritative.
    pub async fn link(&self, destination: DestinationDesc) -> Arc<Mutex<Link>> {
        self.link_observed(destination, |_| {}).await
    }

    /// Observe only a newly published Link, before its first request is handed
    /// off. Reuse and losing concurrent candidates never invoke the observer.
    /// The observer must not re-enter Transport.
    pub async fn link_observed(
        &self,
        destination: DestinationDesc,
        observe: impl FnOnce(&mut Link),
    ) -> Arc<Mutex<Link>> {
        let mut observe = Some(observe);
        loop {
            let observed =
                self.handler.lock().await.out_links.get(&destination.address_hash).cloned();
            if let Some(link) = observed.as_ref() {
                let status = link.lock().await.status();
                if status != LinkStatus::Closed {
                    let handler = self.handler.lock().await;
                    if handler
                        .out_links
                        .get(&destination.address_hash)
                        .is_some_and(|current| Arc::ptr_eq(current, link))
                    {
                        return link.clone();
                    }
                    continue;
                }
            }

            let mut candidate = Link::new(destination, self.link_out_event_tx.clone());
            let (next_hop_iface, hops) = {
                let handler = self.handler.lock().await;
                (
                    handler.path_table.next_hop_iface(&destination.address_hash),
                    handler.path_table.hops_to(&destination.address_hash),
                )
            };
            let next_hop_mtu = match next_hop_iface {
                Some(iface) => self.iface_manager.lock().await.mtu(&iface),
                None => None,
            };
            candidate.set_establishment_timeout(
                self.establishment_timeout_for(&destination.address_hash, next_hop_mtu, hops).await,
            );
            let packet = match next_hop_mtu {
                Some(mtu) => candidate.request_with_mtu(mtu),
                None => candidate.request(),
            };

            let mut handler = self.handler.lock().await;
            let unchanged =
                match (handler.out_links.get(&destination.address_hash), observed.as_ref()) {
                    (None, None) => true,
                    (Some(current), Some(previous)) => Arc::ptr_eq(current, previous),
                    _ => false,
                };
            if !unchanged {
                continue;
            }
            // A public restart can revive the same Arc during preparation. Keep
            // its status stable through publication, without awaiting under handler.
            let mut previous_guard = if let Some(previous) = observed.as_ref() {
                match previous.try_lock() {
                    Ok(guard) => Some(guard),
                    Err(_) => {
                        drop(handler);
                        drop(previous.lock().await);
                        continue;
                    }
                }
            } else {
                None
            };
            if let (Some(previous), Some(guard)) = (observed.as_ref(), previous_guard.as_ref()) {
                if guard.status() != LinkStatus::Closed {
                    return previous.clone();
                }
            }

            if let Some(previous) = previous_guard.as_deref_mut() {
                super::requests::retire_closed_link(&mut handler, previous);
            }

            log::debug!(
                "[tp-diag] create_out_link destination={} link_id={}",
                destination.address_hash,
                candidate.id()
            );
            observe.take().expect("publication invokes observer once")(&mut candidate);
            let link = Arc::new(Mutex::new(candidate));
            handler.out_links.insert(destination.address_hash, link.clone());
            drop(previous_guard);
            // Keep publication and initial dispatch under the existing send lock:
            // reset/replacement cannot dispatch an orphan candidate in between.
            handler.send_packet(packet).await;
            return link;
        }
    }

    /// How long an outbound link to `destination` may stay unestablished,
    /// sized as `RNS.Link.__init__` sizes it: the first hop's own timeout,
    /// then [`crate::destination::link::ESTABLISHMENT_TIMEOUT_PER_HOP`] for
    /// every hop to the destination.
    async fn establishment_timeout_for(
        &self,
        destination: &AddressHash,
        next_hop_mtu: Option<usize>,
        hops: u8,
    ) -> Duration {
        let per_hop = crate::destination::link::ESTABLISHMENT_TIMEOUT_PER_HOP;
        let first_hop_timeout = match self.next_hop_metrics(destination).await {
            Some(metrics) => metrics
                .first_hop_timeout(next_hop_mtu.unwrap_or(crate::packet::PACKET_MDU), per_hop),
            None => per_hop,
        };
        first_hop_timeout + per_hop * u32::from(hops.max(1))
    }
}
