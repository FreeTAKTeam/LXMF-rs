use std::net::Ipv4Addr;

use rns_transport::iface::lora::{
    LoraConfig, LoraInterface, RNodeHardwareError, RNodeProbeStatus, RNodeRadioStatus,
    RNodeBluetoothControl, BATTERY_STATE_CHARGED, BATTERY_STATE_CHARGING, BATTERY_STATE_DISCHARGING,
    BATTERY_STATE_UNKNOWN, CMD_BANDWIDTH, CMD_BLINK, CMD_BT_CTRL, CMD_CFG_READ, CMD_CONF_DELETE,
    CMD_CONF_SAVE, CMD_CR, CMD_DETECT, CMD_DISP_ADR, CMD_DISP_BLNK, CMD_DISP_INT, CMD_DISP_RCND,
    CMD_DISP_READ, CMD_DISP_ROT, CMD_DIS_IA, CMD_ERROR, CMD_FB_EXT, CMD_FB_READ, CMD_FB_WRITE,
    CMD_FREQUENCY, CMD_FW_HASH, CMD_FW_UPD, CMD_FW_VERSION, CMD_LEAVE, CMD_LT_ALOCK, CMD_MCU,
    CMD_NP_INT, CMD_PLATFORM, CMD_RADIO_LOCK, CMD_RADIO_STATE, CMD_RANDOM, CMD_RESET, CMD_ROM_READ,
    CMD_ROM_WIPE, CMD_ROM_WRITE, CMD_SF, CMD_STAT_BAT, CMD_STAT_CHTM, CMD_STAT_CSMA,
    CMD_STAT_PHYPRM, CMD_STAT_RSSI, CMD_STAT_RX, CMD_STAT_SNR, CMD_STAT_TEMP, CMD_STAT_TX,
    CMD_ST_ALOCK, CMD_TXPOWER, CMD_WIFI_CHN, CMD_WIFI_IP, CMD_WIFI_MODE, CMD_WIFI_NM, CMD_WIFI_PSK,
    CMD_WIFI_SSID, DETECT_REQ, DETECT_RESP, ERROR_INITRADIO, ERROR_MEMORY_LOW, ERROR_MODEM_TIMEOUT,
    ERROR_TXFAILED, PLATFORM_AVR, PLATFORM_ESP32, PLATFORM_NRF52, RADIO_STATE_ASK, RADIO_STATE_OFF,
    RADIO_STATE_ON, RESET_ESP32,
};

use rns_transport::kiss::{FEND, FESC, TFEND, TFESC};

const R_NODE_PROBE_FRAME_COUNT: usize = 4;

#[test]
fn lora_config_emits_rnode_probe_before_radio_commands() {
    let frames = LoraConfig::us915_default().command_frames();

    assert_eq!(
        &frames[..R_NODE_PROBE_FRAME_COUNT],
        &[
            vec![FEND, CMD_DETECT, DETECT_REQ, FEND],
            vec![FEND, CMD_FW_VERSION, 0x00, FEND],
            vec![FEND, CMD_PLATFORM, 0x00, FEND],
            vec![FEND, CMD_MCU, 0x00, FEND],
        ]
    );
}

#[test]
fn lora_config_emits_rnode_radio_commands() {
    let config = LoraConfig {
        frequency_hz: 915_000_000,
        bandwidth_hz: 125_000,
        spreading_factor: 9,
        coding_rate: 5,
        tx_power_dbm: 17,
        max_payload_bytes: 220,
        airtime_limit_short_hundredths: None,
        airtime_limit_long_hundredths: None,
    };

    assert_eq!(
        &config.command_frames()[R_NODE_PROBE_FRAME_COUNT..],
        &[
            vec![FEND, CMD_FREQUENCY, 0x36, 0x89, 0xCA, 0xDB, 0xDC, FEND],
            vec![FEND, CMD_BANDWIDTH, 0x00, 0x01, 0xE8, 0x48, FEND],
            vec![FEND, CMD_TXPOWER, 17, FEND],
            vec![FEND, CMD_SF, 9, FEND],
            vec![FEND, CMD_CR, 5, FEND],
            vec![FEND, CMD_RADIO_STATE, RADIO_STATE_ON, FEND],
        ]
    );
}

#[test]
fn lora_config_emits_rnode_airtime_lock_commands_before_radio_on() {
    let config = LoraConfig {
        airtime_limit_short_hundredths: Some(3_300),
        airtime_limit_long_hundredths: Some(150),
        ..LoraConfig::us915_default()
    };

    let frames = config.command_frames();

    assert_eq!(
        &frames[R_NODE_PROBE_FRAME_COUNT + 5..],
        &[
            vec![FEND, CMD_ST_ALOCK, 0x0C, 0xE4, FEND],
            vec![FEND, CMD_LT_ALOCK, 0x00, 0x96, FEND],
            vec![FEND, CMD_RADIO_STATE, RADIO_STATE_ON, FEND],
        ]
    );
}

#[test]
fn lora_config_emits_rnode_radio_off_and_leave_shutdown_commands() {
    let frames = LoraConfig::us915_default().shutdown_frames();

    assert_eq!(
        frames,
        vec![vec![FEND, CMD_RADIO_STATE, RADIO_STATE_OFF, FEND], vec![FEND, CMD_LEAVE, 0xff, FEND],]
    );
}

#[test]
fn lora_config_exposes_python_rnode_management_constants_and_query_frame() {
    assert_eq!(CMD_BLINK, 0x30);
    assert_eq!(CMD_BT_CTRL, 0x46);
    assert_eq!(CMD_ROM_READ, 0x51);
    assert_eq!(RADIO_STATE_ASK, 0xff);
    assert_eq!(
        LoraConfig::radio_state_query_frame(),
        vec![FEND, CMD_RADIO_STATE, RADIO_STATE_ASK, FEND]
    );
    assert_eq!(LoraConfig::blink_frame(0x03), vec![FEND, CMD_BLINK, 0x03, FEND]);
    assert_eq!(
        LoraConfig::blink_frame(FEND),
        vec![FEND, CMD_BLINK, FESC, TFEND, FEND]
    );
    assert_eq!(
        LoraConfig::bluetooth_control_frame(RNodeBluetoothControl::Enable),
        vec![FEND, CMD_BT_CTRL, 0x01, FEND]
    );
    assert_eq!(LoraConfig::bluetooth_enable_frame(), vec![FEND, CMD_BT_CTRL, 0x01, FEND]);
    assert_eq!(LoraConfig::bluetooth_disable_frame(), vec![FEND, CMD_BT_CTRL, 0x00, FEND]);
    assert_eq!(LoraConfig::bluetooth_pair_frame(), vec![FEND, CMD_BT_CTRL, 0x02, FEND]);
    assert_eq!(LoraConfig::rom_read_frame(), vec![FEND, CMD_ROM_READ, 0x00, FEND]);
}

#[test]
fn lora_config_builds_python_rnode_display_config_and_wifi_management_frames() {
    assert_eq!(LoraConfig::config_read_frame(), vec![FEND, CMD_CFG_READ, 0x00, FEND]);
    assert_eq!(LoraConfig::config_save_frame(), vec![FEND, CMD_CONF_SAVE, 0x00, FEND]);
    assert_eq!(LoraConfig::config_delete_frame(), vec![FEND, CMD_CONF_DELETE, 0x00, FEND]);
    assert_eq!(LoraConfig::rom_wipe_frame(), vec![FEND, CMD_ROM_WIPE, RESET_ESP32, FEND]);
    assert_eq!(
        LoraConfig::rom_write_frame(FEND, FESC),
        vec![FEND, CMD_ROM_WRITE, FESC, TFEND, FESC, TFESC, FEND]
    );
    assert_eq!(LoraConfig::display_intensity_frame(0x7f), vec![FEND, CMD_DISP_INT, 0x7f, FEND]);
    assert_eq!(LoraConfig::display_blanking_frame(0x20), vec![FEND, CMD_DISP_BLNK, 0x20, FEND]);
    assert_eq!(LoraConfig::display_rotation_frame(0x02), vec![FEND, CMD_DISP_ROT, 0x02, FEND]);
    assert_eq!(LoraConfig::display_recondition_frame(), vec![FEND, CMD_DISP_RCND, 0x01, FEND]);
    assert_eq!(LoraConfig::disable_interference_avoidance_frame(true), vec![FEND, CMD_DIS_IA, 0x01, FEND]);
    assert_eq!(LoraConfig::disable_interference_avoidance_frame(false), vec![FEND, CMD_DIS_IA, 0x00, FEND]);
    assert_eq!(LoraConfig::neopixel_intensity_frame(0x42), vec![FEND, CMD_NP_INT, 0x42, FEND]);
    assert_eq!(LoraConfig::display_address_frame(0x3c), vec![FEND, CMD_DISP_ADR, 0x3c, FEND]);
    assert_eq!(LoraConfig::firmware_update_indicator_frame(), vec![FEND, CMD_FW_UPD, 0x01, FEND]);
    assert_eq!(
        LoraConfig::firmware_hash_frame(&[FEND, 0x01]),
        vec![FEND, CMD_FW_HASH, FESC, TFEND, 0x01, FEND]
    );
    assert_eq!(LoraConfig::wifi_mode_frame(0x02), vec![FEND, CMD_WIFI_MODE, 0x02, FEND]);
    assert_eq!(LoraConfig::wifi_channel_frame(14).expect("valid channel"), vec![FEND, CMD_WIFI_CHN, 14, FEND]);
    assert!(LoraConfig::wifi_channel_frame(0).is_err());
    assert_eq!(
        LoraConfig::wifi_ip_frame(Some(Ipv4Addr::new(192, 168, 4, 1))),
        vec![FEND, CMD_WIFI_IP, FESC, TFEND, 168, 4, 1, FEND]
    );
    assert_eq!(LoraConfig::wifi_ip_frame(None), vec![FEND, CMD_WIFI_IP, 0, 0, 0, 0, FEND]);
    assert_eq!(
        LoraConfig::wifi_netmask_frame(Some(Ipv4Addr::new(255, 255, 255, 0))),
        vec![FEND, CMD_WIFI_NM, 255, 255, 255, 0, FEND]
    );
    assert_eq!(LoraConfig::wifi_netmask_frame(None), vec![FEND, CMD_WIFI_NM, 0, 0, 0, 0, FEND]);
    assert_eq!(
        LoraConfig::wifi_ssid_frame(Some("net")).expect("valid ssid"),
        vec![FEND, CMD_WIFI_SSID, b'n', b'e', b't', 0x00, FEND]
    );
    assert_eq!(LoraConfig::wifi_ssid_frame(None).expect("empty ssid"), vec![FEND, CMD_WIFI_SSID, 0x00, FEND]);
    assert!(LoraConfig::wifi_ssid_frame(Some("123456789012345678901234567890123")).is_err());
    assert_eq!(
        LoraConfig::wifi_psk_frame(Some("1234567")).expect("valid psk"),
        vec![FEND, CMD_WIFI_PSK, b'1', b'2', b'3', b'4', b'5', b'6', b'7', 0x00, FEND]
    );
    assert_eq!(LoraConfig::wifi_psk_frame(None).expect("empty psk"), vec![FEND, CMD_WIFI_PSK, 0x00, FEND]);
    assert!(LoraConfig::wifi_psk_frame(Some("123456")).is_err());
}

#[test]
fn rnode_probe_status_decodes_detect_firmware_platform_and_mcu() {
    let mut status = RNodeProbeStatus::default();

    assert!(status.accept_command(CMD_DETECT, &[DETECT_RESP]).expect("detect response"));
    assert!(status.accept_command(CMD_FW_VERSION, &[1, 74]).expect("firmware response"));
    assert!(status.accept_command(CMD_PLATFORM, &[0x80]).expect("platform response"));
    assert!(status.accept_command(CMD_MCU, &[0x01]).expect("mcu response"));

    assert!(status.detected);
    assert_eq!(status.firmware_version, Some((1, 74)));
    assert_eq!(status.platform, Some(0x80));
    assert_eq!(status.mcu, Some(0x01));
}

#[test]
fn rnode_probe_status_rejects_malformed_probe_responses() {
    let mut status = RNodeProbeStatus::default();

    let err = status.accept_command(CMD_FW_VERSION, &[1]).expect_err("short firmware response");
    assert!(err.contains("firmware"));

    let err = status.accept_command(CMD_PLATFORM, &[]).expect_err("missing platform response");
    assert!(err.contains("platform"));

    let err = status.accept_command(CMD_MCU, &[1, 2]).expect_err("oversized mcu response");
    assert!(err.contains("mcu"));
}

#[test]
fn rnode_probe_status_marks_negative_detect_response() {
    let mut status = RNodeProbeStatus::default();

    assert!(status.accept_command(CMD_DETECT, &[0x00]).expect("negative detect response"));

    assert!(!status.detected);
}

#[test]
fn rnode_probe_status_ignores_unrelated_commands() {
    let mut status = RNodeProbeStatus::default();

    assert!(!status.accept_command(CMD_TXPOWER, &[17]).expect("unrelated command"));

    assert_eq!(status, RNodeProbeStatus::default());
}

#[test]
fn rnode_probe_status_identifies_python_display_platforms() {
    let mut status = RNodeProbeStatus::default();
    assert!(!status.has_display());

    status.accept_command(CMD_PLATFORM, &[PLATFORM_ESP32]).expect("esp32 platform");
    assert!(status.has_display());

    status.accept_command(CMD_PLATFORM, &[PLATFORM_NRF52]).expect("nrf52 platform");
    assert!(status.has_display());

    status.accept_command(CMD_PLATFORM, &[PLATFORM_AVR]).expect("avr platform");
    assert!(!status.has_display());
}

#[test]
fn rnode_probe_status_builds_python_display_command_frames_only_for_display_platforms() {
    let mut status = RNodeProbeStatus::default();
    assert_eq!(status.external_framebuffer_frame(true), None);
    assert_eq!(status.framebuffer_read_frame(), None);
    assert_eq!(status.display_read_frame(), None);

    status.accept_command(CMD_PLATFORM, &[PLATFORM_ESP32]).expect("esp32 platform");
    assert_eq!(status.external_framebuffer_frame(true), Some(vec![FEND, CMD_FB_EXT, 0x01, FEND]));
    assert_eq!(status.external_framebuffer_frame(false), Some(vec![FEND, CMD_FB_EXT, 0x00, FEND]));
    assert_eq!(status.framebuffer_read_frame(), Some(vec![FEND, CMD_FB_READ, 0x01, FEND]));
    assert_eq!(status.display_read_frame(), Some(vec![FEND, CMD_DISP_READ, 0x01, FEND]));

    status.accept_command(CMD_PLATFORM, &[PLATFORM_AVR]).expect("avr platform");
    assert_eq!(status.external_framebuffer_frame(true), None);
    assert_eq!(status.framebuffer_read_frame(), None);
    assert_eq!(status.display_read_frame(), None);
}

#[test]
fn rnode_probe_status_builds_python_framebuffer_write_frames() {
    let mut status = RNodeProbeStatus::default();
    let line_data = [0x01, FEND, FESC, 0x04, 0x05, 0x06, 0x07, 0x08];

    assert_eq!(status.framebuffer_write_frame(2, line_data), None);

    status.accept_command(CMD_PLATFORM, &[PLATFORM_NRF52]).expect("nrf52 platform");

    assert_eq!(
        status.framebuffer_write_frame(2, line_data),
        Some(vec![
            FEND,
            CMD_FB_WRITE,
            0x02,
            0x01,
            FESC,
            TFEND,
            FESC,
            TFESC,
            0x04,
            0x05,
            0x06,
            0x07,
            0x08,
            FEND,
        ])
    );
}

#[test]
fn rnode_probe_status_builds_python_display_image_line_frames() {
    let mut status = RNodeProbeStatus::default();
    let image = [0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 99];

    assert_eq!(status.display_image_frames(&image), None);

    status.accept_command(CMD_PLATFORM, &[PLATFORM_ESP32]).expect("esp32 platform");

    assert_eq!(
        status.display_image_frames(&image),
        Some(vec![
            vec![FEND, CMD_FB_WRITE, 0, 0, 1, 2, 3, 4, 5, 6, 7, FEND],
            vec![FEND, CMD_FB_WRITE, 1, 8, 9, 10, 11, 12, 13, 14, 15, FEND],
        ])
    );
}

#[test]
fn rnode_probe_status_classifies_python_esp32_reset_response() {
    let mut status = RNodeProbeStatus::default();
    status.accept_command(CMD_PLATFORM, &[PLATFORM_ESP32]).expect("esp32 platform");

    assert!(status
        .accept_reset_response(&[RESET_ESP32], false)
        .expect("offline ESP32 reset is informational"));

    let err = status
        .accept_reset_response(&[RESET_ESP32], true)
        .expect_err("online ESP32 reset must match Python fatal behavior");
    assert!(err.contains("ESP32 reset"), "unexpected reset error: {err}");

    status.accept_command(CMD_PLATFORM, &[PLATFORM_NRF52]).expect("nrf52 platform");
    assert!(status
        .accept_reset_response(&[RESET_ESP32], true)
        .expect("non-ESP32 reset value is ignored"));

    let err = status.accept_reset_response(&[], true).expect_err("missing reset payload");
    assert!(err.contains("reset"), "unexpected reset error: {err}");

    assert!(!status.accept_command(CMD_RESET, &[RESET_ESP32]).expect("reset is not probe status"));
}

#[test]
fn rnode_probe_status_builds_python_hard_reset_frame() {
    assert_eq!(RNodeProbeStatus::hard_reset_frame(), vec![FEND, CMD_RESET, RESET_ESP32, FEND]);
}

#[test]
fn lora_interface_records_rnode_probe_command_status() {
    let mut iface = LoraInterface::new("COM9", 115_200, LoraConfig::us915_default());

    assert_eq!(iface.probe_status(), RNodeProbeStatus::default());
    assert!(iface.record_probe_command(CMD_DETECT, &[DETECT_RESP]).expect("detect"));
    assert!(iface.record_probe_command(CMD_FW_VERSION, &[1, 74]).expect("firmware"));
    assert!(iface.record_probe_command(CMD_PLATFORM, &[0x80]).expect("platform"));
    assert!(iface.record_probe_command(CMD_MCU, &[0x01]).expect("mcu"));

    assert_eq!(
        iface.probe_status(),
        RNodeProbeStatus {
            detected: true,
            firmware_version: Some((1, 74)),
            platform: Some(0x80),
            mcu: Some(0x01),
        }
    );
}

#[test]
fn lora_interface_validates_recorded_rnode_probe_status() {
    let mut iface = LoraInterface::new("COM9", 115_200, LoraConfig::us915_default());
    iface.record_probe_command(CMD_DETECT, &[DETECT_RESP]).expect("detect");
    iface.record_probe_command(CMD_FW_VERSION, &[1, 52]).expect("firmware");
    iface.record_probe_command(CMD_PLATFORM, &[0x80]).expect("platform");
    iface.record_probe_command(CMD_MCU, &[0x01]).expect("mcu");

    iface.validate_probe_status().expect("valid recorded probe");
}

#[test]
fn rnode_radio_status_decodes_and_validates_python_radio_state() {
    let config = LoraConfig::us915_default();
    let mut status = RNodeRadioStatus::default();

    assert!(status
        .accept_command(CMD_FREQUENCY, &915_000_042_u32.to_be_bytes())
        .expect("frequency"));
    assert!(status.accept_command(CMD_BANDWIDTH, &125_000_u32.to_be_bytes()).expect("bandwidth"));
    assert!(status.accept_command(CMD_TXPOWER, &[17]).expect("tx power"));
    assert!(status.accept_command(CMD_SF, &[9]).expect("spreading factor"));
    assert!(status.accept_command(CMD_CR, &[5]).expect("coding rate"));
    assert!(status.accept_command(CMD_RADIO_STATE, &[RADIO_STATE_ON]).expect("radio state"));

    status.validate_config(config, RADIO_STATE_ON).expect("matching reported radio state");
    assert_eq!(status.frequency_hz, Some(915_000_042));
    assert_eq!(status.bandwidth_hz, Some(125_000));
    assert_eq!(status.tx_power_dbm, Some(17));
    assert_eq!(status.spreading_factor, Some(9));
    assert_eq!(status.coding_rate, Some(5));
    assert_eq!(status.radio_state, Some(RADIO_STATE_ON));
}

#[test]
fn rnode_radio_status_records_python_radio_lock() {
    let mut status = RNodeRadioStatus::default();

    assert!(status.accept_command(CMD_RADIO_LOCK, &[0x01]).expect("radio lock"));

    assert_eq!(status.radio_lock, Some(0x01));
}

#[test]
fn rnode_radio_status_defaults_match_python_rnode_initial_telemetry() {
    let status = RNodeRadioStatus::default();

    assert_eq!(status.airtime_short_percent, Some(0.0));
    assert_eq!(status.airtime_long_percent, Some(0.0));
    assert_eq!(status.channel_load_short_percent, Some(0.0));
    assert_eq!(status.channel_load_long_percent, Some(0.0));
    assert_eq!(status.battery_state, Some(BATTERY_STATE_UNKNOWN));
    assert_eq!(status.battery_percent, Some(0));
    assert_eq!(status.framebuffer.as_deref(), Some([].as_slice()));
    assert_eq!(status.display.as_deref(), Some([].as_slice()));
}

#[test]
fn rnode_radio_status_rejects_python_radio_state_mismatches() {
    let config = LoraConfig::us915_default();
    let mut mismatched_frequency = RNodeRadioStatus::default();
    mismatched_frequency
        .accept_command(CMD_FREQUENCY, &914_999_899_u32.to_be_bytes())
        .expect("frequency");
    mismatched_frequency
        .accept_command(CMD_BANDWIDTH, &125_000_u32.to_be_bytes())
        .expect("bandwidth");
    mismatched_frequency.accept_command(CMD_TXPOWER, &[17]).expect("tx power");
    mismatched_frequency.accept_command(CMD_SF, &[9]).expect("spreading factor");
    mismatched_frequency.accept_command(CMD_CR, &[5]).expect("coding rate");
    mismatched_frequency.accept_command(CMD_RADIO_STATE, &[RADIO_STATE_ON]).expect("radio state");

    let err = mismatched_frequency
        .validate_config(config, RADIO_STATE_ON)
        .expect_err("frequency mismatch above Python tolerance must fail");
    assert!(err.contains("frequency"), "unexpected validation error: {err}");

    let mut missing_bandwidth = RNodeRadioStatus::default();
    missing_bandwidth.accept_command(CMD_TXPOWER, &[17]).expect("tx power");
    missing_bandwidth.accept_command(CMD_SF, &[9]).expect("spreading factor");
    missing_bandwidth.accept_command(CMD_RADIO_STATE, &[RADIO_STATE_ON]).expect("radio state");

    let err = missing_bandwidth
        .validate_config(config, RADIO_STATE_ON)
        .expect_err("missing bandwidth response must fail");
    assert!(err.contains("bandwidth"), "unexpected validation error: {err}");

    let mut missing_coding_rate = RNodeRadioStatus::default();
    missing_coding_rate
        .accept_command(CMD_BANDWIDTH, &125_000_u32.to_be_bytes())
        .expect("bandwidth");
    missing_coding_rate.accept_command(CMD_TXPOWER, &[17]).expect("tx power");
    missing_coding_rate.accept_command(CMD_SF, &[9]).expect("spreading factor");
    missing_coding_rate.accept_command(CMD_RADIO_STATE, &[RADIO_STATE_ON]).expect("radio state");

    let err = missing_coding_rate
        .validate_config(config, RADIO_STATE_ON)
        .expect_err("missing coding rate response must fail");
    assert!(err.contains("coding rate"), "unexpected validation error: {err}");

    let mut mismatched_coding_rate = RNodeRadioStatus::default();
    mismatched_coding_rate
        .accept_command(CMD_BANDWIDTH, &125_000_u32.to_be_bytes())
        .expect("bandwidth");
    mismatched_coding_rate.accept_command(CMD_TXPOWER, &[17]).expect("tx power");
    mismatched_coding_rate.accept_command(CMD_SF, &[9]).expect("spreading factor");
    mismatched_coding_rate.accept_command(CMD_CR, &[6]).expect("coding rate");
    mismatched_coding_rate.accept_command(CMD_RADIO_STATE, &[RADIO_STATE_ON]).expect("radio state");

    let err = mismatched_coding_rate
        .validate_config(config, RADIO_STATE_ON)
        .expect_err("coding rate mismatch must fail");
    assert!(err.contains("coding rate"), "unexpected validation error: {err}");
}

#[test]
fn rnode_radio_status_computes_python_reported_bitrate() {
    let mut status = RNodeRadioStatus::default();

    assert_eq!(status.reported_bitrate_bps(), None);

    status.accept_command(CMD_BANDWIDTH, &125_000_u32.to_be_bytes()).expect("bandwidth");
    status.accept_command(CMD_SF, &[9]).expect("spreading factor");
    status.accept_command(CMD_CR, &[5]).expect("coding rate");

    let bitrate = status.reported_bitrate_bps().expect("bitrate");
    assert!((bitrate - 1757.8125).abs() < f64::EPSILON, "unexpected reported bitrate {bitrate}");
}

#[test]
fn rnode_radio_status_decodes_python_counter_and_signal_stats() {
    let mut status = RNodeRadioStatus::default();

    status.accept_command(CMD_SF, &[9]).expect("spreading factor");
    assert!(status.accept_command(CMD_STAT_RX, &1234_u32.to_be_bytes()).expect("rx count"));
    assert!(status.accept_command(CMD_STAT_TX, &9876_u32.to_be_bytes()).expect("tx count"));
    assert!(status.accept_command(CMD_STAT_RSSI, &[97]).expect("rssi"));
    assert!(status.accept_command(CMD_STAT_SNR, &[8]).expect("snr"));

    assert_eq!(status.stat_rx, Some(1234));
    assert_eq!(status.stat_tx, Some(9876));
    assert_eq!(status.rssi_dbm, Some(-60));
    assert_eq!(status.snr_db, Some(2.0));
    assert_eq!(status.signal_quality_percent, Some(78.9));
}

#[test]
fn lora_interface_records_python_counter_and_signal_stats() {
    let mut iface = LoraInterface::new("COM9", 115_200, LoraConfig::us915_default());

    iface.record_command_response(CMD_SF, &[9]).expect("spreading factor");
    iface.record_command_response(CMD_STAT_RX, &1234_u32.to_be_bytes()).expect("rx count");
    iface.record_command_response(CMD_STAT_TX, &9876_u32.to_be_bytes()).expect("tx count");
    iface.record_command_response(CMD_STAT_RSSI, &[97]).expect("rssi");
    iface.record_command_response(CMD_STAT_SNR, &[0xF8]).expect("negative snr");

    let status = iface.radio_status();
    assert_eq!(status.stat_rx, Some(1234));
    assert_eq!(status.stat_tx, Some(9876));
    assert_eq!(status.rssi_dbm, Some(-60));
    assert_eq!(status.snr_db, Some(-2.0));
    assert_eq!(status.signal_quality_percent, Some(57.9));
}
