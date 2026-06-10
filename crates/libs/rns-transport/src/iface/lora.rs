use std::net::{TcpStream as StdTcpStream, ToSocketAddrs};
use std::sync::Arc;
use std::time::Duration;

use tokio::io::{AsyncRead, AsyncWrite};
use tokio::net::TcpStream;
use tokio_serial::{DataBits, FlowControl, Parity, SerialPortBuilderExt, StopBits};

use crate::iface::kiss::{
    run_kiss_stream, KissActivityProbeConfig, KissCommandFrame, KissIdBeaconConfig,
    KissStreamOptions, KISS_FLOW_CONTROL_TIMEOUT, KISS_READ_FRAME_TIMEOUT,
};
use crate::kiss::encode_command_frame;

use super::{Interface, InterfaceContext};

pub const CMD_FREQUENCY: u8 = 0x01;
pub const CMD_BANDWIDTH: u8 = 0x02;
pub const CMD_TXPOWER: u8 = 0x03;
pub const CMD_SF: u8 = 0x04;
pub const CMD_CR: u8 = 0x05;
pub const CMD_RADIO_STATE: u8 = 0x06;
pub const CMD_RADIO_LOCK: u8 = 0x07;
pub const CMD_DETECT: u8 = 0x08;
pub const CMD_LEAVE: u8 = 0x0A;
pub const CMD_ST_ALOCK: u8 = 0x0B;
pub const CMD_LT_ALOCK: u8 = 0x0C;
pub const CMD_STAT_RX: u8 = 0x21;
pub const CMD_STAT_TX: u8 = 0x22;
pub const CMD_STAT_RSSI: u8 = 0x23;
pub const CMD_STAT_SNR: u8 = 0x24;
pub const CMD_STAT_CHTM: u8 = 0x25;
pub const CMD_STAT_PHYPRM: u8 = 0x26;
pub const CMD_STAT_BAT: u8 = 0x27;
pub const CMD_STAT_CSMA: u8 = 0x28;
pub const CMD_STAT_TEMP: u8 = 0x29;
pub const CMD_BLINK: u8 = 0x30;
pub const CMD_RANDOM: u8 = 0x40;
pub const CMD_FB_EXT: u8 = 0x41;
pub const CMD_FB_READ: u8 = 0x42;
pub const CMD_FB_WRITE: u8 = 0x43;
pub const CMD_BT_CTRL: u8 = 0x46;
pub const CMD_PLATFORM: u8 = 0x48;
pub const CMD_MCU: u8 = 0x49;
pub const CMD_FW_VERSION: u8 = 0x50;
pub const CMD_ROM_READ: u8 = 0x51;
pub const CMD_RESET: u8 = 0x55;
pub const CMD_DISP_READ: u8 = 0x66;
pub const CMD_ERROR: u8 = 0x90;

pub const DETECT_REQ: u8 = 0x73;
pub const DETECT_RESP: u8 = 0x46;
pub const RESET_ESP32: u8 = 0xF8;

pub const ERROR_INITRADIO: u8 = 0x01;
pub const ERROR_TXFAILED: u8 = 0x02;
pub const ERROR_EEPROM_LOCKED: u8 = 0x03;
pub const ERROR_QUEUE_FULL: u8 = 0x04;
pub const ERROR_MEMORY_LOW: u8 = 0x05;
pub const ERROR_MODEM_TIMEOUT: u8 = 0x06;

pub const RADIO_STATE_OFF: u8 = 0x00;
pub const RADIO_STATE_ON: u8 = 0x01;
pub const RADIO_STATE_ASK: u8 = 0xFF;
pub const BATTERY_STATE_UNKNOWN: u8 = 0x00;
pub const BATTERY_STATE_DISCHARGING: u8 = 0x01;
pub const BATTERY_STATE_CHARGING: u8 = 0x02;
pub const BATTERY_STATE_CHARGED: u8 = 0x03;
pub const REQUIRED_FW_VERSION_MAJOR: u8 = 1;
pub const REQUIRED_FW_VERSION_MINOR: u8 = 52;
pub const RSSI_OFFSET: i16 = 157;
pub const PLATFORM_AVR: u8 = 0x90;
pub const PLATFORM_ESP32: u8 = 0x80;
pub const PLATFORM_NRF52: u8 = 0x70;

const FREQ_MIN: u64 = 137_000_000;
const FREQ_MAX: u64 = 3_000_000_000;
const Q_SNR_MIN_BASE: f64 = -9.0;
const Q_SNR_MAX: f64 = 6.0;
const Q_SNR_STEP: f64 = 2.0;
const LORA_KISS_PROBE_CHANNEL_CAPACITY: usize = 64;
const R_NODE_STARTUP_RESPONSE_TIMEOUT: Duration = Duration::from_millis(1_500);
const R_NODE_TCP_ACTIVITY_KEEPALIVE: Duration = Duration::from_millis(3_500);
const R_NODE_FRAMEBUFFER_BYTES_PER_LINE: usize = 8;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct RNodeProbeStatus {
    pub detected: bool,
    pub firmware_version: Option<(u8, u8)>,
    pub platform: Option<u8>,
    pub mcu: Option<u8>,
}

impl RNodeProbeStatus {
    pub fn accept_command(&mut self, command: u8, payload: &[u8]) -> Result<bool, String> {
        match command {
            CMD_DETECT => {
                let [value] = payload else {
                    return Err("rnode detect response must contain one byte".to_string());
                };
                self.detected = *value == DETECT_RESP;
                Ok(true)
            }
            CMD_FW_VERSION => {
                let [major, minor] = payload else {
                    return Err("rnode firmware response must contain two bytes".to_string());
                };
                self.firmware_version = Some((*major, *minor));
                Ok(true)
            }
            CMD_PLATFORM => {
                let [platform] = payload else {
                    return Err("rnode platform response must contain one byte".to_string());
                };
                self.platform = Some(*platform);
                Ok(true)
            }
            CMD_MCU => {
                let [mcu] = payload else {
                    return Err("rnode mcu response must contain one byte".to_string());
                };
                self.mcu = Some(*mcu);
                Ok(true)
            }
            _ => Ok(false),
        }
    }

    pub fn validate_startup_probe(&self) -> Result<(), String> {
        if !self.detected {
            return Err("rnode detect response did not confirm an RNode device".to_string());
        }
        let Some((major, minor)) = self.firmware_version else {
            return Err("rnode firmware response is missing".to_string());
        };
        if major < REQUIRED_FW_VERSION_MAJOR
            || (major == REQUIRED_FW_VERSION_MAJOR && minor < REQUIRED_FW_VERSION_MINOR)
        {
            return Err(format!(
                "rnode firmware version {major}.{minor} is below required {REQUIRED_FW_VERSION_MAJOR}.{REQUIRED_FW_VERSION_MINOR}"
            ));
        }
        if self.platform.is_none() {
            return Err("rnode platform response is missing".to_string());
        }
        if self.mcu.is_none() {
            return Err("rnode mcu response is missing".to_string());
        }
        Ok(())
    }

    #[must_use]
    pub fn has_display(&self) -> bool {
        matches!(self.platform, Some(PLATFORM_ESP32 | PLATFORM_NRF52))
    }

    #[must_use]
    pub fn external_framebuffer_frame(&self, enable: bool) -> Option<Vec<u8>> {
        self.has_display().then(|| encode_command_frame(CMD_FB_EXT, &[u8::from(enable)]))
    }

    #[must_use]
    pub fn framebuffer_read_frame(&self) -> Option<Vec<u8>> {
        self.has_display().then(|| encode_command_frame(CMD_FB_READ, &[0x01]))
    }

    #[must_use]
    pub fn display_read_frame(&self) -> Option<Vec<u8>> {
        self.has_display().then(|| encode_command_frame(CMD_DISP_READ, &[0x01]))
    }

    #[must_use]
    pub fn framebuffer_write_frame(
        &self,
        line: u8,
        line_data: [u8; R_NODE_FRAMEBUFFER_BYTES_PER_LINE],
    ) -> Option<Vec<u8>> {
        self.has_display().then(|| {
            let mut payload = Vec::with_capacity(1 + R_NODE_FRAMEBUFFER_BYTES_PER_LINE);
            payload.push(line);
            payload.extend_from_slice(&line_data);
            encode_command_frame(CMD_FB_WRITE, &payload)
        })
    }

    #[must_use]
    pub fn display_image_frames(&self, image_data: &[u8]) -> Option<Vec<Vec<u8>>> {
        if !self.has_display() {
            return None;
        }
        Some(
            image_data
                .chunks_exact(R_NODE_FRAMEBUFFER_BYTES_PER_LINE)
                .take(usize::from(u8::MAX) + 1)
                .enumerate()
                .filter_map(|(line, chunk)| {
                    let line = u8::try_from(line).ok()?;
                    let line_data: [u8; R_NODE_FRAMEBUFFER_BYTES_PER_LINE] =
                        chunk.try_into().expect("chunks_exact yields framebuffer line length");
                    self.framebuffer_write_frame(line, line_data)
                })
                .collect(),
        )
    }

    #[must_use]
    pub fn hard_reset_frame() -> Vec<u8> {
        encode_command_frame(CMD_RESET, &[RESET_ESP32])
    }

    pub fn accept_reset_response(&self, payload: &[u8], online: bool) -> Result<bool, String> {
        let reset_value = single_byte_payload(payload, "reset")?;
        if reset_value == RESET_ESP32 && self.platform == Some(PLATFORM_ESP32) && online {
            return Err("ESP32 reset".to_string());
        }
        Ok(true)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RNodeHardwareError {
    pub code: u8,
    pub description: &'static str,
    pub fatal: bool,
}

impl RNodeHardwareError {
    #[must_use]
    pub const fn from_code(code: u8) -> Self {
        match code {
            ERROR_INITRADIO => {
                Self { code, description: "Radio initialisation failure", fatal: true }
            }
            ERROR_TXFAILED => Self { code, description: "Hardware transmit failure", fatal: true },
            ERROR_MEMORY_LOW => {
                Self { code, description: "Memory exhausted on connected device", fatal: false }
            }
            ERROR_MODEM_TIMEOUT => Self {
                code,
                description: "Modem communication timed out on connected device",
                fatal: false,
            },
            _ => Self { code, description: "Unknown hardware failure", fatal: true },
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct RNodeRadioStatus {
    pub frequency_hz: Option<u32>,
    pub bandwidth_hz: Option<u32>,
    pub tx_power_dbm: Option<u8>,
    pub spreading_factor: Option<u8>,
    pub coding_rate: Option<u8>,
    pub radio_state: Option<u8>,
    pub radio_lock: Option<u8>,
    pub stat_rx: Option<u32>,
    pub stat_tx: Option<u32>,
    pub rssi_dbm: Option<i16>,
    pub snr_db: Option<f64>,
    pub signal_quality_percent: Option<f64>,
    pub short_airtime_limit_percent: Option<f64>,
    pub long_airtime_limit_percent: Option<f64>,
    pub airtime_short_percent: Option<f64>,
    pub airtime_long_percent: Option<f64>,
    pub channel_load_short_percent: Option<f64>,
    pub channel_load_long_percent: Option<f64>,
    pub current_rssi_dbm: Option<i16>,
    pub noise_floor_dbm: Option<i16>,
    pub interference_dbm: Option<i16>,
    pub symbol_time_ms: Option<f64>,
    pub symbol_rate_baud: Option<u16>,
    pub preamble_symbols: Option<u16>,
    pub preamble_time_ms: Option<u16>,
    pub csma_slot_time_ms: Option<u16>,
    pub csma_difs_ms: Option<u16>,
    pub csma_cw_band: Option<u8>,
    pub csma_cw_min: Option<u8>,
    pub csma_cw_max: Option<u8>,
    pub battery_state: Option<u8>,
    pub battery_percent: Option<u8>,
    pub temperature_c: Option<i16>,
    pub framebuffer: Option<Vec<u8>>,
    pub display: Option<Vec<u8>>,
    pub random_byte: Option<u8>,
}

impl Default for RNodeRadioStatus {
    fn default() -> Self {
        Self {
            frequency_hz: None,
            bandwidth_hz: None,
            tx_power_dbm: None,
            spreading_factor: None,
            coding_rate: None,
            radio_state: None,
            radio_lock: None,
            stat_rx: None,
            stat_tx: None,
            rssi_dbm: None,
            snr_db: None,
            signal_quality_percent: None,
            short_airtime_limit_percent: None,
            long_airtime_limit_percent: None,
            airtime_short_percent: Some(0.0),
            airtime_long_percent: Some(0.0),
            channel_load_short_percent: Some(0.0),
            channel_load_long_percent: Some(0.0),
            current_rssi_dbm: None,
            noise_floor_dbm: None,
            interference_dbm: None,
            symbol_time_ms: None,
            symbol_rate_baud: None,
            preamble_symbols: None,
            preamble_time_ms: None,
            csma_slot_time_ms: None,
            csma_difs_ms: None,
            csma_cw_band: None,
            csma_cw_min: None,
            csma_cw_max: None,
            battery_state: Some(BATTERY_STATE_UNKNOWN),
            battery_percent: Some(0),
            temperature_c: None,
            framebuffer: Some(Vec::new()),
            display: Some(Vec::new()),
            random_byte: None,
        }
    }
}

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

#[derive(Debug, Clone)]
pub struct LoraInterface {
    endpoint: LoraEndpoint,
    config: LoraConfig,
    probe_status: RNodeProbeStatus,
    radio_status: RNodeRadioStatus,
    hardware_errors: Vec<RNodeHardwareError>,
    last_command_error: Option<String>,
    online: bool,
    flow_control: bool,
    id_beacon: Option<KissIdBeaconConfig>,
    reconnect_backoff: Duration,
    max_reconnect_backoff: Duration,
    startup_response_timeout: Duration,
}

impl LoraInterface {
    #[must_use]
    pub fn new<T: Into<String>>(device: T, baud_rate: u32, config: LoraConfig) -> Self {
        Self {
            endpoint: LoraEndpoint::Serial { device: device.into(), baud_rate },
            config,
            probe_status: RNodeProbeStatus::default(),
            radio_status: RNodeRadioStatus::default(),
            hardware_errors: Vec::new(),
            last_command_error: None,
            online: false,
            flow_control: false,
            id_beacon: None,
            reconnect_backoff: Duration::from_millis(500),
            max_reconnect_backoff: Duration::from_millis(5_000),
            startup_response_timeout: R_NODE_STARTUP_RESPONSE_TIMEOUT,
        }
    }

    #[must_use]
    pub fn new_tcp<T: Into<String>>(addr: T, config: LoraConfig) -> Self {
        Self {
            endpoint: LoraEndpoint::Tcp { addr: addr.into() },
            config,
            probe_status: RNodeProbeStatus::default(),
            radio_status: RNodeRadioStatus::default(),
            hardware_errors: Vec::new(),
            last_command_error: None,
            online: false,
            flow_control: false,
            id_beacon: None,
            reconnect_backoff: Duration::from_millis(500),
            max_reconnect_backoff: Duration::from_millis(5_000),
            startup_response_timeout: R_NODE_STARTUP_RESPONSE_TIMEOUT,
        }
    }

    #[must_use]
    pub fn bearer(&self) -> LoraBearer {
        self.endpoint.bearer()
    }

    #[must_use]
    pub fn endpoint(&self) -> &str {
        self.endpoint.label()
    }

    #[must_use]
    pub fn baud_rate(&self) -> Option<u32> {
        self.endpoint.baud_rate()
    }

    #[must_use]
    pub fn activity_probe(&self) -> Option<KissActivityProbeConfig> {
        (self.endpoint.bearer() == LoraBearer::Tcp).then(rnode_tcp_activity_probe)
    }

    #[must_use]
    pub fn config(&self) -> LoraConfig {
        self.config
    }

    #[must_use]
    pub fn probe_status(&self) -> RNodeProbeStatus {
        self.probe_status
    }

    #[must_use]
    pub fn radio_status(&self) -> RNodeRadioStatus {
        self.radio_status.clone()
    }

    #[must_use]
    pub fn hardware_errors(&self) -> &[RNodeHardwareError] {
        &self.hardware_errors
    }

    #[must_use]
    pub fn last_command_error(&self) -> Option<&str> {
        self.last_command_error.as_deref()
    }

    #[must_use]
    pub fn online(&self) -> bool {
        self.online
    }

    #[must_use]
    pub fn flow_control(&self) -> bool {
        self.flow_control
    }

    #[must_use]
    pub fn startup_response_timeout(&self) -> Duration {
        self.startup_response_timeout
    }

    #[must_use]
    pub fn with_flow_control(mut self, flow_control: bool) -> Self {
        self.flow_control = flow_control;
        self
    }

    #[must_use]
    pub fn with_id_beacon(mut self, id_beacon: Option<KissIdBeaconConfig>) -> Self {
        self.id_beacon = id_beacon;
        self
    }

    pub fn record_probe_command(&mut self, command: u8, payload: &[u8]) -> Result<bool, String> {
        self.probe_status.accept_command(command, payload)
    }

    pub fn begin_startup_response_collection(&mut self) {
        self.probe_status = RNodeProbeStatus::default();
        self.radio_status = RNodeRadioStatus::default();
        self.hardware_errors.clear();
        self.last_command_error = None;
        self.online = false;
    }

    pub fn record_command_response(&mut self, command: u8, payload: &[u8]) -> Result<bool, String> {
        if self.probe_status.accept_command(command, payload)? {
            return Ok(true);
        }
        if command == CMD_RESET {
            return match self.probe_status.accept_reset_response(payload, self.online) {
                Ok(accepted) => Ok(accepted),
                Err(err) => {
                    self.last_command_error = Some(err.clone());
                    Err(err)
                }
            };
        }
        if command == CMD_ERROR {
            let code = single_byte_payload(payload, "hardware error")?;
            let error = RNodeHardwareError::from_code(code);
            if error.fatal {
                self.last_command_error = Some(error.description.to_string());
                return Err(error.description.to_string());
            }
            self.hardware_errors.push(error);
            return Ok(true);
        }
        let accepted = self.radio_status.accept_command(command, payload)?;
        if accepted && command == CMD_RADIO_STATE {
            self.online = self.radio_status.radio_state == Some(RADIO_STATE_ON);
        }
        Ok(accepted)
    }

    pub fn record_inbound_data_frame(&mut self) {
        self.radio_status.rssi_dbm = None;
        self.radio_status.snr_db = None;
    }

    pub fn is_detected(&self) -> bool {
        self.probe_status.detected
    }

    pub fn validate_probe_status(&self) -> Result<(), String> {
        self.probe_status.validate_startup_probe()
    }

    pub fn validate_radio_status(&self) -> Result<(), String> {
        self.radio_status.validate_config(self.config, RADIO_STATE_ON)
    }

    pub fn validate_startup_responses(&self) -> Result<(), String> {
        if let Some(err) = self.last_command_error() {
            return Err(err.to_string());
        }
        self.validate_probe_status()?;
        self.validate_radio_status()
    }

    pub fn reported_bitrate_bps(&self) -> Option<f64> {
        self.radio_status.reported_bitrate_bps()
    }

    #[must_use]
    pub fn with_reconnect_backoff(mut self, reconnect_backoff: Duration) -> Self {
        self.reconnect_backoff = reconnect_backoff;
        if self.max_reconnect_backoff < self.reconnect_backoff {
            self.max_reconnect_backoff = self.reconnect_backoff;
        }
        self
    }

    #[must_use]
    pub fn with_max_reconnect_backoff(mut self, max_reconnect_backoff: Duration) -> Self {
        self.max_reconnect_backoff = max_reconnect_backoff.max(self.reconnect_backoff);
        self
    }

    #[must_use]
    pub fn with_startup_response_timeout(mut self, startup_response_timeout: Duration) -> Self {
        self.startup_response_timeout = startup_response_timeout;
        self
    }

    pub fn preflight_open(&self) -> Result<(), String> {
        self.config.validate()?;
        match &self.endpoint {
            LoraEndpoint::Serial { device, baud_rate } => {
                tokio_serial::new(device.clone(), *baud_rate)
                    .data_bits(DataBits::Eight)
                    .parity(Parity::None)
                    .stop_bits(StopBits::One)
                    .flow_control(FlowControl::None)
                    .open_native_async()
                    .map(|_| ())
                    .map_err(|err| {
                        format!(
                            "lora preflight open failed device={} baud_rate={} err={}",
                            device, baud_rate, err
                        )
                    })
            }
            LoraEndpoint::Tcp { addr } => preflight_tcp_connect(addr),
        }
    }

    pub async fn spawn(context: InterfaceContext<LoraInterface>) {
        let iface_stop = context.channel.stop.clone();
        let iface_address = context.channel.address;
        let (
            endpoint,
            config,
            flow_control,
            id_beacon,
            reconnect_backoff,
            max_reconnect_backoff,
            startup_response_timeout,
        ) = {
            let guard = context.inner.lock().expect("lora interface mutex poisoned");
            (
                guard.endpoint.clone(),
                guard.config,
                guard.flow_control,
                guard.id_beacon.clone(),
                guard.reconnect_backoff,
                guard.max_reconnect_backoff,
                guard.startup_response_timeout,
            )
        };

        let (rx_channel, tx_channel) = context.channel.split();
        let tx_channel = Arc::new(tokio::sync::Mutex::new(tx_channel));
        let mut active_backoff = reconnect_backoff;

        loop {
            if context.cancel.is_cancelled() {
                break;
            }

            match &endpoint {
                LoraEndpoint::Serial { device, baud_rate } => {
                    let port = match tokio_serial::new(device.clone(), *baud_rate)
                        .data_bits(DataBits::Eight)
                        .parity(Parity::None)
                        .stop_bits(StopBits::One)
                        .flow_control(FlowControl::None)
                        .open_native_async()
                    {
                        Ok(port) => port,
                        Err(err) => {
                            log::warn!(
                                "failed to open LoRa serial device={} baud_rate={} err={}",
                                device,
                                baud_rate,
                                err
                            );
                            tokio::time::sleep(active_backoff).await;
                            active_backoff =
                                bounded_backoff_next(active_backoff, max_reconnect_backoff);
                            continue;
                        }
                    };

                    log::info!(
                        "opened LoRa serial device={} baud_rate={} iface={} frequency_hz={} bandwidth_hz={} sf={} cr={}",
                        device,
                        baud_rate,
                        iface_address,
                        config.frequency_hz,
                        config.bandwidth_hz,
                        config.spreading_factor,
                        config.coding_rate
                    );
                    active_backoff = reconnect_backoff;
                    run_lora_kiss_stream(
                        port,
                        LoraStreamRun {
                            interface: context.inner.clone(),
                            cancel: context.cancel.clone(),
                            iface_address,
                            endpoint_label: device.clone(),
                            config,
                            flow_control,
                            id_beacon: id_beacon.clone(),
                            activity_probe: None,
                            startup_response_timeout,
                            rx_channel: rx_channel.clone(),
                            tx_channel: tx_channel.clone(),
                        },
                    )
                    .await;
                }
                LoraEndpoint::Tcp { addr } => {
                    let stream = match TcpStream::connect(addr.clone()).await {
                        Ok(stream) => stream,
                        Err(err) => {
                            log::warn!("failed to connect LoRa tcp addr={} err={}", addr, err);
                            tokio::time::sleep(active_backoff).await;
                            active_backoff =
                                bounded_backoff_next(active_backoff, max_reconnect_backoff);
                            continue;
                        }
                    };

                    log::info!(
                        "opened LoRa tcp addr={} iface={} frequency_hz={} bandwidth_hz={} sf={} cr={}",
                        addr,
                        iface_address,
                        config.frequency_hz,
                        config.bandwidth_hz,
                        config.spreading_factor,
                        config.coding_rate
                    );
                    active_backoff = reconnect_backoff;
                    run_lora_kiss_stream(
                        stream,
                        LoraStreamRun {
                            interface: context.inner.clone(),
                            cancel: context.cancel.clone(),
                            iface_address,
                            endpoint_label: addr.clone(),
                            config,
                            flow_control,
                            id_beacon: id_beacon.clone(),
                            activity_probe: Some(rnode_tcp_activity_probe()),
                            startup_response_timeout,
                            rx_channel: rx_channel.clone(),
                            tx_channel: tx_channel.clone(),
                        },
                    )
                    .await;
                }
            };

            if context.cancel.is_cancelled() {
                break;
            }
            tokio::time::sleep(active_backoff).await;
            active_backoff = bounded_backoff_next(active_backoff, max_reconnect_backoff);
        }

        iface_stop.cancel();
    }
}

struct LoraStreamRun {
    interface: Arc<std::sync::Mutex<LoraInterface>>,
    cancel: tokio_util::sync::CancellationToken,
    iface_address: crate::hash::AddressHash,
    endpoint_label: String,
    config: LoraConfig,
    flow_control: bool,
    id_beacon: Option<KissIdBeaconConfig>,
    activity_probe: Option<KissActivityProbeConfig>,
    startup_response_timeout: Duration,
    rx_channel: tokio::sync::mpsc::Sender<crate::iface::RxMessage>,
    tx_channel: Arc<tokio::sync::Mutex<tokio::sync::mpsc::Receiver<crate::iface::TxMessage>>>,
}

async fn run_lora_kiss_stream<IO>(stream: IO, run: LoraStreamRun)
where
    IO: AsyncRead + AsyncWrite + Unpin + Send + 'static,
{
    let stream_cancel = run.cancel.child_token();
    let (command_tx, command_rx) = tokio::sync::mpsc::channel(LORA_KISS_PROBE_CHANNEL_CAPACITY);
    let (data_rx_tx, data_rx) = tokio::sync::mpsc::channel(LORA_KISS_PROBE_CHANNEL_CAPACITY);
    let probe_status_task = tokio::spawn(record_probe_status_commands_with_startup_timeout(
        run.interface,
        command_rx,
        data_rx,
        stream_cancel.clone(),
        Some(run.startup_response_timeout),
    ));

    run_kiss_stream(
        stream,
        KissStreamOptions {
            iface_address: run.iface_address,
            device: run.endpoint_label,
            mtu: usize::from(run.config.max_payload_bytes),
            flow_control: run.flow_control,
            flow_control_timeout: KISS_FLOW_CONTROL_TIMEOUT,
            read_frame_timeout: KISS_READ_FRAME_TIMEOUT,
            initial_frames: run.config.command_frames(),
            shutdown_frames: run.config.shutdown_frames(),
            id_beacon: run.id_beacon,
            activity_probe: run.activity_probe,
            strip_command_port_nibble: false,
            command_tx: Some(command_tx),
            data_rx_tx: Some(data_rx_tx),
        },
        stream_cancel,
        run.rx_channel,
        run.tx_channel,
    )
    .await;
    if let Err(err) = probe_status_task.await {
        if !err.is_cancelled() {
            log::warn!("LoRa probe status task failed iface={} err={}", run.iface_address, err);
        }
    }
}

impl Interface for LoraInterface {
    fn mtu() -> usize {
        220
    }

    fn configured_mtu(&self) -> usize {
        usize::from(self.config.max_payload_bytes)
    }
}

async fn record_probe_status_commands_with_startup_timeout(
    interface: Arc<std::sync::Mutex<LoraInterface>>,
    mut command_rx: tokio::sync::mpsc::Receiver<KissCommandFrame>,
    mut data_rx: tokio::sync::mpsc::Receiver<()>,
    cancel: tokio_util::sync::CancellationToken,
    startup_response_timeout: Option<Duration>,
) {
    if startup_response_timeout.is_some() {
        let mut guard = interface.lock().expect("lora interface mutex poisoned");
        guard.begin_startup_response_collection();
    }
    let mut startup_deadline =
        startup_response_timeout.map(|timeout| Box::pin(tokio::time::sleep(timeout)));
    loop {
        tokio::select! {
            _ = cancel.cancelled() => break,
            _ = async {
                if let Some(deadline) = startup_deadline.as_mut() {
                    deadline.as_mut().await;
                }
            }, if startup_deadline.is_some() => {
                startup_deadline = None;
                let result = {
                    let mut guard = interface.lock().expect("lora interface mutex poisoned");
                    match guard.validate_startup_responses() {
                        Ok(()) => Ok(()),
                        Err(err) => {
                            guard.last_command_error = Some(err.clone());
                            Err(err)
                        }
                    }
                };
                match result {
                    Ok(()) => log::debug!("validated LoRa RNode startup responses"),
                    Err(err) => {
                        log::warn!("LoRa RNode startup response validation failed err={}", err);
                        cancel.cancel();
                        break;
                    }
                }
            }
            command = command_rx.recv() => {
                let Some(command) = command else {
                    break;
                };
                let result = {
                    let mut guard = interface.lock().expect("lora interface mutex poisoned");
                    let result = guard.record_command_response(command.command, &command.payload);
                    let fatal = match &result {
                        Ok(_) => false,
                        Err(err) => guard.last_command_error() == Some(err.as_str()),
                    };
                    (result, fatal)
                };
                match result {
                    (Ok(true), _) => log::debug!(
                        "recorded LoRa RNode command response command=0x{:02x}",
                        command.command
                    ),
                    (Ok(false), _) => {}
                    (Err(err), true) => {
                        log::warn!(
                            "fatal LoRa RNode command response command=0x{:02x} err={}",
                            command.command,
                            err
                        );
                        cancel.cancel();
                        break;
                    }
                    (Err(err), false) => log::warn!(
                        "ignored malformed LoRa RNode probe response command=0x{:02x} err={}",
                        command.command,
                        err
                    ),
                }
            }
            data = data_rx.recv() => {
                if data.is_none() {
                    break;
                }
                let mut guard = interface.lock().expect("lora interface mutex poisoned");
                guard.record_inbound_data_frame();
            }
        }
    }
}

fn bounded_backoff_next(current: Duration, max: Duration) -> Duration {
    let current_ms = current.as_millis() as u64;
    let max_ms = max.as_millis() as u64;
    Duration::from_millis(current_ms.saturating_mul(2).min(max_ms))
}

fn preflight_tcp_connect(addr: &str) -> Result<(), String> {
    let socket_addr = addr
        .to_socket_addrs()
        .map_err(|err| format!("lora tcp preflight resolve failed addr={addr} err={err}"))?
        .next()
        .ok_or_else(|| format!("lora tcp preflight resolve failed addr={addr}"))?;
    StdTcpStream::connect_timeout(&socket_addr, Duration::from_secs(3))
        .map(|_| ())
        .map_err(|err| format!("lora tcp preflight connect failed addr={addr} err={err}"))
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};
    use std::time::Duration;

    use tokio_util::sync::CancellationToken;

    use super::*;

    #[tokio::test]
    async fn command_response_task_cancels_stream_on_fatal_rnode_error() {
        let iface =
            Arc::new(Mutex::new(LoraInterface::new("COM9", 115_200, LoraConfig::us915_default())));
        let cancel = CancellationToken::new();
        let (command_tx, command_rx) = tokio::sync::mpsc::channel(LORA_KISS_PROBE_CHANNEL_CAPACITY);
        let (_data_rx_tx, data_rx) = tokio::sync::mpsc::channel(LORA_KISS_PROBE_CHANNEL_CAPACITY);

        let task = tokio::spawn(record_probe_status_commands_with_startup_timeout(
            iface.clone(),
            command_rx,
            data_rx,
            cancel.clone(),
            None,
        ));

        command_tx
            .send(KissCommandFrame { command: CMD_ERROR, payload: vec![ERROR_TXFAILED] })
            .await
            .expect("send fatal command");

        tokio::time::timeout(Duration::from_secs(1), cancel.cancelled())
            .await
            .expect("fatal RNode command should cancel the stream");

        drop(command_tx);
        task.await.expect("command task");

        let guard = iface.lock().expect("lora interface mutex poisoned");
        assert_eq!(guard.last_command_error(), Some("Hardware transmit failure"));
    }

    #[tokio::test]
    async fn command_response_task_keeps_stream_on_malformed_rnode_response() {
        let iface =
            Arc::new(Mutex::new(LoraInterface::new("COM9", 115_200, LoraConfig::us915_default())));
        let cancel = CancellationToken::new();
        let (command_tx, command_rx) = tokio::sync::mpsc::channel(LORA_KISS_PROBE_CHANNEL_CAPACITY);
        let (_data_rx_tx, data_rx) = tokio::sync::mpsc::channel(LORA_KISS_PROBE_CHANNEL_CAPACITY);

        let task = tokio::spawn(record_probe_status_commands_with_startup_timeout(
            iface.clone(),
            command_rx,
            data_rx,
            cancel.clone(),
            None,
        ));

        command_tx
            .send(KissCommandFrame { command: CMD_FW_VERSION, payload: vec![1] })
            .await
            .expect("send malformed command");

        assert!(
            tokio::time::timeout(Duration::from_millis(50), cancel.cancelled()).await.is_err(),
            "malformed RNode response should not cancel the stream"
        );

        drop(command_tx);
        task.await.expect("command task");

        let guard = iface.lock().expect("lora interface mutex poisoned");
        assert_eq!(guard.last_command_error(), None);
    }

    #[tokio::test]
    async fn command_response_task_clears_signal_stats_on_inbound_data_frame() {
        let iface =
            Arc::new(Mutex::new(LoraInterface::new("COM9", 115_200, LoraConfig::us915_default())));
        {
            let mut guard = iface.lock().expect("lora interface mutex poisoned");
            guard.record_command_response(CMD_SF, &[9]).expect("spreading factor");
            guard.record_command_response(CMD_STAT_RSSI, &[97]).expect("rssi");
            guard.record_command_response(CMD_STAT_SNR, &[0xF8]).expect("negative snr");
        }

        let cancel = CancellationToken::new();
        let (command_tx, command_rx) = tokio::sync::mpsc::channel(LORA_KISS_PROBE_CHANNEL_CAPACITY);
        let (data_rx_tx, data_rx) = tokio::sync::mpsc::channel(LORA_KISS_PROBE_CHANNEL_CAPACITY);

        let task = tokio::spawn(record_probe_status_commands_with_startup_timeout(
            iface.clone(),
            command_rx,
            data_rx,
            cancel.clone(),
            None,
        ));

        data_rx_tx.send(()).await.expect("send data frame event");

        tokio::time::sleep(Duration::from_millis(20)).await;
        drop(command_tx);
        drop(data_rx_tx);
        task.await.expect("command task");

        let status = iface.lock().expect("lora interface mutex poisoned").radio_status();
        assert_eq!(status.rssi_dbm, None);
        assert_eq!(status.snr_db, None);
        assert_eq!(status.signal_quality_percent, Some(57.9));
    }

    #[tokio::test]
    async fn command_response_task_cancels_stream_on_missing_startup_responses_after_deadline() {
        let iface =
            Arc::new(Mutex::new(LoraInterface::new("COM9", 115_200, LoraConfig::us915_default())));
        let cancel = CancellationToken::new();
        let (command_tx, command_rx) = tokio::sync::mpsc::channel(LORA_KISS_PROBE_CHANNEL_CAPACITY);
        let (_data_rx_tx, data_rx) = tokio::sync::mpsc::channel(LORA_KISS_PROBE_CHANNEL_CAPACITY);

        let task = tokio::spawn(record_probe_status_commands_with_startup_timeout(
            iface.clone(),
            command_rx,
            data_rx,
            cancel.clone(),
            Some(Duration::from_millis(10)),
        ));

        tokio::time::timeout(Duration::from_secs(1), cancel.cancelled())
            .await
            .expect("missing startup responses should cancel the stream");

        drop(command_tx);
        task.await.expect("command task");

        let guard = iface.lock().expect("lora interface mutex poisoned");
        assert!(
            guard.last_command_error().is_some_and(|err| err.contains("detect")),
            "unexpected startup error: {:?}",
            guard.last_command_error()
        );
    }

    #[tokio::test]
    async fn command_response_task_keeps_stream_when_startup_responses_validate_before_deadline() {
        let iface =
            Arc::new(Mutex::new(LoraInterface::new("COM9", 115_200, LoraConfig::us915_default())));
        let cancel = CancellationToken::new();
        let (command_tx, command_rx) = tokio::sync::mpsc::channel(LORA_KISS_PROBE_CHANNEL_CAPACITY);
        let (_data_rx_tx, data_rx) = tokio::sync::mpsc::channel(LORA_KISS_PROBE_CHANNEL_CAPACITY);

        let task = tokio::spawn(record_probe_status_commands_with_startup_timeout(
            iface.clone(),
            command_rx,
            data_rx,
            cancel.clone(),
            Some(Duration::from_millis(200)),
        ));

        for frame in [
            KissCommandFrame { command: CMD_DETECT, payload: vec![DETECT_RESP] },
            KissCommandFrame { command: CMD_FW_VERSION, payload: vec![1, 52] },
            KissCommandFrame { command: CMD_PLATFORM, payload: vec![PLATFORM_ESP32] },
            KissCommandFrame { command: CMD_MCU, payload: vec![0x01] },
            KissCommandFrame {
                command: CMD_FREQUENCY,
                payload: 915_000_000_u32.to_be_bytes().to_vec(),
            },
            KissCommandFrame {
                command: CMD_BANDWIDTH,
                payload: 125_000_u32.to_be_bytes().to_vec(),
            },
            KissCommandFrame { command: CMD_TXPOWER, payload: vec![17] },
            KissCommandFrame { command: CMD_SF, payload: vec![9] },
            KissCommandFrame { command: CMD_CR, payload: vec![5] },
            KissCommandFrame { command: CMD_RADIO_STATE, payload: vec![RADIO_STATE_ON] },
        ] {
            command_tx.send(frame).await.expect("send startup command");
        }

        assert!(
            tokio::time::timeout(Duration::from_millis(50), cancel.cancelled()).await.is_err(),
            "valid startup responses should not cancel the stream before the deadline"
        );

        drop(command_tx);
        task.await.expect("command task");

        let guard = iface.lock().expect("lora interface mutex poisoned");
        assert_eq!(guard.last_command_error(), None);
        guard.validate_startup_responses().expect("recorded startup responses");
    }

    #[tokio::test]
    async fn command_response_task_clears_stale_startup_state_for_new_stream() {
        let iface =
            Arc::new(Mutex::new(LoraInterface::new("COM9", 115_200, LoraConfig::us915_default())));
        {
            let mut guard = iface.lock().expect("lora interface mutex poisoned");
            guard.record_command_response(CMD_ERROR, &[ERROR_TXFAILED]).expect_err("fatal error");
            assert_eq!(guard.last_command_error(), Some("Hardware transmit failure"));
        }

        let cancel = CancellationToken::new();
        let (command_tx, command_rx) = tokio::sync::mpsc::channel(LORA_KISS_PROBE_CHANNEL_CAPACITY);
        let (_data_rx_tx, data_rx) = tokio::sync::mpsc::channel(LORA_KISS_PROBE_CHANNEL_CAPACITY);

        let task = tokio::spawn(record_probe_status_commands_with_startup_timeout(
            iface.clone(),
            command_rx,
            data_rx,
            cancel.clone(),
            Some(Duration::from_millis(200)),
        ));

        for frame in [
            KissCommandFrame { command: CMD_DETECT, payload: vec![DETECT_RESP] },
            KissCommandFrame { command: CMD_FW_VERSION, payload: vec![1, 52] },
            KissCommandFrame { command: CMD_PLATFORM, payload: vec![PLATFORM_ESP32] },
            KissCommandFrame { command: CMD_MCU, payload: vec![0x01] },
            KissCommandFrame {
                command: CMD_FREQUENCY,
                payload: 915_000_000_u32.to_be_bytes().to_vec(),
            },
            KissCommandFrame {
                command: CMD_BANDWIDTH,
                payload: 125_000_u32.to_be_bytes().to_vec(),
            },
            KissCommandFrame { command: CMD_TXPOWER, payload: vec![17] },
            KissCommandFrame { command: CMD_SF, payload: vec![9] },
            KissCommandFrame { command: CMD_CR, payload: vec![5] },
            KissCommandFrame { command: CMD_RADIO_STATE, payload: vec![RADIO_STATE_ON] },
        ] {
            command_tx.send(frame).await.expect("send startup command");
        }

        assert!(
            tokio::time::timeout(Duration::from_millis(50), cancel.cancelled()).await.is_err(),
            "fresh valid startup responses should not inherit stale fatal errors"
        );

        drop(command_tx);
        task.await.expect("command task");

        let guard = iface.lock().expect("lora interface mutex poisoned");
        assert_eq!(guard.last_command_error(), None);
        guard.validate_startup_responses().expect("fresh startup responses");
    }
}
