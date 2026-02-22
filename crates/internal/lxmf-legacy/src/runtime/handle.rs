use super::*;

#[derive(Clone)]
pub struct RuntimeHandle {
    inner: Arc<RuntimeInner>,
}

struct RuntimeInner {
    profile: String,
    settings: ProfileSettings,
    running: AtomicBool,
    next_id: AtomicU64,
    transport: Option<String>,
    transport_inferred: bool,
    log_path: String,
    command_tx: UnboundedSender<RuntimeRequest>,
}

impl RuntimeHandle {
    pub fn status(&self) -> DaemonStatus {
        match self.request(RuntimeCommand::Status) {
            Ok(RuntimeResponse::Status(status)) => {
                self.inner.running.store(status.running, Ordering::Relaxed);
                status
            }
            _ => {
                self.inner.running.store(false, Ordering::Relaxed);
                self.fallback_status()
            }
        }
    }

    pub fn profile(&self) -> &str {
        &self.inner.profile
    }

    pub fn settings(&self) -> ProfileSettings {
        self.inner.settings.clone()
    }

    pub fn stop(&self) {
        if !self.inner.running.swap(false, Ordering::Relaxed) {
            return;
        }
        let _ = self.request(RuntimeCommand::Stop);
    }

    pub fn is_running(&self) -> bool {
        self.inner.running.load(Ordering::Relaxed)
    }

    pub fn poll_event(&self) -> Option<RpcEvent> {
        if !self.is_running() {
            return None;
        }

        match self.request(RuntimeCommand::PollEvent) {
            Ok(RuntimeResponse::Event(event)) => event,
            _ => {
                self.inner.running.store(false, Ordering::Relaxed);
                None
            }
        }
    }

    pub fn call(&self, method: &str, params: Option<Value>) -> Result<Value, LxmfError> {
        if !self.is_running() {
            return Err(LxmfError::Io("embedded runtime is stopped".to_string()));
        }

        let request = RpcRequest {
            id: self.inner.next_id.fetch_add(1, Ordering::Relaxed),
            method: method.to_string(),
            params,
        };

        match self.request(RuntimeCommand::Call(request)) {
            Ok(RuntimeResponse::Value(value)) => Ok(value),
            Ok(_) => Err(LxmfError::Io("unexpected runtime response".to_string())),
            Err(err) => {
                if Self::is_recoverable_rpc_error(&err) {
                    return Err(err);
                }
                self.inner.running.store(false, Ordering::Relaxed);
                Err(err)
            }
        }
    }

    fn is_recoverable_rpc_error(error: &LxmfError) -> bool {
        match error {
            LxmfError::Io(msg) => msg.starts_with("rpc failed ["),
            _ => false,
        }
    }

    pub fn send_message(
        &self,
        request: SendMessageRequest,
    ) -> Result<SendMessageResponse, LxmfError> {
        let source =
            if let Some(source_private_key) = clean_non_empty(request.source_private_key.clone()) {
                source_hash_from_private_key_hex(&source_private_key)?
            } else {
                self.resolve_source_for_send(request.source.clone())?
            };
        let prepared = build_send_params_with_source(request, source)?;
        let PreparedSendMessage { id, source, destination, params } = prepared;

        let result = self.call("send_message_v2", Some(params))?;

        Ok(SendMessageResponse { id, source, destination, result })
    }

    pub fn send_command(
        &self,
        request: SendCommandRequest,
    ) -> Result<SendMessageResponse, LxmfError> {
        if request.commands.is_empty() {
            return Err(LxmfError::Io(
                "send_command requires at least one command entry".to_string(),
            ));
        }
        if request.message.fields.is_some() {
            return Err(LxmfError::Io(
                "send_command does not accept pre-populated fields; use send_message for custom field maps"
                    .to_string(),
            ));
        }

        let mut fields = WireFields::new();
        fields.set_commands(request.commands);

        let mut message = request.message;
        message.fields = Some(fields.to_transport_json()?);
        self.send_message(message)
    }

    pub fn probe(&self) -> RuntimeProbeReport {
        let local = self.status();
        let started = Instant::now();
        let mut failures = Vec::new();
        let mut rpc_probe = RpcProbeReport {
            reachable: false,
            endpoint: self.inner.settings.rpc.clone(),
            method: None,
            roundtrip_ms: None,
            identity_hash: None,
            status: None,
            errors: Vec::new(),
        };

        if self.is_running() {
            for method in ["daemon_status_ex", "status"] {
                match self.call(method, None) {
                    Ok(status) => {
                        rpc_probe.reachable = true;
                        rpc_probe.method = Some(method.to_string());
                        rpc_probe.roundtrip_ms = Some(started.elapsed().as_millis());
                        rpc_probe.identity_hash = extract_identity_hash(&status);
                        rpc_probe.status = Some(status);
                        rpc_probe.errors = failures.clone();
                        break;
                    }
                    Err(err) => failures.push(format!("{method}: {err}")),
                }
            }
        } else {
            failures.push("runtime not started".to_string());
        }

        if !rpc_probe.reachable {
            rpc_probe.errors = failures;
        }

        let events_started = Instant::now();
        let events_probe = if self.is_running() {
            match self.poll_event() {
                Some(event) => EventsProbeReport {
                    reachable: true,
                    endpoint: self.inner.settings.rpc.clone(),
                    roundtrip_ms: Some(events_started.elapsed().as_millis()),
                    event_type: Some(event.event_type),
                    payload: Some(event.payload),
                    error: None,
                },
                None => EventsProbeReport {
                    reachable: true,
                    endpoint: self.inner.settings.rpc.clone(),
                    roundtrip_ms: Some(events_started.elapsed().as_millis()),
                    event_type: None,
                    payload: None,
                    error: None,
                },
            }
        } else {
            EventsProbeReport {
                reachable: false,
                endpoint: self.inner.settings.rpc.clone(),
                roundtrip_ms: None,
                event_type: None,
                payload: None,
                error: Some("runtime not started".to_string()),
            }
        };

        RuntimeProbeReport {
            profile: self.inner.profile.clone(),
            local,
            rpc: rpc_probe,
            events: events_probe,
        }
    }

    fn request(&self, command: RuntimeCommand) -> Result<RuntimeResponse, LxmfError> {
        let (tx, rx) = std_mpsc::channel();
        self.inner
            .command_tx
            .send(RuntimeRequest { command, respond_to: tx })
            .map_err(|_| LxmfError::Io("embedded runtime worker unavailable".to_string()))?;

        let response = rx
            .recv_timeout(Duration::from_secs(10))
            .map_err(|_| LxmfError::Io("embedded runtime worker did not respond".to_string()))?;

        response.map_err(LxmfError::Io)
    }

    fn fallback_status(&self) -> DaemonStatus {
        DaemonStatus {
            running: self.inner.running.load(Ordering::Relaxed),
            pid: None,
            rpc: self.inner.settings.rpc.clone(),
            profile: self.inner.profile.clone(),
            managed: true,
            transport: self.inner.transport.clone(),
            transport_inferred: self.inner.transport_inferred,
            log_path: self.inner.log_path.clone(),
        }
    }

    fn resolve_source_for_send(&self, source: Option<String>) -> Result<String, LxmfError> {
        if let Some(value) = clean_non_empty(source) {
            return Ok(value);
        }

        let mut failures = Vec::new();
        for method in ["daemon_status_ex", "status"] {
            match self.call(method, None) {
                Ok(response) => {
                    if let Some(hash) = extract_identity_hash(&response) {
                        return Ok(hash);
                    }
                    failures.push(format!("{method}: missing identity hash"));
                }
                Err(err) => failures.push(format!("{method}: {err}")),
            }
        }

        let detail =
            if failures.is_empty() { String::new() } else { format!(" ({})", failures.join("; ")) };
        Err(LxmfError::Io(format!(
            "source not provided and daemon did not report delivery/identity hash{detail}"
        )))
    }
}

impl Drop for RuntimeHandle {
    fn drop(&mut self) {
        if Arc::strong_count(&self.inner) == 1 {
            self.stop();
        }
    }
}

pub fn start(config: RuntimeConfig) -> Result<RuntimeHandle, LxmfError> {
    let profile_requested =
        clean_non_empty(Some(config.profile)).unwrap_or_else(|| "default".to_string());
    let profile = resolve_runtime_profile_name(&profile_requested)
        .map_err(|err| LxmfError::Io(err.to_string()))?;
    let mut settings =
        load_profile_settings(&profile).map_err(|err| LxmfError::Io(err.to_string()))?;

    if let Some(rpc) = clean_non_empty(config.rpc) {
        settings.rpc = rpc;
    }
    if let Some(transport) = clean_non_empty(config.transport) {
        settings.transport = Some(transport);
    }

    let paths = profile_paths(&profile).map_err(|err| LxmfError::Io(err.to_string()))?;
    fs::create_dir_all(&paths.root).map_err(|err| LxmfError::Io(err.to_string()))?;

    let config_interfaces =
        load_reticulum_config(&profile).map_err(|err| LxmfError::Io(err.to_string()))?.interfaces;
    let has_enabled_interfaces = config_interfaces.iter().any(|iface| iface.enabled);
    let (transport, transport_inferred) = resolve_transport(&settings, has_enabled_interfaces);

    let (command_tx, command_rx) = unbounded_channel();
    let (startup_tx, startup_rx) = std_mpsc::channel();

    let worker_init = WorkerInit {
        profile: profile.clone(),
        settings: settings.clone(),
        paths: paths.clone(),
        transport: transport.clone(),
        transport_inferred,
        interfaces: config_interfaces,
    };

    thread::Builder::new()
        .name(format!("lxmf-runtime-{}", profile))
        .spawn(move || runtime_thread(worker_init, command_rx, startup_tx))
        .map_err(|err| LxmfError::Io(format!("failed to spawn runtime worker: {err}")))?;

    match startup_rx
        .recv_timeout(Duration::from_secs(20))
        .map_err(|_| LxmfError::Io("runtime startup timed out".to_string()))?
    {
        Ok(()) => {}
        Err(err) => return Err(LxmfError::Io(err)),
    }

    Ok(RuntimeHandle {
        inner: Arc::new(RuntimeInner {
            profile,
            settings,
            running: AtomicBool::new(true),
            next_id: AtomicU64::new(1),
            transport,
            transport_inferred,
            log_path: paths.daemon_log.display().to_string(),
            command_tx,
        }),
    })
}
