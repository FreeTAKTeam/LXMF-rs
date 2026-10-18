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
            if let Some(active) = managed.get_mut(key) {
                if let Some(native) = active.native.take() {
                    if let Err(error) = stop_owned_native(manager, Some(transport), native).await {
                        log::warn!("[daemon] bootstrap worker stop failed name={name}: {error}");
                    }
                }
                active.record.enabled = false;
                record_publication::clear_inactive_runtime(
                    &mut active.record,
                    "detached",
                    "stopped",
                    None,
                );
                #[cfg(test)]
                if let Some(name) = key.configured_name() {
                    refreshes
                        .tcp_listeners
                        .lock()
                        .expect("tcp listener refresh mutex poisoned")
                        .remove(name);
                    refreshes.udp.lock().expect("udp refresh mutex poisoned").remove(name);
                    refreshes.pipe.lock().expect("pipe refresh mutex poisoned").remove(name);
                }
            }
        }
        record_publication::publish_control_bindings(managed, daemon);

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
        for (index, iface) in &self.config.bootstrap {
            let record = crate::bootstrap::interface_record_from_config(iface);
            let name = crate::interfaces::common::interface_label(iface, *index);
            let key = managed
                .iter()
                .find_map(|(key, entry)| {
                    entry
                        .configured
                        .as_ref()
                        .filter(|binding| binding.recipe_index == *index)
                        .map(|_| key.clone())
                })
                .or_else(|| hot_apply_interface_seed_key(&record).map(|name| configured_key(&name)))
                .unwrap_or(ManagedInterfaceKey::Startup(*index));
            if managed.get(&key).is_some_and(|entry| entry.native.is_some()) {
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
                if let Some(entry) = managed.get_mut(&key) {
                    let mut restored = record.clone();
                    mark_interface_startup_status(
                        &mut restored,
                        "spawned",
                        None,
                        Some(&address.to_string()),
                    );
                    mark_interface_runtime_fields(&mut restored, "running", 0);
                    mark_interface_runtime_managed(&mut restored, "daemon_transport");
                    entry.record = restored;
                    entry.native = Some(NativeInterfaceRuntime::new(address, runtime_status));
                } else {
                    let origin = ManagedInterfaceOrigin::configured(&record);
                    managed.insert(
                        key,
                        ManagedHotApplyInterface::active(
                            record.clone(),
                            address,
                            runtime_status,
                            origin,
                        ),
                    );
                }

                publish_live_runtime_records(managed, daemon);
            }
        }
    }
}
