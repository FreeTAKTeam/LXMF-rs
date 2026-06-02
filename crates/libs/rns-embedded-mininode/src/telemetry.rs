use alloc::{
    collections::BTreeMap,
    string::{String, ToString},
    vec,
    vec::Vec,
};

use rmpv::Value;

use crate::error::MiniNodeError;

#[derive(Debug, Clone, PartialEq)]
pub enum TelemetryValue {
    Integer(i64),
    Unsigned(u64),
    Float(f64),
    Bool(bool),
    Text(String),
}

impl TelemetryValue {
    pub fn to_rmpv(&self) -> Value {
        match self {
            Self::Integer(value) => Value::Integer((*value).into()),
            Self::Unsigned(value) => Value::Integer((*value).into()),
            Self::Float(value) => Value::F64(*value),
            Self::Bool(value) => Value::Boolean(*value),
            Self::Text(value) => Value::String(value.as_str().into()),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct TelemetryPoint {
    pub ts_ms: u64,
    pub key: String,
    pub value: TelemetryValue,
    pub unit: Option<String>,
    pub tags: BTreeMap<String, String>,
}

impl TelemetryPoint {
    pub fn to_rmpv(&self) -> Value {
        let mut entries = vec![
            (Value::String("ts_ms".into()), Value::Integer(self.ts_ms.into())),
            (Value::String("key".into()), Value::String(self.key.as_str().into())),
            (Value::String("value".into()), self.value.to_rmpv()),
        ];

        if let Some(unit) = &self.unit {
            entries.push((Value::String("unit".into()), Value::String(unit.as_str().into())));
        }

        if !self.tags.is_empty() {
            let tags = self
                .tags
                .iter()
                .map(|(key, value)| {
                    (Value::String(key.as_str().into()), Value::String(value.as_str().into()))
                })
                .collect();
            entries.push((Value::String("tags".into()), Value::Map(tags)));
        }

        Value::Map(entries)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TelemetryQuery {
    pub from_ts_ms: Option<u64>,
    pub to_ts_ms: Option<u64>,
    pub key_prefix: Option<String>,
    pub limit: Option<usize>,
}

impl TelemetryQuery {
    pub fn matches(&self, point: &TelemetryPoint) -> bool {
        if let Some(from) = self.from_ts_ms {
            if point.ts_ms < from {
                return false;
            }
        }
        if let Some(to) = self.to_ts_ms {
            if point.ts_ms > to {
                return false;
            }
        }
        if let Some(prefix) = &self.key_prefix {
            if !point.key.starts_with(prefix) {
                return false;
            }
        }
        true
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PositionFix {
    pub ts_ms: u64,
    pub lat: f64,
    pub lon: f64,
    pub alt_m: Option<f64>,
    pub speed_mps: Option<f64>,
    pub heading_deg: Option<f64>,
    pub accuracy_m: Option<f64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BatteryStatus {
    pub ts_ms: u64,
    pub pct: u8,
    pub millivolts: u16,
    pub charging: bool,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LinkStats {
    pub ts_ms: u64,
    pub rssi_dbm: Option<i16>,
    pub snr_db: Option<f32>,
    pub airtime_ms: u32,
    pub rx_ok: u32,
    pub rx_drop: u32,
    pub tx_ok: u32,
    pub tx_drop: u32,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DeviceHealth {
    pub ts_ms: u64,
    pub uptime_ms: u64,
    pub free_bytes: Option<u32>,
    pub temperature_c: Option<f32>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum TelemetrySample {
    PositionFix(PositionFix),
    BatteryStatus(BatteryStatus),
    LinkStats(LinkStats),
    DeviceHealth(DeviceHealth),
    Point(TelemetryPoint),
}

impl TelemetrySample {
    pub fn into_points(self, source_hash: [u8; 16]) -> (Vec<TelemetryPoint>, Option<PositionFix>) {
        let mut tags = BTreeMap::new();
        tags.insert("source".to_string(), hex::encode(source_hash));

        match self {
            Self::PositionFix(fix) => {
                let mut points = vec![
                    TelemetryPoint {
                        ts_ms: fix.ts_ms,
                        key: "position.lat".to_string(),
                        value: TelemetryValue::Float(fix.lat),
                        unit: Some("deg".to_string()),
                        tags: tags.clone(),
                    },
                    TelemetryPoint {
                        ts_ms: fix.ts_ms,
                        key: "position.lon".to_string(),
                        value: TelemetryValue::Float(fix.lon),
                        unit: Some("deg".to_string()),
                        tags: tags.clone(),
                    },
                ];

                if let Some(alt_m) = fix.alt_m {
                    points.push(TelemetryPoint {
                        ts_ms: fix.ts_ms,
                        key: "position.alt_m".to_string(),
                        value: TelemetryValue::Float(alt_m),
                        unit: Some("m".to_string()),
                        tags: tags.clone(),
                    });
                }
                if let Some(speed_mps) = fix.speed_mps {
                    points.push(TelemetryPoint {
                        ts_ms: fix.ts_ms,
                        key: "position.speed_mps".to_string(),
                        value: TelemetryValue::Float(speed_mps),
                        unit: Some("m/s".to_string()),
                        tags: tags.clone(),
                    });
                }
                if let Some(heading_deg) = fix.heading_deg {
                    points.push(TelemetryPoint {
                        ts_ms: fix.ts_ms,
                        key: "position.heading_deg".to_string(),
                        value: TelemetryValue::Float(heading_deg),
                        unit: Some("deg".to_string()),
                        tags: tags.clone(),
                    });
                }
                if let Some(accuracy_m) = fix.accuracy_m {
                    points.push(TelemetryPoint {
                        ts_ms: fix.ts_ms,
                        key: "position.accuracy_m".to_string(),
                        value: TelemetryValue::Float(accuracy_m),
                        unit: Some("m".to_string()),
                        tags,
                    });
                }
                (points, Some(fix))
            }
            Self::BatteryStatus(status) => (
                vec![
                    TelemetryPoint {
                        ts_ms: status.ts_ms,
                        key: "battery.pct".to_string(),
                        value: TelemetryValue::Unsigned(u64::from(status.pct)),
                        unit: Some("%".to_string()),
                        tags: tags.clone(),
                    },
                    TelemetryPoint {
                        ts_ms: status.ts_ms,
                        key: "battery.mv".to_string(),
                        value: TelemetryValue::Unsigned(u64::from(status.millivolts)),
                        unit: Some("mV".to_string()),
                        tags: tags.clone(),
                    },
                    TelemetryPoint {
                        ts_ms: status.ts_ms,
                        key: "battery.charging".to_string(),
                        value: TelemetryValue::Bool(status.charging),
                        unit: None,
                        tags,
                    },
                ],
                None,
            ),
            Self::LinkStats(stats) => {
                let mut points = vec![
                    TelemetryPoint {
                        ts_ms: stats.ts_ms,
                        key: "link.airtime_ms".to_string(),
                        value: TelemetryValue::Unsigned(u64::from(stats.airtime_ms)),
                        unit: Some("ms".to_string()),
                        tags: tags.clone(),
                    },
                    TelemetryPoint {
                        ts_ms: stats.ts_ms,
                        key: "link.rx_ok".to_string(),
                        value: TelemetryValue::Unsigned(u64::from(stats.rx_ok)),
                        unit: None,
                        tags: tags.clone(),
                    },
                    TelemetryPoint {
                        ts_ms: stats.ts_ms,
                        key: "link.rx_drop".to_string(),
                        value: TelemetryValue::Unsigned(u64::from(stats.rx_drop)),
                        unit: None,
                        tags: tags.clone(),
                    },
                    TelemetryPoint {
                        ts_ms: stats.ts_ms,
                        key: "link.tx_ok".to_string(),
                        value: TelemetryValue::Unsigned(u64::from(stats.tx_ok)),
                        unit: None,
                        tags: tags.clone(),
                    },
                    TelemetryPoint {
                        ts_ms: stats.ts_ms,
                        key: "link.tx_drop".to_string(),
                        value: TelemetryValue::Unsigned(u64::from(stats.tx_drop)),
                        unit: None,
                        tags: tags.clone(),
                    },
                ];

                if let Some(rssi) = stats.rssi_dbm {
                    points.push(TelemetryPoint {
                        ts_ms: stats.ts_ms,
                        key: "link.rssi_dbm".to_string(),
                        value: TelemetryValue::Integer(i64::from(rssi)),
                        unit: Some("dBm".to_string()),
                        tags: tags.clone(),
                    });
                }
                if let Some(snr) = stats.snr_db {
                    points.push(TelemetryPoint {
                        ts_ms: stats.ts_ms,
                        key: "link.snr_db".to_string(),
                        value: TelemetryValue::Float(f64::from(snr)),
                        unit: Some("dB".to_string()),
                        tags,
                    });
                }

                (points, None)
            }
            Self::DeviceHealth(health) => {
                let mut points = vec![TelemetryPoint {
                    ts_ms: health.ts_ms,
                    key: "device.uptime_ms".to_string(),
                    value: TelemetryValue::Unsigned(health.uptime_ms),
                    unit: Some("ms".to_string()),
                    tags: tags.clone(),
                }];

                if let Some(free_bytes) = health.free_bytes {
                    points.push(TelemetryPoint {
                        ts_ms: health.ts_ms,
                        key: "device.free_bytes".to_string(),
                        value: TelemetryValue::Unsigned(u64::from(free_bytes)),
                        unit: Some("bytes".to_string()),
                        tags: tags.clone(),
                    });
                }
                if let Some(temperature_c) = health.temperature_c {
                    points.push(TelemetryPoint {
                        ts_ms: health.ts_ms,
                        key: "device.temperature_c".to_string(),
                        value: TelemetryValue::Float(f64::from(temperature_c)),
                        unit: Some("C".to_string()),
                        tags,
                    });
                }

                (points, None)
            }
            Self::Point(mut point) => {
                point.tags.entry("source".to_string()).or_insert_with(|| hex::encode(source_hash));
                (vec![point], None)
            }
        }
    }
}

pub fn encode_recent_fields(
    points: &[TelemetryPoint],
    latest_position: Option<&PositionFix>,
) -> Result<Value, MiniNodeError> {
    let mut entries = Vec::new();

    if let Some(position) = latest_position {
        entries.push((Value::Integer(2.into()), Value::Binary(encode_position_payload(position)?)));
    }

    if !points.is_empty() {
        let telemetry_values = points.iter().map(TelemetryPoint::to_rmpv).collect::<Vec<_>>();
        let encoded = rmp_serde::to_vec(&Value::Array(telemetry_values))
            .map_err(|_| MiniNodeError::StorageError)?;
        entries.push((Value::Integer(3.into()), Value::Binary(encoded)));
    }

    Ok(Value::Map(entries))
}

fn encode_position_payload(position: &PositionFix) -> Result<Vec<u8>, MiniNodeError> {
    let lat = ((position.lat * 1_000_000.0).round()) as i32;
    let lon = ((position.lon * 1_000_000.0).round()) as i32;
    let alt = ((position.alt_m.unwrap_or_default() * 100.0).round()) as i32;
    let speed = ((position.speed_mps.unwrap_or_default() * 100.0).round()) as u32;
    let heading = ((position.heading_deg.unwrap_or_default() * 100.0).round()) as i32;
    let accuracy = ((position.accuracy_m.unwrap_or_default() * 100.0).round()) as u16;

    let position_array = Value::Array(vec![
        Value::Binary(lat.to_be_bytes().to_vec()),
        Value::Binary(lon.to_be_bytes().to_vec()),
        Value::Binary(alt.to_be_bytes().to_vec()),
        Value::Binary(speed.to_be_bytes().to_vec()),
        Value::Binary(heading.to_be_bytes().to_vec()),
        Value::Binary(accuracy.to_be_bytes().to_vec()),
        Value::Integer(position.ts_ms.into()),
    ]);

    let payload = Value::Map(vec![(Value::Integer(0x02.into()), position_array)]);
    rmp_serde::to_vec(&payload).map_err(|_| MiniNodeError::StorageError)
}
