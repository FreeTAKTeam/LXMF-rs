use crate::cli::app::{PeerAction, PeerCommand, RuntimeContext};
use anyhow::{anyhow, Result};
use serde_json::{json, Value};

pub fn run(ctx: &RuntimeContext, command: &PeerCommand) -> Result<()> {
    match &command.action {
        PeerAction::List => {
            let peers = ctx.rpc.call("list_peers", None)?;
            ctx.output.emit_status(&json!({"peers": peers}))
        }
        PeerAction::Show { peer } => {
            let peers = ctx.rpc.call("list_peers", None)?;
            let found = find_peer(&peers, peer);
            if let Some(entry) = found {
                ctx.output.emit_status(&entry)
            } else {
                Err(anyhow!("peer '{}' not found", peer))
            }
        }
        PeerAction::Watch { interval_secs } => watch_peers(ctx, *interval_secs),
        PeerAction::Sync { peer } => {
            let result = ctx.rpc.call("peer_sync", Some(json!({ "peer": peer })))?;
            ctx.output.emit_status(&result)
        }
        PeerAction::Unpeer { peer } => {
            let result = ctx.rpc.call("peer_unpeer", Some(json!({ "peer": peer })))?;
            ctx.output.emit_status(&result)
        }
        PeerAction::Clear => {
            let result = ctx.rpc.call("clear_peers", None)?;
            ctx.output.emit_status(&result)
        }
    }
}

fn watch_peers(ctx: &RuntimeContext, interval_secs: u64) -> Result<()> {
    loop {
        let peers = ctx.rpc.call("list_peers", None)?;
        ctx.output.emit_status(&json!({ "peers": peers } ))?;
        std::thread::sleep(std::time::Duration::from_secs(interval_secs.max(1)));
    }
}

fn find_peer(peers: &Value, key: &str) -> Option<Value> {
    let list = peers.as_array()?;
    for peer in list {
        if peer.get("peer").and_then(Value::as_str) == Some(key) {
            return Some(peer.clone());
        }
    }
    None
}
