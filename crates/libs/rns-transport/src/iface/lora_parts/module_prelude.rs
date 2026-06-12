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
