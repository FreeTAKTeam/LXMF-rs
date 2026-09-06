// The reply to `LoraConfig::rom_read_frame()`, read back. `include!`d into
// the lora module beside the frame builders; its own file so each stays
// within the repository's module-size policy.

/// Byte offsets into an RNode's EEPROM image — `rnodeconf`'s `ROM.ADDR_CONF_*`.
mod rom {
    pub const ADDR_CONF_SF: usize = 0x9C;
    pub const ADDR_CONF_CR: usize = 0x9D;
    pub const ADDR_CONF_TXP: usize = 0x9E;
    /// Four bytes, big-endian.
    pub const ADDR_CONF_BW: usize = 0x9F;
    /// Four bytes, big-endian.
    pub const ADDR_CONF_FREQ: usize = 0xA3;
    /// Holds [`CONF_OK_BYTE`] when, and only when, a configuration is stored.
    pub const ADDR_CONF_OK: usize = 0xA7;
    /// `rnodeconf`'s `ROM.CONF_OK_BYTE`.
    pub const CONF_OK_BYTE: u8 = 0x73;
}

/// The shortest EEPROM image that can say whether a configuration is stored.
/// Anything shorter is a truncated read, not a device without one.
const STORED_CONFIG_IMAGE_LEN: usize = rom::ADDR_CONF_OK + 1;

/// Why an RNode's stored configuration could not be read. Both variants are
/// failures to answer, and neither is "the device holds no configuration",
/// which is `Ok(None)`. A caller that cannot tell them apart is a caller that
/// may reprogram a radio because a serial read was cut short.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum StoredConfigError {
    /// No `CMD_ROM_READ` reply has arrived on this interface.
    #[error("no ROM read response has been received")]
    NotRead,
    /// The reply stopped before [`rom::ADDR_CONF_OK`], the byte that says
    /// whether a configuration is stored at all.
    #[error("ROM read response is {len} bytes, short of the {STORED_CONFIG_IMAGE_LEN} needed")]
    TruncatedImage { len: usize },
}

/// The radio settings an RNode holds in its EEPROM: what `rnodeconf --tnc`
/// saved with `CMD_CONF_SAVE`, and what the device starts up on.
///
/// Deliberately not a [`LoraConfig`]: this is what the device reported, not
/// what a caller intends to apply, and the two are different facts until a
/// person has confirmed the first. The payload limits and airtime limits a
/// `LoraConfig` carries are not stored on the device at all.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StoredRadioConfig {
    pub frequency_hz: u32,
    pub bandwidth_hz: u32,
    pub spreading_factor: u8,
    pub coding_rate: u8,
    pub tx_power_dbm: u8,
}

impl LoraConfig {
    /// Reads the stored configuration out of a `CMD_ROM_READ` reply — the
    /// EEPROM image — the way `rnodeconf` reads it: only when
    /// `ADDR_CONF_OK` carries `CONF_OK_BYTE`, with the two multi-byte
    /// fields big-endian.
    ///
    /// `Ok(None)` means a complete image whose sentinel says the device holds
    /// no configuration. A short image is [`StoredConfigError::TruncatedImage`]
    /// instead, because the two are different facts and only one of them
    /// licenses a caller to write settings: a default here is how a radio ends
    /// up transmitting on a frequency nobody chose.
    pub fn parse_stored_config(
        eeprom: &[u8],
    ) -> Result<Option<StoredRadioConfig>, StoredConfigError> {
        if eeprom.len() < STORED_CONFIG_IMAGE_LEN {
            return Err(StoredConfigError::TruncatedImage { len: eeprom.len() });
        }
        if eeprom[rom::ADDR_CONF_OK] != rom::CONF_OK_BYTE {
            return Ok(None);
        }
        Ok(Some(StoredRadioConfig {
            frequency_hz: be_u32(eeprom, rom::ADDR_CONF_FREQ),
            bandwidth_hz: be_u32(eeprom, rom::ADDR_CONF_BW),
            spreading_factor: eeprom[rom::ADDR_CONF_SF],
            coding_rate: eeprom[rom::ADDR_CONF_CR],
            tx_power_dbm: eeprom[rom::ADDR_CONF_TXP],
        }))
    }
}

/// The parse of whatever `CMD_ROM_READ` reply has arrived, or why there is
/// nothing to parse. Shared with the stream task that receives it.
fn stored_config_from_image(
    image: &RNodeStoredConfigImage,
) -> Result<Option<StoredRadioConfig>, StoredConfigError> {
    let guard = image.lock().expect("lora stored config mutex poisoned");
    let eeprom = guard.as_deref().ok_or(StoredConfigError::NotRead)?;
    LoraConfig::parse_stored_config(eeprom)
}

/// How a stored configuration reads in a status document. Each outcome gets
/// its own state name, so "nothing stored" and "the read was cut short" do not
/// arrive at an operator as the same empty answer.
#[must_use]
pub fn stored_config_status_json(
    stored: &Result<Option<StoredRadioConfig>, StoredConfigError>,
) -> serde_json::Value {
    match stored {
        Ok(Some(config)) => serde_json::json!({
            "state": "read",
            "frequency_hz": config.frequency_hz,
            "bandwidth_hz": config.bandwidth_hz,
            "spreading_factor": config.spreading_factor,
            "coding_rate": config.coding_rate,
            "tx_power_dbm": config.tx_power_dbm,
        }),
        Ok(None) => serde_json::json!({ "state": "none" }),
        Err(StoredConfigError::NotRead) => serde_json::json!({ "state": "not_read" }),
        Err(error @ StoredConfigError::TruncatedImage { .. }) => serde_json::json!({
            "state": "truncated",
            "error": error.to_string(),
        }),
    }
}

/// Big-endian `u32` at `offset`; the caller has bounds-checked against
/// [`STORED_CONFIG_IMAGE_LEN`], which covers every field read here.
fn be_u32(bytes: &[u8], offset: usize) -> u32 {
    u32::from_be_bytes([bytes[offset], bytes[offset + 1], bytes[offset + 2], bytes[offset + 3]])
}

#[cfg(test)]
mod stored_config_tests {
    use super::*;

    /// Real values from a Heltec LoRa32 v4 on firmware 1.86, as
    /// `rnodeconf -i` reports them — a byte-order or offset mistake then
    /// reads as an obviously wrong number rather than a plausible one.
    fn device_image() -> Vec<u8> {
        let mut eeprom = vec![0u8; STORED_CONFIG_IMAGE_LEN];
        eeprom[rom::ADDR_CONF_SF] = 11;
        eeprom[rom::ADDR_CONF_CR] = 5;
        eeprom[rom::ADDR_CONF_TXP] = 22;
        eeprom[rom::ADDR_CONF_BW..rom::ADDR_CONF_BW + 4]
            .copy_from_slice(&250_000u32.to_be_bytes());
        eeprom[rom::ADDR_CONF_FREQ..rom::ADDR_CONF_FREQ + 4]
            .copy_from_slice(&917_375_000u32.to_be_bytes());
        eeprom[rom::ADDR_CONF_OK] = rom::CONF_OK_BYTE;
        eeprom
    }

    #[test]
    fn reads_the_configuration_a_device_is_holding() {
        let parsed = LoraConfig::parse_stored_config(&device_image())
            .expect("a complete image")
            .expect("CONF_OK is set");

        assert_eq!(
            parsed,
            StoredRadioConfig {
                frequency_hz: 917_375_000,
                bandwidth_hz: 250_000,
                spreading_factor: 11,
                coding_rate: 5,
                tx_power_dbm: 22,
            }
        );
    }

    /// Only the exact sentinel counts, so a half-written EEPROM cannot pass
    /// for a configuration.
    #[test]
    fn a_device_holding_no_configuration_reads_as_none() {
        let mut eeprom = device_image();
        eeprom[rom::ADDR_CONF_OK] = 0x00;
        assert_eq!(LoraConfig::parse_stored_config(&eeprom), Ok(None));

        eeprom[rom::ADDR_CONF_OK] = 0x72;
        assert_eq!(LoraConfig::parse_stored_config(&eeprom), Ok(None));
    }

    /// A caller deciding whether to write settings has to tell a short read
    /// from a device that genuinely holds nothing, so they get different
    /// answers rather than a shared `None`.
    #[test]
    fn a_truncated_image_is_an_error_not_an_absent_configuration() {
        let full = device_image();
        for len in [0, rom::ADDR_CONF_SF, rom::ADDR_CONF_FREQ, STORED_CONFIG_IMAGE_LEN - 1] {
            assert_eq!(
                LoraConfig::parse_stored_config(&full[..len]),
                Err(StoredConfigError::TruncatedImage { len }),
                "an image of {len} bytes cannot answer the question"
            );
        }
        assert!(LoraConfig::parse_stored_config(&full[..STORED_CONFIG_IMAGE_LEN]).is_ok());
    }

    /// The reply has to reach the parser. Sent through a live interface it
    /// lands in `record_command_response`, which had no `CMD_ROM_READ` arm and
    /// dropped the image, leaving the parser reachable only by a caller owning
    /// its own transport.
    #[test]
    fn a_rom_read_reply_reaches_the_parser_through_the_command_path() {
        let mut iface = LoraInterface::new("COM9", 115_200, LoraConfig::us915_default());
        assert_eq!(iface.stored_config(), Err(StoredConfigError::NotRead));

        assert!(
            iface.record_command_response(CMD_ROM_READ, &device_image()).expect("rom read"),
            "the reply is recorded, not passed over"
        );

        assert_eq!(
            iface.stored_config().expect("a complete image").expect("CONF_OK is set").frequency_hz,
            917_375_000
        );
        assert_eq!(iface.rnode_management_handle().stored_config(), iface.stored_config());
        assert_eq!(iface.runtime_status_json()["stored_config"]["state"].as_str(), Some("read"));
    }

    /// The three answers stay apart all the way out to a status document.
    #[test]
    fn a_short_reply_is_reported_as_truncated_not_as_an_unconfigured_device() {
        let mut iface = LoraInterface::new("COM9", 115_200, LoraConfig::us915_default());
        iface.record_command_response(CMD_ROM_READ, &device_image()[..4]).expect("short rom read");

        assert_eq!(
            iface.stored_config(),
            Err(StoredConfigError::TruncatedImage { len: 4 })
        );
        assert_eq!(
            iface.runtime_status_json()["stored_config"]["state"].as_str(),
            Some("truncated")
        );

        let mut blank = device_image();
        blank[rom::ADDR_CONF_OK] = 0x00;
        iface.record_command_response(CMD_ROM_READ, &blank).expect("rom read");
        assert_eq!(iface.stored_config(), Ok(None));
        assert_eq!(iface.runtime_status_json()["stored_config"]["state"].as_str(), Some("none"));
    }

    /// Read little-endian, 917.375 MHz becomes 3.4 GHz.
    #[test]
    fn multi_byte_fields_are_big_endian() {
        let parsed = LoraConfig::parse_stored_config(&device_image())
            .expect("a complete image")
            .expect("configured");

        assert_eq!(parsed.frequency_hz, 917_375_000);
        assert_ne!(parsed.frequency_hz, u32::from_le_bytes(917_375_000u32.to_be_bytes()));
    }
}
