/// Correlates a transport delivery proof with an in-flight diagnostic probe.
///
/// The RPC crate deliberately stores only the packet hash here.  The concrete
/// transport receipt type belongs to the daemon/transport boundary, while the
/// legacy RPC contract only needs a completion signal for the probe request.
#[derive(Clone, Default)]
pub struct ProbeReceiptRegistry {
    pending: Arc<Mutex<HashMap<[u8; 32], tokio::sync::oneshot::Sender<()>>>>,
}

impl ProbeReceiptRegistry {
    pub fn register(&self, packet_hash: [u8; 32]) -> tokio::sync::oneshot::Receiver<()> {
        let (sender, receiver) = tokio::sync::oneshot::channel();
        self.pending
            .lock()
            .expect("probe receipt registry mutex poisoned")
            .insert(packet_hash, sender);
        receiver
    }

    pub fn notify(&self, packet_hash: [u8; 32]) {
        let sender = self
            .pending
            .lock()
            .expect("probe receipt registry mutex poisoned")
            .remove(&packet_hash);
        if let Some(sender) = sender {
            if sender.send(()).is_err() {
                log::debug!("probe receipt waiter was dropped before delivery notification");
            }
        }
    }

    pub fn cancel(&self, packet_hash: [u8; 32]) {
        self.pending
            .lock()
            .expect("probe receipt registry mutex poisoned")
            .remove(&packet_hash);
    }
}

pub trait PathLookupBridge: Send + Sync {
    fn has_path(&self, destination: &str) -> Result<bool, std::io::Error>;

    fn request_path(&self, destination: &str) -> Result<(), std::io::Error>;

    fn request_path_scoped(
        &self,
        destination: &str,
        _on_iface: Option<&str>,
        _tag: Option<&[u8]>,
    ) -> Result<(), std::io::Error> {
        self.request_path(destination)
    }

    fn path_status(&self, destination: &str) -> Result<JsonValue, std::io::Error> {
        let path_found = self.has_path(destination)?;
        Ok(json!({ "path_found": path_found }))
    }

    fn path_table(&self, _max_hops: Option<u64>) -> Result<JsonValue, std::io::Error> {
        Err(std::io::Error::other("path table bridge is not configured"))
    }

    fn link_count(&self) -> Result<usize, std::io::Error> {
        Err(std::io::Error::other("link count bridge is not configured"))
    }

    fn active_link_count(&self) -> Result<usize, std::io::Error> {
        self.link_count()
    }

    fn lowest_interface_bitrate(&self) -> Result<Option<u64>, std::io::Error> {
        Err(std::io::Error::other("interface bitrate bridge is not configured"))
    }

    fn medium_path_timeout(&self) -> Result<f64, std::io::Error> {
        Err(std::io::Error::other("medium path timeout bridge is not configured"))
    }

    fn transport_status(&self) -> Result<JsonValue, std::io::Error> {
        Err(std::io::Error::other("transport status bridge is not configured"))
    }

    fn probe(
        &self,
        _destination: &str,
        _full_name: &str,
        _size: usize,
        _probes: usize,
        _timeout_secs: f64,
        _wait_secs: f64,
    ) -> Result<JsonValue, std::io::Error> {
        Err(std::io::Error::other("probe bridge is not configured"))
    }

    fn drop_path(&self, _destination: &str) -> Result<bool, std::io::Error> {
        Err(std::io::Error::other("path mutation bridge is not configured"))
    }

    fn drop_all_via(&self, _transport: &str) -> Result<usize, std::io::Error> {
        Err(std::io::Error::other("path mutation bridge is not configured"))
    }

    fn drop_announce_queues(&self) -> Result<usize, std::io::Error> {
        Err(std::io::Error::other("announce queue bridge is not configured"))
    }

    fn rate_table(&self) -> Result<JsonValue, std::io::Error> {
        Err(std::io::Error::other("rate table bridge is not configured"))
    }

    fn packet_signal(&self, _packet_hash: &str) -> Result<JsonValue, std::io::Error> {
        Err(std::io::Error::other("packet signal bridge is not configured"))
    }

    fn discovered_interfaces(&self) -> Result<JsonValue, std::io::Error> {
        Err(std::io::Error::other("interface discovery bridge is not configured"))
    }

    fn remove_paths_for_identity(&self, _identity: &str) -> Result<usize, std::io::Error> {
        Ok(0)
    }

    fn set_identity_blackholed(
        &self,
        identity: &str,
        blackholed: bool,
    ) -> Result<usize, std::io::Error> {
        if blackholed {
            self.remove_paths_for_identity(identity)
        } else {
            Ok(0)
        }
    }

    fn set_identity_blackholed_until(
        &self,
        identity: &str,
        blackholed: bool,
        _until: Option<f64>,
    ) -> Result<usize, std::io::Error> {
        self.set_identity_blackholed(identity, blackholed)
    }
}
