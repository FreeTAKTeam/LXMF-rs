use crate::error::LxmfError;
use crate::lxmd::config::LxmdConfig;
use crate::router::Router;
use std::collections::{BTreeMap, VecDeque};
use std::path::PathBuf;
use std::process::Command;
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LxmdCommand {
    Serve,
    Sync { peer: Option<String> },
    Unpeer { peer: String },
    Status,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InboundEvent {
    pub source: [u8; 16],
    pub destination: [u8; 16],
    pub message_id: String,
    pub content: String,
}

impl InboundEvent {
    pub fn new(
        source: [u8; 16],
        destination: [u8; 16],
        message_id: impl Into<String>,
        content: impl Into<String>,
    ) -> Self {
        Self {
            source,
            destination,
            message_id: message_id.into(),
            content: content.into(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServeTickReport {
    pub now: u64,
    pub announced: bool,
    pub inbound_processed: usize,
    pub jobs_run_total: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SyncReport {
    pub synced_peers: usize,
    pub created_transfers: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LxmdStatus {
    pub propagation_node: bool,
    pub propagation_enabled: bool,
    pub announce_interval_secs: u64,
    pub service_tick_interval_secs: u64,
    pub last_announce_at: Option<u64>,
    pub peer_count: usize,
    pub peers: Vec<String>,
    pub pending_inbound: usize,
    pub pending_sync_items: usize,
    pub outbound_queue: usize,
    pub jobs_run: u64,
    pub sync_runs: u64,
    pub unpeer_runs: u64,
    pub inbound_hooks_run: u64,
    pub announces_sent: u64,
    pub service_ticks: u64,
    pub peer_sync_runs: usize,
    pub peer_sync_items: usize,
}

pub struct LxmdRuntime {
    config: LxmdConfig,
    router: Router,
    peers: BTreeMap<String, [u8; 16]>,
    pending_inbound: VecDeque<InboundEvent>,
    recent_inbound_ids: VecDeque<Vec<u8>>,
    last_announce_at: Option<u64>,
    jobs_run: u64,
    sync_runs: u64,
    unpeer_runs: u64,
    inbound_hooks_run: u64,
    announces_sent: u64,
    service_ticks: u64,
}

impl LxmdRuntime {
    pub fn new(config: LxmdConfig) -> Result<Self, LxmfError> {
        let mut router = Router::default();
        router.set_propagation_node(config.propagation_node);

        if config.propagation_node {
            let store_path = propagation_store_path(&config);
            std::fs::create_dir_all(&store_path).map_err(|e| LxmfError::Io(e.to_string()))?;
            router.enable_propagation(
                &store_path,
                config
                    .propagation_target_cost
                    .max(crate::constants::PROPAGATION_COST_MIN),
            );
        }

        Ok(Self {
            config,
            router,
            peers: BTreeMap::new(),
            pending_inbound: VecDeque::new(),
            recent_inbound_ids: VecDeque::new(),
            last_announce_at: None,
            jobs_run: 0,
            sync_runs: 0,
            unpeer_runs: 0,
            inbound_hooks_run: 0,
            announces_sent: 0,
            service_ticks: 0,
        })
    }

    pub fn config(&self) -> &LxmdConfig {
        &self.config
    }

    pub fn router(&self) -> &Router {
        &self.router
    }

    pub fn queue_inbound(&mut self, event: InboundEvent) {
        self.pending_inbound.push_back(event);
    }

    pub fn serve_tick(&mut self) -> Result<ServeTickReport, LxmfError> {
        self.serve_tick_at(unix_now())
    }

    pub fn serve_tick_at(&mut self, now: u64) -> Result<ServeTickReport, LxmfError> {
        self.router.jobs_at(now);
        self.jobs_run += 1;
        self.service_ticks += 1;

        let announced = self.maybe_announce(now);
        let inbound_processed = self.run_inbound_hooks()?;

        Ok(ServeTickReport {
            now,
            announced,
            inbound_processed,
            jobs_run_total: self.jobs_run,
        })
    }

    pub fn sync(&mut self, peer: Option<String>, now: u64) -> Result<SyncReport, LxmfError> {
        self.sync_runs += 1;

        let targets: Vec<String> = match peer {
            Some(peer) => vec![peer],
            None => self.peers.keys().cloned().collect(),
        };

        let mut synced_peers = 0;
        let mut created_transfers = 0;
        for peer_name in targets {
            let destination = self.ensure_peer(&peer_name);
            self.router.allow_destination(destination);
            synced_peers += 1;
            let max_sync_items = self.router.config().propagation_per_sync_limit as usize;

            let batch = self
                .router
                .build_peer_sync_batch(&destination, max_sync_items);
            created_transfers += batch.len();

            if !batch.is_empty() {
                self.router
                    .apply_peer_sync_result(&destination, &batch, &[]);
            }

            if let Some(peer_state) = self.router.peer_mut(&destination) {
                peer_state.mark_seen(now as f64);
            }
        }

        Ok(SyncReport {
            synced_peers,
            created_transfers,
        })
    }

    pub fn unpeer(&mut self, peer: &str) -> bool {
        self.unpeer_runs += 1;

        let Some(destination) = self.peers.remove(peer) else {
            return false;
        };

        self.router.unregister_identity(&destination);
        self.router.remove_peer(&destination);
        self.router.clear_destination_policy(&destination);
        self.router.unignore_destination(&destination);
        self.router.deprioritise_destination(&destination);
        true
    }

    pub fn status(&self) -> LxmdStatus {
        LxmdStatus {
            propagation_node: self.config.propagation_node,
            propagation_enabled: self.router.propagation_enabled(),
            announce_interval_secs: self.config.announce_interval_secs,
            service_tick_interval_secs: self.config.service_tick_interval_secs,
            last_announce_at: self.last_announce_at,
            peer_count: self.peers.len(),
            peers: self.peers.keys().cloned().collect(),
            pending_inbound: self.pending_inbound.len(),
            pending_sync_items: self
                .peers
                .values()
                .filter_map(|destination| self.router.peer(destination))
                .map(|peer| peer.unhandled_message_count())
                .sum(),
            outbound_queue: self.router.outbound_len(),
            jobs_run: self.jobs_run,
            sync_runs: self.sync_runs,
            unpeer_runs: self.unpeer_runs,
            inbound_hooks_run: self.inbound_hooks_run,
            announces_sent: self.announces_sent,
            service_ticks: self.service_ticks,
            peer_sync_runs: self.router.stats().peer_sync_runs_total,
            peer_sync_items: self.router.stats().peer_sync_items_total,
        }
    }

    pub fn run_service(&mut self, max_ticks: Option<u64>) -> Result<LxmdStatus, LxmfError> {
        let tick_interval = Duration::from_secs(self.config.service_tick_interval_secs.max(1));
        loop {
            self.serve_tick()?;
            if let Some(max_ticks) = max_ticks {
                if self.service_ticks >= max_ticks {
                    break;
                }
            }
            thread::sleep(tick_interval);
        }
        Ok(self.status())
    }

    fn maybe_announce(&mut self, now: u64) -> bool {
        if !self.config.propagation_node {
            return false;
        }

        let due = match self.last_announce_at {
            Some(last) => now.saturating_sub(last) >= self.config.announce_interval_secs.max(1),
            None => true,
        };

        if due {
            self.last_announce_at = Some(now);
            self.announces_sent += 1;
            true
        } else {
            false
        }
    }

    fn run_inbound_hooks(&mut self) -> Result<usize, LxmfError> {
        let hook_cmd = self.config.on_inbound.clone();

        let mut processed = 0usize;
        while let Some(event) = self.pending_inbound.pop_front() {
            self.remember_inbound_id(event.message_id.as_bytes().to_vec());
            processed += 1;

            if let Some(hook_cmd) = hook_cmd.as_deref() {
                let status = Command::new("sh")
                    .args(["-c", hook_cmd])
                    .env("LXMF_SOURCE", hex::encode(event.source))
                    .env("LXMF_DESTINATION", hex::encode(event.destination))
                    .env("LXMF_MESSAGE_ID", &event.message_id)
                    .env("LXMF_CONTENT", &event.content)
                    .status()
                    .map_err(|e| LxmfError::Io(e.to_string()))?;

                if !status.success() {
                    return Err(LxmfError::Io(format!(
                        "inbound hook failed with status {status}"
                    )));
                }

                self.inbound_hooks_run += 1;
            }
        }

        Ok(processed)
    }

    fn ensure_peer(&mut self, peer_name: &str) -> [u8; 16] {
        if let Some(destination) = self.peers.get(peer_name).copied() {
            return destination;
        }

        let destination = peer_to_destination(peer_name);
        self.router
            .register_identity(destination, Some(peer_name.to_string()));
        self.router.register_peer(destination);
        for message_id in &self.recent_inbound_ids {
            self.router.queue_peer_unhandled(destination, message_id);
        }
        self.peers.insert(peer_name.to_string(), destination);
        destination
    }

    fn remember_inbound_id(&mut self, message_id: Vec<u8>) {
        if self.recent_inbound_ids.iter().any(|id| *id == message_id) {
            return;
        }

        self.recent_inbound_ids.push_back(message_id.clone());
        while self.recent_inbound_ids.len() > 256 {
            self.recent_inbound_ids.pop_front();
        }

        for destination in self.peers.values().copied() {
            self.router.queue_peer_unhandled(destination, &message_id);
        }
    }
}

pub fn execute(command: LxmdCommand, config: &LxmdConfig) -> Result<String, LxmfError> {
    let mut runtime = LxmdRuntime::new(config.clone())?;
    execute_with_runtime(&mut runtime, command, unix_now())
}

pub fn execute_with_runtime(
    runtime: &mut LxmdRuntime,
    command: LxmdCommand,
    now: u64,
) -> Result<String, LxmfError> {
    match command {
        LxmdCommand::Serve => {
            let report = runtime.serve_tick_at(now)?;
            Ok(format!(
                "lxmd serve tick={} announced={} inbound_processed={} jobs_run_total={}",
                report.now, report.announced, report.inbound_processed, report.jobs_run_total
            ))
        }
        LxmdCommand::Sync { peer } => {
            let report = runtime.sync(peer.clone(), now)?;
            Ok(format!(
                "lxmd sync peer={} synced_peers={} created_transfers={}",
                peer.as_deref().unwrap_or("all"),
                report.synced_peers,
                report.created_transfers
            ))
        }
        LxmdCommand::Unpeer { peer } => Ok(format!(
            "lxmd unpeer peer={} removed={}",
            peer,
            runtime.unpeer(&peer)
        )),
        LxmdCommand::Status => {
            let status = runtime.status();
            Ok(format!(
                "lxmd status propagation_node={} propagation_enabled={} peer_count={} outbound_queue={} pending_inbound={} pending_sync_items={} jobs_run={} announces_sent={} sync_runs={} unpeer_runs={} peer_sync_runs={} peer_sync_items={}",
                status.propagation_node,
                status.propagation_enabled,
                status.peer_count,
                status.outbound_queue,
                status.pending_inbound,
                status.pending_sync_items,
                status.jobs_run,
                status.announces_sent,
                status.sync_runs,
                status.unpeer_runs,
                status.peer_sync_runs,
                status.peer_sync_items
            ))
        }
    }
}

fn propagation_store_path(config: &LxmdConfig) -> PathBuf {
    config
        .storage_path
        .as_deref()
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(".lxmd-propagation"))
}

fn peer_to_destination(peer_name: &str) -> [u8; 16] {
    let mut destination = [0u8; 16];
    let hash = reticulum::hash::Hash::new_from_slice(peer_name.as_bytes()).to_bytes();
    destination.copy_from_slice(&hash[..16]);
    destination
}

fn unix_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}
