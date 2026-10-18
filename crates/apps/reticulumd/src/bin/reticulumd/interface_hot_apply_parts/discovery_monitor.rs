use super::*;

impl NativeDiscovery {
    pub(in crate::interface_hot_apply) async fn monitor(
        &mut self,
        manager: &Arc<tokio::sync::Mutex<InterfaceManager>>,
        managed: &mut HashMap<ManagedInterfaceKey, ManagedHotApplyInterface>,
        transport: &Arc<Transport>,
        refreshes: &HotApplyRuntimeRefreshes,
        daemon: &Arc<RpcDaemon>,
    ) {
        if transport.connected_to_shared_instance().await {
            return;
        }
        let snapshot = runtime_states(manager, managed).await;
        let now = snapshot.observed_at;
        let mut states = snapshot.interfaces;
        let keys = snapshot.keys;
        for state in &mut states {
            if let Some(ManagedHotApplyInterface {
                origin: ManagedInterfaceOrigin::Discovered { down_since, .. },
                ..
            }) = keys.get(&state.id).and_then(|key| managed.get(key))
            {
                state.down_since =
                    down_since.map(|since| since.duration_since(self.started).as_secs());
            }
        }
        let plan = self.lifecycle.monitor(
            now.duration_since(self.started).as_secs(),
            &mut states,
            self.config.maximum,
        );
        for state in &states {
            if let Some(ManagedHotApplyInterface {
                origin: ManagedInterfaceOrigin::Discovered { down_since, .. },
                ..
            }) = keys.get(&state.id).and_then(|key| managed.get_mut(key))
            {
                if state.online {
                    *down_since = None;
                } else {
                    down_since.get_or_insert(now);
                }
            }
        }
        for name in &plan.recovered {
            log::info!("[daemon] discovered interface reconnected name={name}");
        }
        for name in &plan.marked_down {
            log::debug!("[daemon] discovered interface disconnected name={name}");
        }
        if plan.enable_bootstrap {
            self.restore_bootstrap(manager, managed, transport, refreshes, daemon).await;
        }
        // The target selects one candidate before processing this tick's detach list.
        if self.lifecycle.initial_autoconnect_ran() && plan.free_slots > plan.reserved_slots {
            let candidates = self.candidates(true);
            if !candidates.is_empty() {
                // Rejection sampling keeps the choice uniform without another dependency.
                let length = candidates.len() as u64;
                let ceiling = u64::MAX - u64::MAX % length;
                let mut sample = OsRng.next_u64();
                while sample >= ceiling {
                    sample = OsRng.next_u64();
                }
                self.admit(
                    &candidates[(sample % length) as usize],
                    manager,
                    managed,
                    transport,
                    daemon,
                )
                .await;
            }
        }
        for name in plan.disable_bootstrap {
            let Some(key) = keys.get(&name) else {
                continue;
            };
            if let Some(active) = managed.remove(key) {
                if let Err(error) = stop_and_join(manager, Some(transport), active.address).await {
                    log::warn!("[daemon] bootstrap worker stop failed name={name}: {error}");
                }
                let mut records = daemon.interface_records();
                if let Some(record) =
                    records.iter_mut().find(|record| same_record(record, &active.record))
                {
                    record.enabled = false;
                    record_publication::committed_records(std::slice::from_mut(record), managed);
                }
                daemon.replace_interfaces(records);
                refreshes
                    .tcp_listeners
                    .lock()
                    .expect("tcp listener refresh mutex poisoned")
                    .remove(
                        key.configured_name().expect("bootstrap interfaces retain configured keys"),
                    );
                refreshes.udp.lock().expect("udp refresh mutex poisoned").remove(
                    key.configured_name().expect("bootstrap interfaces retain configured keys"),
                );
                refreshes.pipe.lock().expect("pipe refresh mutex poisoned").remove(
                    key.configured_name().expect("bootstrap interfaces retain configured keys"),
                );
            }
        }
        for name in plan.detach {
            // Integer projection can round the planner's boundary forward; the native
            // monotonic anchor remains the authority for the full twelve seconds.
            let elapsed = keys.get(&name).and_then(|key| managed.get(key)).and_then(|active| {
                match active.origin {
                    ManagedInterfaceOrigin::Discovered { down_since: Some(since), .. } => {
                        Some(now.duration_since(since))
                    }
                    _ => None,
                }
            });
            if elapsed.is_some_and(|elapsed| elapsed >= Duration::from_secs(12)) {
                let Some(public_name) = keys
                    .get(&name)
                    .and_then(|key| managed.get(key))
                    .and_then(|active| active.record.name.clone())
                else {
                    continue;
                };
                match detach_discovered(
                    manager,
                    managed,
                    &public_name,
                    Some(transport),
                    Some(&Arc::downgrade(daemon)),
                )
                .await
                {
                    Ok(_) => {}
                    Err(error) => log::warn!(
                        "[daemon] automatic discovery detach failed name={name}: {error}"
                    ),
                }
                self.lifecycle.teardown_interface(&name);
            }
        }
        publish_live_runtime_records(managed, daemon);
    }

    async fn restore_bootstrap(
        &self,
        manager: &Arc<tokio::sync::Mutex<InterfaceManager>>,
        managed: &mut HashMap<ManagedInterfaceKey, ManagedHotApplyInterface>,
        transport: &Arc<Transport>,
        refreshes: &HotApplyRuntimeRefreshes,
        daemon: &Arc<RpcDaemon>,
    ) {
        for iface in &self.config.bootstrap {
            let record = crate::bootstrap::interface_record_from_config(iface);
            let Some(name) = hot_apply_interface_seed_key(&record) else {
                continue;
            };
            let key = configured_key(&name);
            if managed.contains_key(&key) {
                continue;
            }
            let spawned = match iface.kind.as_str() {
                "tcp_client" | "backbone_client" => {
                    spawn_interface::spawn_trusted_stream(manager, iface).await
                }
                "tcp_server" | "udp" | "pipe" => {
                    spawn_hot_apply_interface(manager, Some(transport), &record).await
                }
                _ => {
                    // This family still needs its trusted recipe in the item-4 composition gate.
                    log::warn!(
                        "[daemon] bootstrap restore requires family recipe name={name} kind={}",
                        iface.kind
                    );
                    continue;
                }
            };
            if let Some((address, runtime_status)) = spawned {
                apply_records::register_runtime_refresh(
                    &name,
                    &record,
                    address,
                    runtime_status.as_ref(),
                    refreshes,
                    Some(&Arc::downgrade(daemon)),
                );
                let origin = ManagedInterfaceOrigin::configured(&record);
                managed.insert(
                    key,
                    ManagedHotApplyInterface {
                        record: record.clone(),
                        address,
                        runtime_status,
                        origin,
                    },
                );
                let mut changed = vec![record.clone()];
                record_publication::committed_records(&mut changed, managed);
                let mut records = daemon.interface_records();
                if let Some(current) =
                    records.iter_mut().find(|current| same_record(current, &record))
                {
                    *current = changed.remove(0);
                } else {
                    records.extend(changed);
                }
                daemon.replace_interfaces(records);
            }
        }
    }
}
