impl<'de> Deserialize<'de> for PeerRecord {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let wire = PeerRecordWire::deserialize(deserializer)?;
        let peer = wire
            .peer
            .map(PythonHexId::into_string)
            .or_else(|| wire.destination_hash.map(PythonHexId::into_string))
            .ok_or_else(|| serde::de::Error::missing_field("peer"))?;
        let last_seen = if let Some(value) = wire.last_seen.as_ref() {
            parse_python_timestamp_i64(value).map_err(serde::de::Error::custom)?
        } else if let Some(value) = wire.last_heard.as_ref() {
            parse_python_timestamp_i64(value).map_err(serde::de::Error::custom)?
        } else {
            return Err(serde::de::Error::missing_field("last_seen"));
        };
        let last_sync_attempt = wire
            .last_sync_attempt
            .as_ref()
            .map(parse_python_timestamp_i64)
            .transpose()
            .map_err(serde::de::Error::custom)?
            .unwrap_or_default();
        let next_sync_attempt = wire
            .next_sync_attempt
            .as_ref()
            .map(parse_python_timestamp_i64)
            .transpose()
            .map_err(serde::de::Error::custom)?
            .unwrap_or_default();
        let peering_timebase = wire
            .peering_timebase
            .as_ref()
            .map(parse_python_timestamp_i64)
            .transpose()
            .map_err(serde::de::Error::custom)?
            .unwrap_or_default();
        let sync_transfer_rate = wire.sync_transfer_rate.or(wire.str).unwrap_or_default();
        let offered = wire
            .offered
            .as_ref()
            .map(parse_python_int_u64)
            .transpose()
            .map_err(serde::de::Error::custom)?
            .unwrap_or_default();
        let outgoing = wire
            .outgoing
            .as_ref()
            .map(parse_python_int_u64)
            .transpose()
            .map_err(serde::de::Error::custom)?
            .unwrap_or_default();
        let incoming = wire
            .incoming
            .as_ref()
            .map(parse_python_int_u64)
            .transpose()
            .map_err(serde::de::Error::custom)?
            .unwrap_or_default();
        let rx_bytes = wire
            .rx_bytes
            .as_ref()
            .map(parse_python_int_u64)
            .transpose()
            .map_err(serde::de::Error::custom)?
            .unwrap_or_default();
        let tx_bytes = wire
            .tx_bytes
            .as_ref()
            .map(parse_python_int_u64)
            .transpose()
            .map_err(serde::de::Error::custom)?
            .unwrap_or_default();
        let acceptance_rate = wire.acceptance_rate.unwrap_or_else(|| {
            if offered == 0 {
                0.0
            } else {
                (outgoing as f64 / offered as f64).max(0.0)
            }
        });
        let python_transfer_limit = wire.propagation_transfer_limit.is_some();
        let transfer_limit = parse_peer_limit_bytes(
            wire.propagation_transfer_limit.as_ref(),
            wire.transfer_limit.as_ref(),
            python_transfer_limit,
        );
        let python_sync_limit = wire.propagation_sync_limit.is_some();
        let sync_limit = parse_peer_sync_limit_bytes(
            wire.propagation_sync_limit.as_ref(),
            wire.sync_limit.as_ref(),
            python_sync_limit,
        )
        .or_else(|| python_transfer_limit.then_some(transfer_limit).flatten());
        let (peering_key_stamp, peering_key_value) =
            wire.peering_key.map(PythonPeeringKey::into_parts).unwrap_or_default();
        Ok(Self {
            peer,
            last_seen,
            capabilities: wire.capabilities,
            name: wire.name,
            name_source: wire.name_source,
            metadata: wire.metadata,
            peer_type: wire.peer_type,
            alive: wire.alive,
            last_sync_attempt,
            next_sync_attempt,
            sync_backoff: wire.sync_backoff,
            sync_schedule_reason: wire.sync_schedule_reason,
            network_distance: wire.network_distance,
            offered,
            outgoing,
            incoming,
            rx_bytes,
            tx_bytes,
            sync_transfer_rate,
            acceptance_rate,
            first_seen: wire.first_seen.unwrap_or(last_seen),
            seen_count: wire.seen_count.unwrap_or_else(|| u64::from(last_seen > 0)),
            peering_timebase,
            sync_strategy: wire
                .sync_strategy
                .as_ref()
                .map(parse_python_int_u8)
                .transpose()
                .map_err(serde::de::Error::custom)?
                .unwrap_or_else(default_peer_sync_strategy),
            propagation_transfer_limit: transfer_limit,
            propagation_sync_limit: sync_limit,
            propagation_stamp_cost: wire
                .propagation_stamp_cost
                .as_ref()
                .and_then(|v| parse_python_int_u32(v).ok())
                .or_else(|| {
                    wire.target_stamp_cost
                        .as_ref()
                        .and_then(|v| parse_python_int_u32(v).ok())
                }),
            propagation_stamp_cost_flexibility: wire
                .propagation_stamp_cost_flexibility
                .as_ref()
                .and_then(|v| parse_python_int_u32(v).ok())
                .or_else(|| {
                    wire.stamp_cost_flexibility
                        .as_ref()
                        .and_then(|v| parse_python_int_u32(v).ok())
                }),
            peering_cost: wire.peering_cost.as_ref().and_then(|v| parse_python_int_u32(v).ok()),
            peering_key_stamp,
            peering_key_value,
            restored_handled_ids: wire
                .handled_ids
                .into_iter()
                .map(PythonHexId::into_string)
                .collect(),
            restored_unhandled_ids: wire
                .unhandled_ids
                .into_iter()
                .map(PythonHexId::into_string)
                .collect(),
        })
    }
}

struct PythonHexId(String);

impl PythonHexId {
    fn into_string(self) -> String {
        self.0
    }
}

impl<'de> Deserialize<'de> for PythonHexId {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        struct PythonHexIdVisitor;

        impl Visitor<'_> for PythonHexIdVisitor {
            type Value = PythonHexId;

            fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter.write_str("a hex string or MessagePack binary hash")
            }

            fn visit_str<E>(self, value: &str) -> Result<Self::Value, E>
            where
                E: serde::de::Error,
            {
                Ok(PythonHexId(value.trim().to_ascii_lowercase()))
            }

            fn visit_string<E>(self, value: String) -> Result<Self::Value, E>
            where
                E: serde::de::Error,
            {
                self.visit_str(value.as_str())
            }

            fn visit_bytes<E>(self, value: &[u8]) -> Result<Self::Value, E>
            where
                E: serde::de::Error,
            {
                Ok(PythonHexId(hex::encode(value)))
            }

            fn visit_byte_buf<E>(self, value: Vec<u8>) -> Result<Self::Value, E>
            where
                E: serde::de::Error,
            {
                self.visit_bytes(value.as_slice())
            }
        }

        deserializer.deserialize_any(PythonHexIdVisitor)
    }
}

struct PythonPeeringKey {
    stamp: Option<Vec<u8>>,
    value: Option<u32>,
}

impl PythonPeeringKey {
    fn value(value: Option<u32>) -> Self {
        Self { stamp: None, value }
    }

    fn into_parts(self) -> (Option<Vec<u8>>, Option<u32>) {
        (self.stamp, self.value)
    }
}

impl<'de> Deserialize<'de> for PythonPeeringKey {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        struct PythonPeeringKeyVisitor;

        impl<'de> Visitor<'de> for PythonPeeringKeyVisitor {
            type Value = PythonPeeringKey;

            fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter.write_str("a peering key value or [stamp, value] pair")
            }

            fn visit_u64<E>(self, value: u64) -> Result<Self::Value, E> {
                Ok(PythonPeeringKey::value(u32::try_from(value).ok()))
            }

            fn visit_i64<E>(self, value: i64) -> Result<Self::Value, E> {
                Ok(PythonPeeringKey::value(u32::try_from(value.max(0)).ok()))
            }

            fn visit_f64<E>(self, value: f64) -> Result<Self::Value, E> {
                let value = value.max(0.0).floor();
                Ok(PythonPeeringKey::value(
                    (value.is_finite() && value <= f64::from(u32::MAX)).then_some(value as u32),
                ))
            }

            fn visit_str<E>(self, value: &str) -> Result<Self::Value, E> {
                let value = value.trim().parse::<f64>().ok().and_then(|value| {
                    let value = value.max(0.0).floor();
                    (value.is_finite() && value <= f64::from(u32::MAX)).then_some(value as u32)
                });
                Ok(PythonPeeringKey::value(value))
            }

            fn visit_string<E>(self, value: String) -> Result<Self::Value, E>
            where
                E: serde::de::Error,
            {
                self.visit_str(value.as_str())
            }

            fn visit_seq<A>(self, mut sequence: A) -> Result<Self::Value, A::Error>
            where
                A: serde::de::SeqAccess<'de>,
            {
                let stamp = sequence
                    .next_element::<PythonPeeringKeyStamp>()?
                    .and_then(PythonPeeringKeyStamp::into_bytes);
                let value = sequence.next_element::<JsonValue>()?;
                Ok(PythonPeeringKey {
                    stamp,
                    value: value
                        .as_ref()
                        .map(|v| parse_json_u32(v).map_err(serde::de::Error::custom))
                        .transpose()?
                        .flatten(),
                })
            }
        }

        deserializer.deserialize_any(PythonPeeringKeyVisitor)
    }
}

struct PythonPeeringKeyStamp(Option<Vec<u8>>);

impl PythonPeeringKeyStamp {
    fn into_bytes(self) -> Option<Vec<u8>> {
        self.0
    }
}

impl<'de> Deserialize<'de> for PythonPeeringKeyStamp {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        struct PythonPeeringKeyStampVisitor;

        impl<'de> Visitor<'de> for PythonPeeringKeyStampVisitor {
            type Value = PythonPeeringKeyStamp;

            fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter.write_str("a nil, string, byte array, or MessagePack binary stamp")
            }

            fn visit_unit<E>(self) -> Result<Self::Value, E> {
                Ok(PythonPeeringKeyStamp(None))
            }

            fn visit_bytes<E>(self, value: &[u8]) -> Result<Self::Value, E>
            where
                E: serde::de::Error,
            {
                Ok(PythonPeeringKeyStamp(Some(value.to_vec())))
            }

            fn visit_byte_buf<E>(self, value: Vec<u8>) -> Result<Self::Value, E>
            where
                E: serde::de::Error,
            {
                Ok(PythonPeeringKeyStamp(Some(value)))
            }

            fn visit_str<E>(self, value: &str) -> Result<Self::Value, E>
            where
                E: serde::de::Error,
            {
                Ok(PythonPeeringKeyStamp(Some(value.as_bytes().to_vec())))
            }

            fn visit_string<E>(self, value: String) -> Result<Self::Value, E>
            where
                E: serde::de::Error,
            {
                self.visit_str(value.as_str())
            }

            fn visit_seq<A>(self, mut sequence: A) -> Result<Self::Value, A::Error>
            where
                A: serde::de::SeqAccess<'de>,
            {
                let mut bytes = Vec::new();
                while let Some(byte) = sequence.next_element::<u8>()? {
                    bytes.push(byte);
                }
                Ok(PythonPeeringKeyStamp(Some(bytes)))
            }
        }

        deserializer.deserialize_any(PythonPeeringKeyStampVisitor)
    }
}

fn parse_peer_limit_bytes(
    primary: Option<&JsonValue>,
    alias: Option<&JsonValue>,
    primary_is_python_kb: bool,
) -> Option<u32> {
    if let Some(alias) = alias {
        let alias_bytes = parse_json_u32(alias).ok().flatten()?;
        if primary_is_python_kb {
            if let Some(primary) = primary {
                let Some(primary_kb) = parse_json_f64(primary) else {
                    return Some(alias_bytes);
                };
                if kilobytes_to_bytes(primary_kb) == Some(alias_bytes) {
                    return Some(alias_bytes);
                }
                if primary_kb == 0.0 && alias_bytes > 0 {
                    return parse_json_u32(primary).ok().flatten();
                }
                return Some(alias_bytes);
            }
        }
        Some(alias_bytes)
    } else if primary_is_python_kb {
        parse_json_f64(primary?).and_then(kilobytes_to_bytes)
    } else {
        parse_json_u32(primary?).ok().flatten()
    }
}

fn parse_peer_sync_limit_bytes(
    primary: Option<&JsonValue>,
    alias: Option<&JsonValue>,
    primary_is_python_kb: bool,
) -> Option<u32> {
    if let Some(alias) = alias {
        parse_json_u32(alias).ok().flatten()
    } else if primary_is_python_kb {
        parse_python_sync_limit_bytes(primary?)
    } else {
        parse_json_u32(primary?).ok().flatten()
    }
}

fn parse_json_u32(value: &JsonValue) -> Result<Option<u32>, &'static str> {
    if value.is_null() {
        return Ok(None);
    }
    if let Some(value) = value.as_u64() {
        return u32::try_from(value).map(Some).map_err(|_| "value out of u32 range");
    }
    if let Some(value) = value.as_i64() {
        return u32::try_from(value.max(0)).map(Some).map_err(|_| "value out of u32 range");
    }
    if let Some(value) = parse_json_f64(value) {
        let bytes = value.max(0.0).floor();
        if bytes.is_finite() && bytes <= f64::from(u32::MAX) {
            return Ok(Some(bytes as u32));
        }
        return Err("float value out of u32 range");
    }
    Err("not a number")
}

fn parse_json_f64(value: &JsonValue) -> Option<f64> {
    value.as_f64().or_else(|| value.as_str()?.trim().parse::<f64>().ok())
}

fn kilobytes_to_bytes(value: f64) -> Option<u32> {
    let bytes = (value.max(0.0) * 1000.0).floor();
    (bytes.is_finite() && bytes <= f64::from(u32::MAX)).then_some(bytes as u32)
}

fn parse_python_sync_limit_bytes(value: &JsonValue) -> Option<u32> {
    let kilobytes = f64::from(parse_python_int_u32(value).ok()?);
    kilobytes_to_bytes(kilobytes)
}

fn parse_python_int_u32(value: &JsonValue) -> Result<u32, &'static str> {
    if let Some(value) = value.as_u64() {
        u32::try_from(value).map_err(|_| "value out of u32 range")
    } else if let Some(value) = value.as_i64() {
        u32::try_from(value.max(0)).map_err(|_| "value out of u32 range")
    } else if let Some(value) = value.as_f64() {
        let value = value.max(0.0).trunc();
        if value.is_finite() && value <= f64::from(u32::MAX) {
            Ok(value as u32)
        } else {
            Err("float value out of u32 range")
        }
    } else if let Some(value) = value.as_bool() {
        Ok(u32::from(value))
    } else if let Some(value) = value.as_str() {
        let parsed = value.trim().parse::<i64>().map_err(|_| "invalid integer string")?;
        u32::try_from(parsed.max(0)).map_err(|_| "value out of u32 range")
    } else {
        Err("unsupported JSON type for integer")
    }
}
