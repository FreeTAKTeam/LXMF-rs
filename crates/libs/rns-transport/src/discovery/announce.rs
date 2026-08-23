use super::DiscoveredInterface;
use hkdf::Hkdf;
use rmpv::Value;
use sha2::{Digest, Sha256};
use std::io;

pub const DEFAULT_STAMP_VALUE: u32 = 16;
pub const WORKBLOCK_EXPAND_ROUNDS: u32 = 20;
pub const STAMP_SIZE: usize = 32;
pub const FLAG_SIGNED: u8 = 0b0000_0001;
pub const FLAG_ENCRYPTED: u8 = 0b0000_0010;

const NAME: i64 = 0xff;
const TRANSPORT_ID: i64 = 0xfe;
const INTERFACE_TYPE: i64 = 0x00;
const TRANSPORT: i64 = 0x01;
const REACHABLE_ON: i64 = 0x02;
const LATITUDE: i64 = 0x03;
const LONGITUDE: i64 = 0x04;
const HEIGHT: i64 = 0x05;
const PORT: i64 = 0x06;
const IFAC_NETNAME: i64 = 0x07;
const IFAC_NETKEY: i64 = 0x08;
const FREQUENCY: i64 = 0x09;
const BANDWIDTH: i64 = 0x0a;
const SPREADING_FACTOR: i64 = 0x0b;
const CODING_RATE: i64 = 0x0c;
const MODULATION: i64 = 0x0d;
const CHANNEL: i64 = 0x0e;
const OPERATOR_LXMF_ADDRESS: i64 = 0xf0;

#[derive(Debug, Clone, PartialEq)]
pub struct DiscoverableInterface {
    pub interface_type: String,
    pub transport: bool,
    pub transport_id: [u8; 16],
    pub name: String,
    pub latitude: Option<f64>,
    pub longitude: Option<f64>,
    pub height: Option<f64>,
    pub operator_lxmf_address: Option<[u8; 16]>,
    pub reachable_on: Option<String>,
    pub port: Option<u16>,
    pub ifac_netname: Option<String>,
    pub ifac_netkey: Option<String>,
    pub frequency: Option<u64>,
    pub bandwidth: Option<u64>,
    pub spreading_factor: Option<u8>,
    pub coding_rate: Option<u8>,
    pub modulation: Option<String>,
    pub channel: Option<u64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiscoveryAnnounceError {
    UnsupportedInterface,
    MissingEndpoint,
    InvalidEndpoint,
    MissingRadioField(&'static str),
    StampGenerationExhausted,
    PayloadTooShort,
    UnauthorizedSource,
    MissingNetworkIdentity,
    EncryptedWithoutDecryptor,
    DecryptionFailed,
    InvalidStamp,
    InvalidMessagePack,
    MissingField(i64),
    InvalidField(i64),
}

impl std::fmt::Display for DiscoveryAnnounceError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{self:?}")
    }
}

impl std::error::Error for DiscoveryAnnounceError {}

pub fn sanitize(value: &str) -> String {
    value.replace(['\n', '\r'], "").trim().to_string()
}

pub fn encode_announce<F>(
    interface: &DiscoverableInterface,
    stamp_value: u32,
    encrypt: Option<F>,
) -> Result<Vec<u8>, DiscoveryAnnounceError>
where
    F: FnOnce(&[u8]) -> Option<Vec<u8>>,
{
    let packed = encode_interface(interface)?;
    let info_hash = Sha256::digest(&packed);
    let workblock = stamp_workblock(info_hash.as_slice(), WORKBLOCK_EXPAND_ROUNDS);
    let stamp = generate_stamp(info_hash.as_slice(), &workblock, stamp_value, 1 << 24)?;
    let mut body = packed;
    body.extend_from_slice(&stamp);
    match encrypt {
        Some(encrypt) => {
            let encrypted = encrypt(&body).ok_or(DiscoveryAnnounceError::DecryptionFailed)?;
            let mut payload = Vec::with_capacity(encrypted.len() + 1);
            payload.push(FLAG_ENCRYPTED);
            payload.extend_from_slice(&encrypted);
            Ok(payload)
        }
        None => {
            let mut payload = Vec::with_capacity(body.len() + 1);
            payload.push(0);
            payload.extend_from_slice(&body);
            Ok(payload)
        }
    }
}

pub fn encode_plain_announce(
    interface: &DiscoverableInterface,
    stamp_value: u32,
) -> Result<Vec<u8>, DiscoveryAnnounceError> {
    encode_announce(interface, stamp_value, None::<fn(&[u8]) -> Option<Vec<u8>>>)
}

pub fn decode_announce<F>(
    payload: &[u8],
    network_id: &str,
    allowed_network_ids: &[String],
    hops: u8,
    received: f64,
    required_value: u32,
    decrypt: Option<F>,
) -> Result<DiscoveredInterface, DiscoveryAnnounceError>
where
    F: FnOnce(&[u8]) -> Option<Vec<u8>>,
{
    if !allowed_network_ids.is_empty()
        && !allowed_network_ids.iter().any(|allowed| allowed == network_id)
    {
        return Err(DiscoveryAnnounceError::UnauthorizedSource);
    }
    let (&flags, body) = payload.split_first().ok_or(DiscoveryAnnounceError::PayloadTooShort)?;
    let decrypted;
    let body = if flags & FLAG_ENCRYPTED != 0 {
        let decrypt = decrypt.ok_or(DiscoveryAnnounceError::EncryptedWithoutDecryptor)?;
        decrypted = decrypt(body).ok_or(DiscoveryAnnounceError::DecryptionFailed)?;
        decrypted.as_slice()
    } else {
        body
    };
    if body.len() <= STAMP_SIZE {
        return Err(DiscoveryAnnounceError::PayloadTooShort);
    }
    let (packed, stamp) = body.split_at(body.len() - STAMP_SIZE);
    let info_hash = Sha256::digest(packed);
    let workblock = stamp_workblock(info_hash.as_slice(), WORKBLOCK_EXPAND_ROUNDS);
    let value = stamp_value(&workblock, stamp);
    if value < required_value {
        return Err(DiscoveryAnnounceError::InvalidStamp);
    }
    decode_interface(packed, stamp, value, network_id, hops, received)
}

pub fn decode_plain_announce(
    payload: &[u8],
    network_id: &str,
    allowed_network_ids: &[String],
    hops: u8,
    received: f64,
    required_value: u32,
) -> Result<DiscoveredInterface, DiscoveryAnnounceError> {
    decode_announce(
        payload,
        network_id,
        allowed_network_ids,
        hops,
        received,
        required_value,
        None::<fn(&[u8]) -> Option<Vec<u8>>>,
    )
}

pub fn stamp_workblock(material: &[u8], expand_rounds: u32) -> Vec<u8> {
    let mut workblock = Vec::with_capacity(expand_rounds as usize * 256);
    for round in 0..expand_rounds {
        let packed_round = rmp_serde::to_vec(&round).expect("u32 msgpack encoding is infallible");
        let salt = Sha256::digest([material, packed_round.as_slice()].concat());
        let hkdf = Hkdf::<Sha256>::new(Some(salt.as_slice()), material);
        let mut expanded = [0_u8; 256];
        hkdf.expand(&[], &mut expanded).expect("256-byte SHA-256 HKDF expansion is valid");
        workblock.extend_from_slice(&expanded);
    }
    workblock
}

pub fn stamp_value(workblock: &[u8], stamp: &[u8]) -> u32 {
    let digest = Sha256::digest([workblock, stamp].concat());
    digest.iter().map(|byte| byte.leading_zeros()).take_while(|zeros| *zeros == 8).sum::<u32>()
        + digest.iter().find(|byte| **byte != 0).map_or(0, |byte| byte.leading_zeros())
}

pub fn stamp_valid(stamp: &[u8], target_cost: u32, workblock: &[u8]) -> bool {
    stamp_value(workblock, stamp) >= target_cost
}

fn generate_stamp(
    material: &[u8],
    workblock: &[u8],
    target: u32,
    max_rounds: u64,
) -> Result<[u8; STAMP_SIZE], DiscoveryAnnounceError> {
    for round in 0..max_rounds {
        let stamp: [u8; STAMP_SIZE] =
            Sha256::digest([material, &round.to_be_bytes()].concat()).into();
        if stamp_valid(&stamp, target, workblock) {
            return Ok(stamp);
        }
    }
    Err(DiscoveryAnnounceError::StampGenerationExhausted)
}

fn encode_interface(interface: &DiscoverableInterface) -> Result<Vec<u8>, DiscoveryAnnounceError> {
    let interface_type = interface.interface_type.as_str();
    if !super::DISCOVERABLE_TYPES.contains(&interface_type) {
        return Err(DiscoveryAnnounceError::UnsupportedInterface);
    }
    let mut fields = vec![
        field(INTERFACE_TYPE, Value::from(interface_type)),
        field(TRANSPORT, Value::from(interface.transport)),
        field(TRANSPORT_ID, Value::Binary(interface.transport_id.to_vec())),
        field(NAME, Value::from(sanitize(&interface.name))),
        field(LATITUDE, option_f64(interface.latitude)),
        field(LONGITUDE, option_f64(interface.longitude)),
        field(HEIGHT, option_f64(interface.height)),
    ];
    if matches!(
        interface_type,
        "BackboneInterface" | "TCPServerInterface" | "TCPClientInterface" | "I2PInterface"
    ) {
        let endpoint =
            interface.reachable_on.as_deref().ok_or(DiscoveryAnnounceError::MissingEndpoint)?;
        if !super::valid_endpoint(endpoint) {
            return Err(DiscoveryAnnounceError::InvalidEndpoint);
        }
        fields.push(field(REACHABLE_ON, Value::from(sanitize(endpoint))));
    }
    if matches!(interface_type, "BackboneInterface" | "TCPServerInterface" | "TCPClientInterface") {
        fields.push(field(
            PORT,
            Value::from(interface.port.ok_or(DiscoveryAnnounceError::MissingEndpoint)?),
        ));
    }
    if let Some(value) = &interface.ifac_netname {
        fields.push(field(IFAC_NETNAME, Value::from(sanitize(value))));
    }
    if let Some(value) = &interface.ifac_netkey {
        fields.push(field(IFAC_NETKEY, Value::from(sanitize(value))));
    }
    if let Some(address) = interface.operator_lxmf_address {
        fields.push(field(OPERATOR_LXMF_ADDRESS, Value::Binary(address.to_vec())));
    }
    match interface_type {
        "RNodeInterface" => {
            fields.push(required_u64(FREQUENCY, interface.frequency, "frequency")?);
            fields.push(required_u64(BANDWIDTH, interface.bandwidth, "bandwidth")?);
            fields.push(required_u8(
                SPREADING_FACTOR,
                interface.spreading_factor,
                "spreading_factor",
            )?);
            fields.push(required_u8(CODING_RATE, interface.coding_rate, "coding_rate")?);
        }
        "WeaveInterface" => {
            fields.push(required_u64(FREQUENCY, interface.frequency, "frequency")?);
            fields.push(required_u64(BANDWIDTH, interface.bandwidth, "bandwidth")?);
            fields.push(required_u64(CHANNEL, interface.channel, "channel")?);
            fields.push(required_string(
                MODULATION,
                interface.modulation.as_deref(),
                "modulation",
            )?);
        }
        "KISSInterface" => {
            fields.push(required_u64(FREQUENCY, interface.frequency, "frequency")?);
            fields.push(required_u64(BANDWIDTH, interface.bandwidth, "bandwidth")?);
            fields.push(required_string(
                MODULATION,
                interface.modulation.as_deref(),
                "modulation",
            )?);
        }
        _ => {}
    }
    let mut packed = Vec::new();
    rmpv::encode::write_value(&mut packed, &Value::Map(fields))
        .map_err(|_| DiscoveryAnnounceError::InvalidMessagePack)?;
    Ok(packed)
}

fn decode_interface(
    packed: &[u8],
    stamp: &[u8],
    value: u32,
    network_id: &str,
    hops: u8,
    received: f64,
) -> Result<DiscoveredInterface, DiscoveryAnnounceError> {
    let decoded = rmpv::decode::read_value(&mut io::Cursor::new(packed))
        .map_err(|_| DiscoveryAnnounceError::InvalidMessagePack)?;
    let map = decoded.as_map().ok_or(DiscoveryAnnounceError::InvalidMessagePack)?;
    let interface_type = string(map, INTERFACE_TYPE)?;
    let transport_id = binary(map, TRANSPORT_ID)?;
    let name = string(map, NAME)?;
    let discovery_hash =
        Sha256::digest(format!("{}{}", hex::encode(&transport_id), name).as_bytes()).to_vec();
    let reachable_on = optional_string(map, REACHABLE_ON)?;
    if reachable_on.as_deref().is_some_and(|endpoint| !super::valid_endpoint(endpoint)) {
        return Err(DiscoveryAnnounceError::InvalidEndpoint);
    }
    Ok(DiscoveredInterface {
        discovery_hash,
        interface_type,
        transport: boolean(map, TRANSPORT)?,
        name,
        received,
        stamp: stamp.to_vec(),
        value: u64::from(value),
        transport_id: hex::encode(transport_id),
        network_id: network_id.to_string(),
        hops,
        latitude: optional_f64(map, LATITUDE)?,
        longitude: optional_f64(map, LONGITUDE)?,
        height: optional_f64(map, HEIGHT)?,
        operator_lxmf_address: optional_binary(map, OPERATOR_LXMF_ADDRESS)?.map(hex::encode),
        reachable_on,
        port: optional_u64(map, PORT)?.map(|port| port as u16),
        ifac_netname: optional_string(map, IFAC_NETNAME)?,
        ifac_netkey: optional_string(map, IFAC_NETKEY)?,
        config_entry: None,
        discovered: 0.0,
        last_heard: 0.0,
        heard_count: 0,
        status: Default::default(),
        status_code: 0,
    })
}

fn field(key: i64, value: Value) -> (Value, Value) {
    (Value::from(key), value)
}
fn option_f64(value: Option<f64>) -> Value {
    value.map_or(Value::Nil, Value::F64)
}
fn required_u64(
    key: i64,
    value: Option<u64>,
    name: &'static str,
) -> Result<(Value, Value), DiscoveryAnnounceError> {
    value
        .map(|value| field(key, Value::from(value)))
        .ok_or(DiscoveryAnnounceError::MissingRadioField(name))
}
fn required_u8(
    key: i64,
    value: Option<u8>,
    name: &'static str,
) -> Result<(Value, Value), DiscoveryAnnounceError> {
    required_u64(key, value.map(u64::from), name)
}
fn required_string(
    key: i64,
    value: Option<&str>,
    name: &'static str,
) -> Result<(Value, Value), DiscoveryAnnounceError> {
    value
        .map(|value| field(key, Value::from(sanitize(value))))
        .ok_or(DiscoveryAnnounceError::MissingRadioField(name))
}
fn value(map: &[(Value, Value)], key: i64) -> Result<&Value, DiscoveryAnnounceError> {
    map.iter()
        .find(|(candidate, _)| candidate.as_i64() == Some(key))
        .map(|(_, value)| value)
        .ok_or(DiscoveryAnnounceError::MissingField(key))
}
fn optional(map: &[(Value, Value)], key: i64) -> Option<&Value> {
    map.iter()
        .find(|(candidate, _)| candidate.as_i64() == Some(key))
        .map(|(_, value)| value)
        .filter(|value| !value.is_nil())
}
fn optional_binary(
    map: &[(Value, Value)],
    key: i64,
) -> Result<Option<Vec<u8>>, DiscoveryAnnounceError> {
    optional(map, key)
        .map(|value| {
            value.as_slice().map(ToOwned::to_owned).ok_or(DiscoveryAnnounceError::InvalidField(key))
        })
        .transpose()
        .map(|value| value.filter(|bytes| bytes.len() == 16))
}
fn string(map: &[(Value, Value)], key: i64) -> Result<String, DiscoveryAnnounceError> {
    value(map, key)?
        .as_str()
        .map(ToOwned::to_owned)
        .ok_or(DiscoveryAnnounceError::InvalidField(key))
}
fn optional_string(
    map: &[(Value, Value)],
    key: i64,
) -> Result<Option<String>, DiscoveryAnnounceError> {
    optional(map, key)
        .map(|value| {
            value.as_str().map(ToOwned::to_owned).ok_or(DiscoveryAnnounceError::InvalidField(key))
        })
        .transpose()
}
fn binary(map: &[(Value, Value)], key: i64) -> Result<Vec<u8>, DiscoveryAnnounceError> {
    value(map, key)?
        .as_slice()
        .map(ToOwned::to_owned)
        .ok_or(DiscoveryAnnounceError::InvalidField(key))
}
fn boolean(map: &[(Value, Value)], key: i64) -> Result<bool, DiscoveryAnnounceError> {
    value(map, key)?.as_bool().ok_or(DiscoveryAnnounceError::InvalidField(key))
}
fn optional_f64(map: &[(Value, Value)], key: i64) -> Result<Option<f64>, DiscoveryAnnounceError> {
    optional(map, key)
        .map(|value| value.as_f64().ok_or(DiscoveryAnnounceError::InvalidField(key)))
        .transpose()
}
fn optional_u64(map: &[(Value, Value)], key: i64) -> Result<Option<u64>, DiscoveryAnnounceError> {
    optional(map, key)
        .map(|value| value.as_u64().ok_or(DiscoveryAnnounceError::InvalidField(key)))
        .transpose()
}

#[cfg(test)]
mod tests {
    include!("announce_tests.rs");
}
