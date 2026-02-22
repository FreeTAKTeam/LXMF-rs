use super::*;

impl WorkerState {
    pub(super) async fn initialize(init: WorkerInit) -> Result<Self, LxmfError> {
        let identity_path = resolve_identity_path(&init.settings, &init.paths);
        drop_empty_identity_stub(&identity_path)?;
        let identity = load_or_create_identity(&identity_path)?;
        let identity_hash = hex::encode(identity.address_hash().as_slice());

        let db_path = init
            .settings
            .db_path
            .as_ref()
            .map(PathBuf::from)
            .unwrap_or_else(|| init.paths.daemon_db.clone());
        let store = MessagesStore::open(&db_path).map_err(|err| LxmfError::Io(err.to_string()))?;

        let mut configured_interfaces =
            init.interfaces.iter().cloned().map(interface_to_rpc).collect::<Vec<_>>();

        let peer_identity_cache_path = init.paths.root.join("peer_identities.json");
        let peer_crypto: Arc<Mutex<HashMap<String, PeerCrypto>>> =
            Arc::new(Mutex::new(HashMap::new()));
        if let Ok(restored) = load_peer_identity_cache(&peer_identity_cache_path) {
            if let Ok(mut guard) = peer_crypto.lock() {
                *guard = restored;
            }
        }
        let peer_announce_meta: Arc<Mutex<HashMap<String, PeerAnnounceMeta>>> =
            Arc::new(Mutex::new(HashMap::new()));
        let selected_propagation_node: Arc<Mutex<Option<String>>> = Arc::new(Mutex::new(None));
        let known_propagation_nodes: Arc<Mutex<HashSet<String>>> =
            Arc::new(Mutex::new(HashSet::new()));
        let receipt_map: Arc<Mutex<HashMap<String, String>>> = Arc::new(Mutex::new(HashMap::new()));
        let outbound_resource_map: Arc<Mutex<HashMap<String, String>>> =
            Arc::new(Mutex::new(HashMap::new()));
        let delivered_messages: Arc<Mutex<HashSet<String>>> = Arc::new(Mutex::new(HashSet::new()));
        let last_announce_epoch_secs = Arc::new(AtomicU64::new(0));
        let (receipt_tx, receipt_rx) = unbounded_channel();
        let (shutdown_tx, _) = watch::channel(false);

        let mut transport: Option<Arc<Transport>> = None;
        let mut announce_targets: Vec<AnnounceTarget> = Vec::new();
        let mut delivery_destination_hash_hex: Option<String> = None;
        let mut delivery_source_hash = [0u8; 16];
        let normalized_display_name = init
            .settings
            .display_name
            .as_ref()
            .and_then(|value| normalize_display_name(value).ok());

        if let Some(bind) = init.transport.clone() {
            // Embedded desktop runtime should behave as an endpoint, not a transit router.
            // Keep announce/path functionality, but avoid rebroadcasting arbitrary transit traffic.
            let mut transport_instance =
                Transport::new(TransportConfig::new("embedded", &identity, false));
            transport_instance
                .set_receipt_handler(Box::new(ReceiptBridge::new(
                    receipt_map.clone(),
                    delivered_messages.clone(),
                    receipt_tx.clone(),
                )))
                .await;

            let iface_manager = transport_instance.iface_manager();
            iface_manager
                .lock()
                .await
                .spawn(TcpServer::new(bind.clone(), iface_manager.clone()), TcpServer::spawn);

            for entry in &init.interfaces {
                if !entry.enabled || entry.kind != "tcp_client" {
                    continue;
                }
                let Some(host) = entry.host.as_ref() else {
                    continue;
                };
                let Some(port) = entry.port else {
                    continue;
                };
                let addr = format!("{host}:{port}");
                iface_manager.lock().await.spawn(TcpClient::new(addr), TcpClient::spawn);
            }

            if let Some((host, port)) = parse_bind_host_port(&bind) {
                configured_interfaces.push(InterfaceRecord {
                    kind: "tcp_server".into(),
                    enabled: true,
                    host: Some(host),
                    port: Some(port),
                    name: Some("embedded-transport".into()),
                });
            }

            let destination = transport_instance
                .add_destination(identity.clone(), DestinationName::new("lxmf", "delivery"))
                .await;
            {
                let dest = destination.lock().await;
                delivery_source_hash.copy_from_slice(dest.desc.address_hash.as_slice());
                delivery_destination_hash_hex =
                    Some(hex::encode(dest.desc.address_hash.as_slice()));
            }

            let delivery_app_data =
                normalized_display_name.as_deref().and_then(encode_delivery_display_name_app_data);
            announce_targets.push(AnnounceTarget { destination, app_data: delivery_app_data });

            let propagation_destination = transport_instance
                .add_destination(identity.clone(), DestinationName::new("lxmf", "propagation"))
                .await;
            let propagation_app_data =
                encode_propagation_node_app_data(normalized_display_name.as_deref());
            announce_targets.push(AnnounceTarget {
                destination: propagation_destination,
                app_data: propagation_app_data,
            });
            transport = Some(Arc::new(transport_instance));
        }

        let bridge: Option<Arc<EmbeddedTransportBridge>> = transport.as_ref().map(|transport| {
            Arc::new(EmbeddedTransportBridge::new(
                transport.clone(),
                identity.clone(),
                delivery_source_hash,
                announce_targets.clone(),
                last_announce_epoch_secs.clone(),
                peer_crypto.clone(),
                peer_identity_cache_path.clone(),
                selected_propagation_node.clone(),
                known_propagation_nodes.clone(),
                receipt_map.clone(),
                outbound_resource_map.clone(),
                delivered_messages.clone(),
                receipt_tx.clone(),
            ))
        });

        let outbound_bridge: Option<Arc<dyn OutboundBridge>> =
            bridge.as_ref().map(|bridge| bridge.clone() as Arc<dyn OutboundBridge>);
        let announce_bridge: Option<Arc<dyn AnnounceBridge>> =
            bridge.as_ref().map(|bridge| bridge.clone() as Arc<dyn AnnounceBridge>);

        let daemon = Rc::new(RpcDaemon::with_store_and_bridges(
            store,
            identity_hash,
            outbound_bridge,
            announce_bridge,
        ));

        daemon.set_delivery_destination_hash(delivery_destination_hash_hex);
        daemon.replace_interfaces(configured_interfaces);
        let propagation_enabled = bridge.is_some();
        daemon.set_propagation_state(propagation_enabled, None, crate::constants::PROPAGATION_COST);
        daemon.push_event(RpcEvent {
            event_type: "runtime_started".to_string(),
            payload: json!({ "profile": init.profile }),
        });

        if let Some(bridge) = bridge.as_ref() {
            let _ = bridge.announce_now();
        }

        if transport.is_some() {
            spawn_receipt_worker(daemon.clone(), receipt_rx, &shutdown_tx);
        }

        if let Some(transport) = transport.clone() {
            spawn_transport_workers(
                transport,
                daemon.clone(),
                receipt_tx.clone(),
                outbound_resource_map.clone(),
                peer_crypto.clone(),
                peer_announce_meta.clone(),
                peer_identity_cache_path.clone(),
                known_propagation_nodes.clone(),
                &shutdown_tx,
            );
        }

        let scheduler_handle = if bridge.is_some() {
            Some(daemon.clone().start_announce_scheduler(DEFAULT_ANNOUNCE_INTERVAL_SECS))
        } else {
            None
        };

        if let Some(bridge) = bridge.clone() {
            spawn_startup_announce_burst(bridge);
        }

        Ok(Self {
            profile: init.profile.clone(),
            status_template: DaemonStatus {
                running: true,
                pid: None,
                rpc: init.settings.rpc,
                profile: init.profile,
                managed: true,
                transport: init.transport,
                transport_inferred: init.transport_inferred,
                log_path: init.paths.daemon_log.display().to_string(),
            },
            daemon,
            transport,
            local_identity: identity,
            peer_announce_meta,
            peer_crypto,
            peer_identity_cache_path,
            selected_propagation_node,
            propagation_sync_state: Arc::new(Mutex::new(RuntimePropagationSyncState::default())),
            shutdown_tx,
            scheduler_handle,
            shutdown: false,
        })
    }
}
