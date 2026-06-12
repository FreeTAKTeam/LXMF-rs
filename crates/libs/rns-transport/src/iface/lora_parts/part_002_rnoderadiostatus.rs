impl RNodeRadioStatus {
    #[must_use]
    pub const fn battery_state_string(&self) -> &'static str {
        match self.battery_state {
            Some(BATTERY_STATE_CHARGED) => "charged",
            Some(BATTERY_STATE_CHARGING) => "charging",
            Some(BATTERY_STATE_DISCHARGING) => "discharging",
            _ => "unknown",
        }
    }

    pub fn accept_command(&mut self, command: u8, payload: &[u8]) -> Result<bool, String> {
        match command {
            CMD_FREQUENCY => {
                self.frequency_hz = Some(u32_from_payload(command, payload, "frequency")?);
                Ok(true)
            }
            CMD_BANDWIDTH => {
                self.bandwidth_hz = Some(u32_from_payload(command, payload, "bandwidth")?);
                Ok(true)
            }
            CMD_TXPOWER => {
                self.tx_power_dbm = Some(single_byte_payload(payload, "tx power")?);
                Ok(true)
            }
            CMD_SF => {
                self.spreading_factor = Some(single_byte_payload(payload, "spreading factor")?);
                Ok(true)
            }
            CMD_CR => {
                self.coding_rate = Some(single_byte_payload(payload, "coding rate")?);
                Ok(true)
            }
            CMD_RADIO_STATE => {
                self.radio_state = Some(single_byte_payload(payload, "radio state")?);
                Ok(true)
            }
            CMD_RADIO_LOCK => {
                self.radio_lock = Some(single_byte_payload(payload, "radio lock")?);
                Ok(true)
            }
            CMD_STAT_RX => {
                self.stat_rx = Some(u32_from_payload(command, payload, "rx stat")?);
                Ok(true)
            }
            CMD_STAT_TX => {
                self.stat_tx = Some(u32_from_payload(command, payload, "tx stat")?);
                Ok(true)
            }
            CMD_STAT_RSSI => {
                self.rssi_dbm =
                    Some(i16::from(single_byte_payload(payload, "rssi")?) - RSSI_OFFSET);
                Ok(true)
            }
            CMD_STAT_SNR => {
                let snr_db =
                    f64::from(i8::from_be_bytes([single_byte_payload(payload, "snr")?])) * 0.25;
                self.snr_db = Some(snr_db);
                self.signal_quality_percent = self.spreading_factor.and_then(|sf| {
                    let q_snr_min = Q_SNR_MIN_BASE - f64::from(sf.saturating_sub(7)) * Q_SNR_STEP;
                    let q_snr_span = Q_SNR_MAX - q_snr_min;
                    if q_snr_span == 0.0 {
                        return None;
                    }
                    Some(round_one_decimal(
                        ((snr_db - q_snr_min) / q_snr_span * 100.0).clamp(0.0, 100.0),
                    ))
                });
                Ok(true)
            }
            CMD_ST_ALOCK => {
                self.short_airtime_limit_percent = Some(
                    f64::from(u16_from_payload(command, payload, "short airtime limit")?) / 100.0,
                );
                Ok(true)
            }
            CMD_LT_ALOCK => {
                self.long_airtime_limit_percent = Some(
                    f64::from(u16_from_payload(command, payload, "long airtime limit")?) / 100.0,
                );
                Ok(true)
            }
            CMD_STAT_CHTM => {
                let [ats_hi, ats_lo, atl_hi, atl_lo, cus_hi, cus_lo, cul_hi, cul_lo, crs, nfl, ntf] =
                    payload
                else {
                    return Err(
                        "rnode channel telemetry response must contain eleven bytes".to_string()
                    );
                };
                self.airtime_short_percent =
                    Some(f64::from(u16::from_be_bytes([*ats_hi, *ats_lo])) / 100.0);
                self.airtime_long_percent =
                    Some(f64::from(u16::from_be_bytes([*atl_hi, *atl_lo])) / 100.0);
                self.channel_load_short_percent =
                    Some(f64::from(u16::from_be_bytes([*cus_hi, *cus_lo])) / 100.0);
                self.channel_load_long_percent =
                    Some(f64::from(u16::from_be_bytes([*cul_hi, *cul_lo])) / 100.0);
                self.current_rssi_dbm = Some(i16::from(*crs) - RSSI_OFFSET);
                self.noise_floor_dbm = Some(i16::from(*nfl) - RSSI_OFFSET);
                self.interference_dbm =
                    if *ntf == 0xff { None } else { Some(i16::from(*ntf) - RSSI_OFFSET) };
                Ok(true)
            }
            CMD_STAT_PHYPRM => {
                let [lst_hi, lst_lo, lsr_hi, lsr_lo, prs_hi, prs_lo, prt_hi, prt_lo, cst_hi, cst_lo, dft_hi, dft_lo] =
                    payload
                else {
                    return Err("rnode phy params response must contain twelve bytes".to_string());
                };
                self.symbol_time_ms =
                    Some(f64::from(u16::from_be_bytes([*lst_hi, *lst_lo])) / 1000.0);
                self.symbol_rate_baud = Some(u16::from_be_bytes([*lsr_hi, *lsr_lo]));
                self.preamble_symbols = Some(u16::from_be_bytes([*prs_hi, *prs_lo]));
                self.preamble_time_ms = Some(u16::from_be_bytes([*prt_hi, *prt_lo]));
                self.csma_slot_time_ms = Some(u16::from_be_bytes([*cst_hi, *cst_lo]));
                self.csma_difs_ms = Some(u16::from_be_bytes([*dft_hi, *dft_lo]));
                Ok(true)
            }
            CMD_STAT_CSMA => {
                let [band, min, max] = payload else {
                    return Err("rnode csma response must contain three bytes".to_string());
                };
                self.csma_cw_band = Some(*band);
                self.csma_cw_min = Some(*min);
                self.csma_cw_max = Some(*max);
                Ok(true)
            }
            CMD_STAT_BAT => {
                let [state, percent] = payload else {
                    return Err("rnode battery response must contain two bytes".to_string());
                };
                self.battery_state = Some(*state);
                self.battery_percent = Some((*percent).min(100));
                Ok(true)
            }
            CMD_STAT_TEMP => {
                let temp = i16::from(single_byte_payload(payload, "temperature")?) - 120;
                self.temperature_c = (-30..=90).contains(&temp).then_some(temp);
                Ok(true)
            }
            CMD_FB_READ => {
                self.framebuffer = Some(fixed_payload(payload, 512, "framebuffer")?);
                Ok(true)
            }
            CMD_DISP_READ => {
                self.display = Some(fixed_payload(payload, 1024, "display")?);
                Ok(true)
            }
            CMD_RANDOM => {
                self.random_byte = Some(single_byte_payload(payload, "random")?);
                Ok(true)
            }
            _ => Ok(false),
        }
    }

    pub fn validate_config(
        &self,
        config: LoraConfig,
        expected_radio_state: u8,
    ) -> Result<(), String> {
        if let Some(frequency_hz) = self.frequency_hz {
            let configured = i64::try_from(config.frequency_hz)
                .expect("validated LoRa frequency fits signed comparison range");
            let reported = i64::from(frequency_hz);
            if (configured - reported).abs() > 100 {
                return Err(format!(
                    "rnode frequency mismatch configured={} reported={}",
                    config.frequency_hz, frequency_hz
                ));
            }
        }
        match self.bandwidth_hz {
            Some(value) if value == config.bandwidth_hz => {}
            Some(value) => {
                return Err(format!(
                    "rnode bandwidth mismatch configured={} reported={}",
                    config.bandwidth_hz, value
                ));
            }
            None => return Err("rnode bandwidth response is missing".to_string()),
        }
        match self.tx_power_dbm {
            Some(value) if i8::try_from(value).ok() == Some(config.tx_power_dbm) => {}
            Some(value) => {
                return Err(format!(
                    "rnode tx power mismatch configured={} reported={}",
                    config.tx_power_dbm, value
                ));
            }
            None => return Err("rnode tx power response is missing".to_string()),
        }
        match self.spreading_factor {
            Some(value) if value == config.spreading_factor => {}
            Some(value) => {
                return Err(format!(
                    "rnode spreading factor mismatch configured={} reported={}",
                    config.spreading_factor, value
                ));
            }
            None => return Err("rnode spreading factor response is missing".to_string()),
        }
        match self.coding_rate {
            Some(value) if value == config.coding_rate => {}
            Some(value) => {
                return Err(format!(
                    "rnode coding rate mismatch configured={} reported={}",
                    config.coding_rate, value
                ));
            }
            None => return Err("rnode coding rate response is missing".to_string()),
        }
        match self.radio_state {
            Some(value) if value == expected_radio_state => {}
            Some(value) => {
                return Err(format!(
                    "rnode radio state mismatch configured={} reported={}",
                    expected_radio_state, value
                ));
            }
            None => return Err("rnode radio state response is missing".to_string()),
        }
        Ok(())
    }

    pub fn reported_bitrate_bps(&self) -> Option<f64> {
        let bandwidth_hz = f64::from(self.bandwidth_hz?);
        let spreading_factor = self.spreading_factor?;
        let coding_rate = self.coding_rate?;
        if coding_rate == 0 {
            return None;
        }
        let symbol_divisor = 2_u32.checked_pow(u32::from(spreading_factor))?;
        Some(
            f64::from(spreading_factor)
                * (4.0 / f64::from(coding_rate))
                * (bandwidth_hz / f64::from(symbol_divisor)),
        )
    }
}

fn u32_from_payload(command: u8, payload: &[u8], name: &str) -> Result<u32, String> {
    let bytes: [u8; 4] = payload.try_into().map_err(|_| {
        format!("rnode {name} response command=0x{command:02x} must contain four bytes")
    })?;
    Ok(u32::from_be_bytes(bytes))
}

fn u16_from_payload(command: u8, payload: &[u8], name: &str) -> Result<u16, String> {
    let bytes: [u8; 2] = payload.try_into().map_err(|_| {
        format!("rnode {name} response command=0x{command:02x} must contain two bytes")
    })?;
    Ok(u16::from_be_bytes(bytes))
}

fn single_byte_payload(payload: &[u8], name: &str) -> Result<u8, String> {
    let [value] = payload else {
        return Err(format!("rnode {name} response must contain one byte"));
    };
    Ok(*value)
}

fn fixed_payload(payload: &[u8], expected_len: usize, name: &str) -> Result<Vec<u8>, String> {
    if payload.len() != expected_len {
        return Err(format!("rnode {name} response must contain {expected_len} bytes"));
    }
    Ok(payload.to_vec())
}

fn round_one_decimal(value: f64) -> f64 {
    (value * 10.0).round() / 10.0
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LoraConfig {
    pub frequency_hz: u64,
    pub bandwidth_hz: u32,
    pub spreading_factor: u8,
    pub coding_rate: u8,
    pub tx_power_dbm: i8,
    pub max_payload_bytes: u16,
    pub airtime_limit_short_hundredths: Option<u16>,
    pub airtime_limit_long_hundredths: Option<u16>,
}

impl LoraConfig {
    #[must_use]
    pub const fn us915_default() -> Self {
        Self {
            frequency_hz: 915_000_000,
            bandwidth_hz: 125_000,
            spreading_factor: 9,
            coding_rate: 5,
            tx_power_dbm: 17,
            max_payload_bytes: 220,
            airtime_limit_short_hundredths: None,
            airtime_limit_long_hundredths: None,
        }
    }

    #[must_use]
    pub fn for_region(region: &str) -> Option<Self> {
        let frequency_hz = match region.trim().to_ascii_uppercase().as_str() {
            "EU868" => 868_000_000,
            "US915" => 915_000_000,
            "AU915" => 915_000_000,
            "AS923" => 923_000_000,
            "IN865" => 865_000_000,
            "KR920" => 920_000_000,
            "RU864" => 864_000_000,
            _ => return None,
        };
        Some(Self { frequency_hz, ..Self::us915_default() })
    }

    pub fn validate(self) -> Result<(), String> {
        if !(FREQ_MIN..=FREQ_MAX).contains(&self.frequency_hz) {
            return Err(format!("lora.frequency_hz must be between {FREQ_MIN} and {FREQ_MAX}"));
        }
        if !(7_800..=1_625_000).contains(&self.bandwidth_hz) {
            return Err("lora.bandwidth_hz must be between 7800 and 1625000".to_string());
        }
        if !(5..=12).contains(&self.spreading_factor) {
            return Err("lora.spreading_factor must be between 5 and 12".to_string());
        }
        if !(5..=8).contains(&self.coding_rate) {
            return Err("lora.coding_rate must be between 5 and 8".to_string());
        }
        if !(0..=37).contains(&self.tx_power_dbm) {
            return Err("lora.tx_power_dbm must be between 0 and 37".to_string());
        }
        if !(1..=255).contains(&self.max_payload_bytes) {
            return Err("lora.max_payload_bytes must be between 1 and 255".to_string());
        }
        if self.airtime_limit_short_hundredths.is_some_and(|value| value > 10_000) {
            return Err("lora.airtime_limit_short must be between 0 and 100".to_string());
        }
        if self.airtime_limit_long_hundredths.is_some_and(|value| value > 10_000) {
            return Err("lora.airtime_limit_long must be between 0 and 100".to_string());
        }
        Ok(())
    }

    #[must_use]
    pub fn probe_frames(&self) -> Vec<Vec<u8>> {
        vec![
            encode_command_frame(CMD_DETECT, &[DETECT_REQ]),
            encode_command_frame(CMD_FW_VERSION, &[0x00]),
            encode_command_frame(CMD_PLATFORM, &[0x00]),
            encode_command_frame(CMD_MCU, &[0x00]),
        ]
    }

    #[must_use]
    pub fn radio_config_frames(self) -> Vec<Vec<u8>> {
        let mut frames = vec![
            encode_command_frame(CMD_FREQUENCY, &u32_be_bytes(self.frequency_hz)),
            encode_command_frame(CMD_BANDWIDTH, &self.bandwidth_hz.to_be_bytes()),
            encode_command_frame(CMD_TXPOWER, &[self.tx_power_dbm as u8]),
            encode_command_frame(CMD_SF, &[self.spreading_factor]),
            encode_command_frame(CMD_CR, &[self.coding_rate]),
        ];
        if let Some(limit) = self.airtime_limit_short_hundredths {
            frames.push(encode_command_frame(CMD_ST_ALOCK, &limit.to_be_bytes()));
        }
        if let Some(limit) = self.airtime_limit_long_hundredths {
            frames.push(encode_command_frame(CMD_LT_ALOCK, &limit.to_be_bytes()));
        }
        frames.push(encode_command_frame(CMD_RADIO_STATE, &[RADIO_STATE_ON]));
        frames
    }

    #[must_use]
    pub fn command_frames(self) -> Vec<Vec<u8>> {
        self.probe_frames().into_iter().chain(self.radio_config_frames()).collect()
    }

    #[must_use]
    pub fn shutdown_frames(self) -> Vec<Vec<u8>> {
        vec![
            encode_command_frame(CMD_RADIO_STATE, &[RADIO_STATE_OFF]),
            encode_command_frame(CMD_LEAVE, &[0xff]),
        ]
    }

    #[must_use]
    pub fn radio_state_query_frame() -> Vec<u8> {
        encode_command_frame(CMD_RADIO_STATE, &[RADIO_STATE_ASK])
    }
}

fn u32_be_bytes(value: u64) -> [u8; 4] {
    u32::try_from(value).expect("validated LoRa frequency fits u32").to_be_bytes()
}

fn rnode_tcp_activity_probe() -> KissActivityProbeConfig {
    KissActivityProbeConfig {
        interval: R_NODE_TCP_ACTIVITY_KEEPALIVE,
        frames: vec![encode_command_frame(CMD_DETECT, &[DETECT_REQ])],
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LoraBearer {
    Serial,
    Tcp,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum LoraEndpoint {
    Serial { device: String, baud_rate: u32 },
    Tcp { addr: String },
}

impl LoraEndpoint {
    fn label(&self) -> &str {
        match self {
            Self::Serial { device, .. } => device,
            Self::Tcp { addr } => addr,
        }
    }

    const fn bearer(&self) -> LoraBearer {
        match self {
            Self::Serial { .. } => LoraBearer::Serial,
            Self::Tcp { .. } => LoraBearer::Tcp,
        }
    }

    const fn baud_rate(&self) -> Option<u32> {
        match self {
            Self::Serial { baud_rate, .. } => Some(*baud_rate),
            Self::Tcp { .. } => None,
        }
    }
}
