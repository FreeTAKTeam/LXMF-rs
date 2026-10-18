use super::*;

pub(super) fn initial_interfaces(
    seeded: Vec<(String, InterfaceRecord, AddressHash)>,
    startup: Option<Vec<ConfiguredInterfaceStartup>>,
) -> HashMap<ManagedInterfaceKey, ManagedHotApplyInterface> {
    let mut managed = seeded
        .into_iter()
        .map(|(key, record, address)| {
            let origin = ManagedInterfaceOrigin::configured(&record);
            (key.into(), ManagedHotApplyInterface::active(record, address, None, origin))
        })
        .collect::<HashMap<ManagedInterfaceKey, _>>();

    if let Some(startup) = startup {
        let startup_key = |record: &InterfaceRecord| {
            hot_apply_interface_key(record).or_else(|| {
                record
                    .name
                    .as_deref()
                    .map(str::trim)
                    .filter(|name| !name.is_empty())
                    .map(str::to_owned)
            })
        };
        let mut counts = HashMap::<String, usize>::new();
        for root in &startup {
            if let Some(key) = startup_key(&root.record) {
                *counts.entry(key).or_default() += 1;
            }
        }
        for root in startup {
            let name = hot_apply_interface_key(&root.record);
            let protected =
                !matches!(root.record.kind.as_str(), "tcp_client" | "tcp_server" | "udp" | "pipe")
                    || name.as_ref().is_none_or(|name| counts.get(name).copied().unwrap_or(0) > 1);
            let key = if protected {
                ManagedInterfaceKey::Startup(root.index)
            } else {
                configured_key(name.as_deref().expect("unambiguous configured key checked"))
            };
            managed.insert(
                key,
                ManagedHotApplyInterface {
                    record: root.record,
                    native: root.native,
                    configured: Some(ConfiguredRecordBinding {
                        recipe_index: root.index,
                        position: root.index,
                    }),
                    origin: if protected {
                        ManagedInterfaceOrigin::StartupOnly
                    } else {
                        ManagedInterfaceOrigin::Configured
                    },
                },
            );
        }
    }
    managed
}

pub(super) async fn run_interface_mutation_worker(
    iface_manager: Arc<tokio::sync::Mutex<InterfaceManager>>,
    mut rx: Receiver<InterfaceHotApplyCommand>,
    mut managed: HashMap<ManagedInterfaceKey, ManagedHotApplyInterface>,
    transport: Option<Arc<Transport>>,
    refreshes: HotApplyRuntimeRefreshes,
    daemon: Option<Weak<RpcDaemon>>,
    discovery: Option<NativeDiscoveryConfig>,
) -> io::Result<()> {
    let mut publication = tokio::time::interval(std::time::Duration::from_secs(2));
    publication.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    let mut discovery = discovery.map(NativeDiscovery::new);
    if let (Some(discovery), Some(transport), Some(daemon)) =
        (discovery.as_mut(), transport.as_ref(), daemon.as_ref().and_then(Weak::upgrade))
    {
        discovery.initial(&iface_manager, &mut managed, transport, &daemon).await;
    }
    let mut monitor = tokio::time::interval_at(
        tokio::time::Instant::now() + std::time::Duration::from_secs(5),
        std::time::Duration::from_secs(5),
    );
    monitor.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    loop {
        let command = tokio::select! {
            command = rx.recv() => match command { Some(command) => command, None => break },
            _ = publication.tick() => {
                if let Some(daemon) = daemon.as_ref().and_then(Weak::upgrade) {
                    discovery_runtime::publish_live_runtime_records(&managed, &daemon);
                    record_publication::publish_control_bindings(&managed, &daemon);
                }
                continue;
            }
            _ = monitor.tick(), if discovery.is_some() => {
                if let (Some(discovery), Some(transport), Some(daemon)) = (
                    discovery.as_mut(), transport.as_ref(), daemon.as_ref().and_then(Weak::upgrade),
                ) {
                    discovery.monitor(&iface_manager, &mut managed, transport, &refreshes, &daemon).await;
                }
                continue;
            }
        };
        match command {
            InterfaceHotApplyCommand::Shutdown => break,
            InterfaceHotApplyCommand::Apply { interfaces, reload_timestamp } => {
                let applied = apply_hot_apply_interface_records(
                    &iface_manager,
                    &mut managed,
                    interfaces,
                    transport.as_ref(),
                    &refreshes,
                    daemon.as_ref(),
                )
                .await;
                if let Some(daemon) = daemon.as_ref().and_then(Weak::upgrade) {
                    daemon.replace_interfaces(applied.clone());
                    daemon.publish_event(rns_rpc::RpcEvent {
                        event_type: "interfaces_updated".to_string(),
                        payload: serde_json::json!({
                            "interfaces": applied, "queued": false, "complete": true,
                        }),
                    });
                    if let Some(timestamp) = reload_timestamp {
                        daemon.publish_event(rns_rpc::RpcEvent {
                            event_type: "config_reloaded".to_string(),
                            payload: serde_json::json!({ "timestamp": timestamp }),
                        });
                    }
                }
            }
            InterfaceHotApplyCommand::Manage { operation, name, configured, reply } => {
                let endpoint =
                    configured.as_ref().and_then(discovery_runtime::record_endpoint_hash);
                let discovered_key = discovery_runtime::key_for_discovered_name(&managed, &name);
                let discovered = discovered_key
                    .as_ref()
                    .and_then(|key| managed.get(key))
                    .and_then(|active| active.origin.endpoint_hash());
                if operation == "detach" {
                    if let (Some(discovery), Some(endpoint)) = (discovery.as_mut(), discovered) {
                        if let Some(key) = &discovered_key {
                            discovery.suppress(endpoint, key);
                        }
                    }
                }
                let result = if operation == "detach" && discovered.is_some() {
                    discovery_runtime::detach_discovered(
                        &iface_manager,
                        &mut managed,
                        &name,
                        transport.as_ref(),
                        daemon.as_ref(),
                    )
                    .await
                } else {
                    named_management::apply(
                        &iface_manager,
                        &mut managed,
                        named_management::NamedInterfaceAction::new(&operation, &name, configured),
                        transport.as_ref(),
                        &refreshes,
                        daemon.as_ref(),
                    )
                    .await
                };
                if let Some(discovery) = discovery.as_mut() {
                    if result.is_ok() {
                        if let Some(endpoint) = endpoint.filter(|_| operation != "detach") {
                            discovery.allow(endpoint);
                        }
                    }
                }
                if reply.send(result).is_err() {
                    log::warn!(
                        "[daemon] named interface management reply was not received operation={operation} name={name}"
                    );
                }
            }
            InterfaceHotApplyCommand::DiscoveryObserved(info) => {
                if let (Some(discovery), Some(transport), Some(daemon)) = (
                    discovery.as_mut(),
                    transport.as_ref(),
                    daemon.as_ref().and_then(Weak::upgrade),
                ) {
                    discovery.admit(&info, &iface_manager, &mut managed, transport, &daemon).await;
                }
            }
        }
        if let Some(daemon) = daemon.as_ref().and_then(Weak::upgrade) {
            record_publication::publish_control_bindings(&managed, &daemon);
        }
    }
    // Once retirement starts, no sender may acknowledge work this owner cannot execute.
    rx.close();
    let mut failure = None;
    for (_, mut active) in managed.drain() {
        if let Some(native) = active.native.take() {
            if let Err(error) =
                discovery_runtime::stop_owned_native(&iface_manager, transport.as_ref(), native)
                    .await
            {
                log::warn!("[daemon] mutation owner shutdown did not confirm worker exit: {error}");
                failure.get_or_insert(error);
            }
        }
    }
    if let Some(daemon) = daemon.as_ref().and_then(Weak::upgrade) {
        record_publication::publish_control_bindings(&managed, &daemon);
    }
    match failure {
        Some(error) => Err(error),
        None => Ok(()),
    }
}
