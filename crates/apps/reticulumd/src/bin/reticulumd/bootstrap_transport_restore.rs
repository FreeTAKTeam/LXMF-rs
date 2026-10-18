use super::*;
use crate::configured_interface_runtime::ConfiguredInterfaceStartup;
use reticulum_daemon::config::InterfaceConfig;

pub(crate) struct ConfiguredStartupRuntime {
    pub(super) selection: TcpServerSelection,
    pub(super) strict_interface_startup: bool,
    pub(super) strict_selected_listener: bool,
    pub(super) storage_path: std::path::PathBuf,
    pub(super) identity_hash: [u8; 16],
    pub(super) reconnect_events: tokio::sync::mpsc::Sender<AddressHash>,
    pub(super) reconnect_completion: tokio::task::JoinHandle<()>,
}

impl ConfiguredStartupRuntime {
    pub(crate) fn reconnect_events(&self) -> &tokio::sync::mpsc::Sender<AddressHash> {
        &self.reconnect_events
    }

    #[cfg(test)]
    pub(crate) async fn for_test(
        transport: &Arc<Transport>,
        storage_path: &std::path::Path,
        recipes: &[InterfaceConfig],
        strict: bool,
    ) -> Self {
        let args = <Args as clap::Parser>::parse_from(["reticulumd"]);
        let mut config =
            DaemonConfig::from_toml("[reticulum]\nshare_instance=false").expect("fixture config");
        config.interfaces = recipes.to_vec();
        let (reconnect_events, rx) = tokio::sync::mpsc::channel(32);
        Self {
            selection: select_tcp_server_bind(&args, Some(&config)).expect("fixture selection"),
            strict_interface_startup: strict,
            strict_selected_listener: strict,
            storage_path: storage_path.to_owned(),
            identity_hash: transport
                .internal_identity()
                .await
                .as_slice()
                .try_into()
                .expect("transport identity address is sixteen bytes"),
            reconnect_events,
            reconnect_completion: spawn_stream_reconnect_tunnel_synthesizer(transport.clone(), rx),
        }
    }

    pub(crate) async fn restore(
        &self,
        index: usize,
        iface: &InterfaceConfig,
        transport: &Arc<Transport>,
    ) -> (ConfiguredInterfaceStartup, Option<String>) {
        let manager = transport.iface_manager();
        let mut record = crate::bootstrap::interface_record_from_config(iface);
        let mut batch = InterfaceStartupBatch::default();
        let server_iface = if self.selection.selected_index == Some(index) {
            startup_selected_tcp_server(
                &self.selection,
                Some(iface),
                &manager,
                self.strict_selected_listener,
                Some(&mut record),
                &mut batch.startup_successes,
                &mut batch.startup_failures,
            )
            .await
            .map(|(address, status)| {
                batch.tcp_runtime_refreshes.push(TcpRuntimeRefresh {
                    runtime_iface: address,
                    status: TcpRuntimeStatusSource::Listener(status),
                });
                address
            })
        } else {
            None
        };
        let context = interface_startup::ConfiguredStartupContext {
            strict_interface_startup: self.strict_interface_startup,
            selected_tcp_server: &self.selection,
            transport,
            iface_manager: &manager,
            server_iface: server_iface.as_ref(),
            reticulum_storage_path: &self.storage_path,
            shared_reconnect_events: Some(self.reconnect_events.clone()),
            transport_identity_hash: Some(self.identity_hash),
        };
        interface_startup::startup_configured_interface_with_auto_plan_builder(
            &context,
            index,
            iface,
            &mut record,
            &mut batch,
            &mut crate::interfaces::auto::build_native_startup_plan,
        )
        .await;
        for address in std::mem::take(&mut batch.tunnel_synth_ifaces) {
            if !transport.synthesize_tunnel_on_interface(address).await {
                log::warn!(
                    "[daemon] restored interface could not synthesize tunnel iface={address}"
                );
            }
        }
        let failure = (!batch.startup_failures.is_empty()).then(|| {
            batch
                .startup_failures
                .iter()
                .map(|failure| failure.error.as_str())
                .collect::<Vec<_>>()
                .join("; ")
        });
        let mut root = batch
            .into_owned_indexed_roots(std::iter::once((index, record)))
            .pop()
            .expect("one trusted recipe produces one owned outcome");
        if root.native.is_some() && failure.is_none() {
            mark_interface_runtime_managed(&mut root.record, "daemon_transport");
            if root
                .record
                .settings
                .as_ref()
                .and_then(|settings| settings.get("_runtime"))
                .and_then(|runtime| runtime.get("runtime_status"))
                .is_none()
            {
                crate::bootstrap::mark_interface_runtime_fields(&mut root.record, "running", 0);
            }
        }
        (root, failure)
    }

    pub(crate) fn into_reconnect_completion(self) -> tokio::task::JoinHandle<()> {
        drop(self.reconnect_events);
        self.reconnect_completion
    }
}
