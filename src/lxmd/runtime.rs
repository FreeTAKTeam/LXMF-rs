use crate::error::LxmfError;
use crate::lxmd::config::LxmdConfig;
use crate::router::Router;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LxmdCommand {
    Serve,
    Sync { peer: Option<String> },
    Unpeer { peer: String },
    Status,
}

pub fn execute(command: LxmdCommand, config: &LxmdConfig) -> Result<String, LxmfError> {
    let mut router = Router::default();
    router.set_propagation_node(config.propagation_node);

    let result = match command {
        LxmdCommand::Serve => format!(
            "lxmd serve propagation_node={} announce_interval_secs={} on_inbound={}",
            config.propagation_node,
            config.announce_interval_secs,
            config.on_inbound.as_deref().unwrap_or("none")
        ),
        LxmdCommand::Sync { peer } => format!(
            "lxmd sync peer={} propagation_node={}",
            peer.as_deref().unwrap_or("all"),
            config.propagation_node
        ),
        LxmdCommand::Unpeer { peer } => format!("lxmd unpeer peer={peer}"),
        LxmdCommand::Status => format!(
            "lxmd status propagation_node={} announce_interval_secs={} outbound_queue={}",
            config.propagation_node,
            config.announce_interval_secs,
            router.outbound_len()
        ),
    };

    Ok(result)
}
