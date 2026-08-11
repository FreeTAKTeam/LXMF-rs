use clap::Parser;
use rns_transport::destination::DestinationDesc;
use rns_transport::hash::AddressHash;
use serde::Deserialize;
use serde_json::Value;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{Mutex, RwLock};

pub const CHANNEL_MESSAGE_TYPE: u16 = 0xCAFE;

pub fn address_hex(value: &AddressHash) -> String {
    hex::encode(value.as_slice())
}

#[derive(Clone, Debug, Parser)]
#[command(
    name = "independent-interop-node",
    about = "Controlled LXMF-rs Reticulum node for independent-stack interoperability"
)]
pub struct Cli {
    #[arg(long)]
    pub name: String,
    #[arg(long)]
    pub control: String,
    #[arg(long)]
    pub listen: Vec<String>,
    #[arg(long, value_name = "MODE")]
    pub listen_mode: Vec<String>,
    #[arg(long, value_name = "GRAVITY")]
    pub listen_gravity: Vec<i64>,
    #[arg(long)]
    pub connect: Vec<String>,
    #[arg(long, value_name = "MODE")]
    pub connect_mode: Vec<String>,
    #[arg(long, value_name = "GRAVITY")]
    pub connect_gravity: Vec<i64>,
    #[arg(long)]
    pub transport: bool,
    #[arg(long)]
    pub identity_seed: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct ControlRequest {
    pub method: String,
    #[serde(default)]
    pub params: Value,
}

#[derive(Clone)]
pub struct SharedState {
    pub name: String,
    pub identity_hash: AddressHash,
    pub destination_hash: AddressHash,
    pub known_destinations: Arc<RwLock<HashMap<AddressHash, DestinationDesc>>>,
    pub links: Arc<RwLock<HashMap<AddressHash, Value>>>,
    pub prepared_resources: Arc<RwLock<HashMap<String, Vec<u8>>>>,
    pub events: Arc<Mutex<Vec<Value>>>,
}

impl SharedState {
    pub fn new(name: String, identity_hash: AddressHash, destination_hash: AddressHash) -> Self {
        Self {
            name,
            identity_hash,
            destination_hash,
            known_destinations: Arc::new(RwLock::new(HashMap::new())),
            links: Arc::new(RwLock::new(HashMap::new())),
            prepared_resources: Arc::new(RwLock::new(HashMap::new())),
            events: Arc::new(Mutex::new(Vec::new())),
        }
    }

    pub async fn record(&self, event: Value) {
        self.events.lock().await.push(event);
    }
}
