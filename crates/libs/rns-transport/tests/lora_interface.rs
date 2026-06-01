use rns_transport::iface::lora::{
    LoraConfig, LoraInterface, RNodeHardwareError, RNodeProbeStatus, RNodeRadioStatus,
    BATTERY_STATE_CHARGED, BATTERY_STATE_CHARGING, BATTERY_STATE_DISCHARGING,
    BATTERY_STATE_UNKNOWN, CMD_BANDWIDTH, CMD_BLINK, CMD_BT_CTRL, CMD_CR, CMD_DETECT,
    CMD_DISP_READ, CMD_ERROR, CMD_FB_EXT, CMD_FB_READ, CMD_FB_WRITE, CMD_FREQUENCY, CMD_FW_VERSION,
    CMD_LEAVE, CMD_LT_ALOCK, CMD_MCU, CMD_PLATFORM, CMD_RADIO_LOCK, CMD_RADIO_STATE, CMD_RANDOM,
    CMD_RESET, CMD_ROM_READ, CMD_SF, CMD_STAT_BAT, CMD_STAT_CHTM, CMD_STAT_CSMA, CMD_STAT_PHYPRM,
    CMD_STAT_RSSI, CMD_STAT_RX, CMD_STAT_SNR, CMD_STAT_TEMP, CMD_STAT_TX, CMD_ST_ALOCK,
    CMD_TXPOWER, DETECT_REQ, DETECT_RESP, ERROR_INITRADIO, ERROR_MEMORY_LOW, ERROR_MODEM_TIMEOUT,
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

#[test]
fn lora_interface_clears_python_per_packet_signal_stats_after_inbound_data() {
    let mut iface = LoraInterface::new("COM9", 115_200, LoraConfig::us915_default());

    iface.record_command_response(CMD_SF, &[9]).expect("spreading factor");
    iface.record_command_response(CMD_STAT_RSSI, &[97]).expect("rssi");
    iface.record_command_response(CMD_STAT_SNR, &[0xF8]).expect("negative snr");

    iface.record_inbound_data_frame();

    let status = iface.radio_status();
    assert_eq!(status.rssi_dbm, None);
    assert_eq!(status.snr_db, None);
    assert_eq!(status.signal_quality_percent, Some(57.9));
}

#[test]
fn rnode_radio_status_decodes_python_airtime_and_channel_stats() {
    let mut status = RNodeRadioStatus::default();

    assert!(status.accept_command(CMD_ST_ALOCK, &[0x0c, 0xe4]).expect("short airtime limit"));
    assert!(status.accept_command(CMD_LT_ALOCK, &[0x00, 0x96]).expect("long airtime limit"));
    assert!(status
        .accept_command(
            CMD_STAT_CHTM,
            &[0x01, 0x2c, 0x00, 0xc8, 0x00, 0x64, 0x00, 0x32, 97, 87, 0xff],
        )
        .expect("channel telemetry"));

    assert_eq!(status.short_airtime_limit_percent, Some(33.0));
    assert_eq!(status.long_airtime_limit_percent, Some(1.5));
    assert_eq!(status.airtime_short_percent, Some(3.0));
    assert_eq!(status.airtime_long_percent, Some(2.0));
    assert_eq!(status.channel_load_short_percent, Some(1.0));
    assert_eq!(status.channel_load_long_percent, Some(0.5));
    assert_eq!(status.current_rssi_dbm, Some(-60));
    assert_eq!(status.noise_floor_dbm, Some(-70));
    assert_eq!(status.interference_dbm, None);
}

#[test]
fn lora_interface_records_python_channel_interference_stats() {
    let mut iface = LoraInterface::new("COM9", 115_200, LoraConfig::us915_default());

    iface
        .record_command_response(
            CMD_STAT_CHTM,
            &[0x00, 0x64, 0x00, 0x32, 0x00, 0x19, 0x00, 0x0a, 107, 90, 117],
        )
        .expect("channel telemetry");

    let status = iface.radio_status();
    assert_eq!(status.airtime_short_percent, Some(1.0));
    assert_eq!(status.airtime_long_percent, Some(0.5));
    assert_eq!(status.channel_load_short_percent, Some(0.25));
    assert_eq!(status.channel_load_long_percent, Some(0.1));
    assert_eq!(status.current_rssi_dbm, Some(-50));
    assert_eq!(status.noise_floor_dbm, Some(-67));
    assert_eq!(status.interference_dbm, Some(-40));
}

#[test]
fn rnode_radio_status_decodes_python_phy_and_csma_stats() {
    let mut status = RNodeRadioStatus::default();

    assert!(status
        .accept_command(
            CMD_STAT_PHYPRM,
            &[0x30, 0x39, 0x01, 0xf4, 0x00, 0x0c, 0x00, 0x96, 0x00, 0x0a, 0x00, 0x14],
        )
        .expect("phy params"));
    assert!(status.accept_command(CMD_STAT_CSMA, &[3, 4, 9]).expect("csma params"));

    assert_eq!(status.symbol_time_ms, Some(12.345));
    assert_eq!(status.symbol_rate_baud, Some(500));
    assert_eq!(status.preamble_symbols, Some(12));
    assert_eq!(status.preamble_time_ms, Some(150));
    assert_eq!(status.csma_slot_time_ms, Some(10));
    assert_eq!(status.csma_difs_ms, Some(20));
    assert_eq!(status.csma_cw_band, Some(3));
    assert_eq!(status.csma_cw_min, Some(4));
    assert_eq!(status.csma_cw_max, Some(9));
}

#[test]
fn lora_interface_records_python_battery_and_temperature_stats() {
    let mut iface = LoraInterface::new("COM9", 115_200, LoraConfig::us915_default());

    iface.record_command_response(CMD_STAT_BAT, &[BATTERY_STATE_CHARGING, 150]).expect("battery");
    iface.record_command_response(CMD_STAT_TEMP, &[150]).expect("temperature");

    let status = iface.radio_status();
    assert_eq!(status.battery_state, Some(BATTERY_STATE_CHARGING));
    assert_eq!(status.battery_state_string(), "charging");
    assert_eq!(status.battery_percent, Some(100));
    assert_eq!(status.temperature_c, Some(30));

    iface.record_command_response(CMD_STAT_TEMP, &[230]).expect("invalid temperature");
    assert_eq!(iface.radio_status().temperature_c, None);
}

#[test]
fn rnode_radio_status_reports_python_battery_state_strings() {
    let mut status = RNodeRadioStatus::default();

    assert_eq!(status.battery_state_string(), "unknown");

    for (state, expected) in [
        (BATTERY_STATE_CHARGED, "charged"),
        (BATTERY_STATE_CHARGING, "charging"),
        (BATTERY_STATE_DISCHARGING, "discharging"),
        (BATTERY_STATE_UNKNOWN, "unknown"),
        (0xff, "unknown"),
    ] {
        status.battery_state = Some(state);
        assert_eq!(status.battery_state_string(), expected);
    }
}

#[test]
fn rnode_radio_status_decodes_python_display_payloads() {
    let mut status = RNodeRadioStatus::default();
    let framebuffer = vec![0xa5; 512];
    let display = vec![0x5a; 1024];

    assert!(status.accept_command(CMD_FB_READ, &framebuffer).expect("framebuffer"));
    assert!(status.accept_command(CMD_DISP_READ, &display).expect("display"));

    assert_eq!(status.framebuffer.as_deref(), Some(framebuffer.as_slice()));
    assert_eq!(status.display.as_deref(), Some(display.as_slice()));
}

#[test]
fn rnode_radio_status_rejects_malformed_display_payloads() {
    let mut status = RNodeRadioStatus::default();

    let err = status
        .accept_command(CMD_FB_READ, &[0; 511])
        .expect_err("short framebuffer response must fail");
    assert!(err.contains("framebuffer"), "unexpected framebuffer error: {err}");
    assert_eq!(status.framebuffer.as_deref(), Some([].as_slice()));

    let err = status
        .accept_command(CMD_DISP_READ, &[0; 1023])
        .expect_err("short display response must fail");
    assert!(err.contains("display"), "unexpected display error: {err}");
    assert_eq!(status.display.as_deref(), Some([].as_slice()));
}

#[test]
fn lora_interface_records_python_display_payloads() {
    let mut iface = LoraInterface::new("COM9", 115_200, LoraConfig::us915_default());
    let framebuffer = vec![0xa5; 512];
    let display = vec![0x5a; 1024];

    iface.record_command_response(CMD_FB_READ, &framebuffer).expect("framebuffer");
    iface.record_command_response(CMD_DISP_READ, &display).expect("display");

    let status = iface.radio_status();
    assert_eq!(status.framebuffer.as_deref(), Some(framebuffer.as_slice()));
    assert_eq!(status.display.as_deref(), Some(display.as_slice()));
}

#[test]
fn lora_interface_records_python_random_response() {
    let mut iface = LoraInterface::new("COM9", 115_200, LoraConfig::us915_default());

    assert!(iface.record_command_response(CMD_RANDOM, &[0xa5]).expect("random byte"));

    assert_eq!(iface.radio_status().random_byte, Some(0xa5));
}

#[test]
fn lora_interface_records_and_validates_rnode_radio_state() {
    let mut iface = LoraInterface::new("COM9", 115_200, LoraConfig::us915_default());

    assert!(!iface.online());
    assert!(iface
        .record_command_response(CMD_FREQUENCY, &915_000_000_u32.to_be_bytes())
        .expect("frequency"));
    assert!(iface
        .record_command_response(CMD_BANDWIDTH, &125_000_u32.to_be_bytes())
        .expect("bandwidth"));
    assert!(iface.record_command_response(CMD_TXPOWER, &[17]).expect("tx power"));
    assert!(iface.record_command_response(CMD_SF, &[9]).expect("spreading factor"));
    assert!(iface.record_command_response(CMD_CR, &[5]).expect("coding rate"));
    assert!(iface
        .record_command_response(CMD_RADIO_STATE, &[RADIO_STATE_ON])
        .expect("radio state"));
    assert!(iface.online());

    iface.validate_radio_status().expect("valid recorded radio state");
}

#[test]
fn lora_interface_validates_complete_python_startup_responses() {
    let mut iface = LoraInterface::new("COM9", 115_200, LoraConfig::us915_default());

    iface.record_command_response(CMD_DETECT, &[DETECT_RESP]).expect("detect");
    iface.record_command_response(CMD_FW_VERSION, &[1, 52]).expect("firmware");
    iface.record_command_response(CMD_PLATFORM, &[PLATFORM_ESP32]).expect("platform");
    iface.record_command_response(CMD_MCU, &[0x01]).expect("mcu");
    iface
        .record_command_response(CMD_FREQUENCY, &915_000_000_u32.to_be_bytes())
        .expect("frequency");
    iface.record_command_response(CMD_BANDWIDTH, &125_000_u32.to_be_bytes()).expect("bandwidth");
    iface.record_command_response(CMD_TXPOWER, &[17]).expect("tx power");
    iface.record_command_response(CMD_SF, &[9]).expect("spreading factor");
    iface.record_command_response(CMD_CR, &[5]).expect("coding rate");
    iface.record_command_response(CMD_RADIO_STATE, &[RADIO_STATE_ON]).expect("radio online");

    iface.validate_startup_responses().expect("complete startup responses");
}

#[test]
fn lora_interface_startup_response_validation_reports_first_python_gap() {
    let mut iface = LoraInterface::new("COM9", 115_200, LoraConfig::us915_default());

    let err = iface.validate_startup_responses().expect_err("missing probe must fail");
    assert!(err.contains("detect"), "unexpected startup validation error: {err}");

    iface.record_command_response(CMD_DETECT, &[DETECT_RESP]).expect("detect");
    iface.record_command_response(CMD_FW_VERSION, &[1, 52]).expect("firmware");
    iface.record_command_response(CMD_PLATFORM, &[PLATFORM_ESP32]).expect("platform");
    iface.record_command_response(CMD_MCU, &[0x01]).expect("mcu");

    let err = iface.validate_startup_responses().expect_err("missing radio state must fail");
    assert!(err.contains("bandwidth"), "unexpected startup validation error: {err}");
}

#[test]
fn lora_interface_rejects_python_online_esp32_reset_response() {
    let mut iface = LoraInterface::new("COM9", 115_200, LoraConfig::us915_default());

    assert_eq!(iface.last_command_error(), None);
    assert!(iface.record_command_response(CMD_PLATFORM, &[PLATFORM_ESP32]).expect("platform"));
    assert!(iface.record_command_response(CMD_RESET, &[RESET_ESP32]).expect("offline reset"));
    assert_eq!(iface.last_command_error(), None);

    iface.record_command_response(CMD_RADIO_STATE, &[RADIO_STATE_ON]).expect("radio online");

    let err = iface
        .record_command_response(CMD_RESET, &[RESET_ESP32])
        .expect_err("online ESP32 reset must fail");
    assert!(err.contains("ESP32 reset"), "unexpected reset error: {err}");
    assert_eq!(iface.last_command_error(), Some("ESP32 reset"));

    let err = iface.validate_startup_responses().expect_err("fatal reset must fail startup");
    assert!(err.contains("ESP32 reset"), "unexpected startup validation error: {err}");
}

#[test]
fn lora_interface_exposes_python_reported_bitrate() {
    let mut iface = LoraInterface::new("COM9", 115_200, LoraConfig::us915_default());

    iface.record_command_response(CMD_BANDWIDTH, &125_000_u32.to_be_bytes()).expect("bandwidth");
    iface.record_command_response(CMD_SF, &[9]).expect("spreading factor");
    iface.record_command_response(CMD_CR, &[5]).expect("coding rate");

    let bitrate = iface.reported_bitrate_bps().expect("reported bitrate");
    assert!((bitrate - 1757.8125).abs() < f64::EPSILON, "unexpected reported bitrate {bitrate}");
}

#[test]
fn rnode_hardware_error_classifies_python_error_commands() {
    assert_eq!(
        RNodeHardwareError::from_code(ERROR_MEMORY_LOW),
        RNodeHardwareError {
            code: ERROR_MEMORY_LOW,
            description: "Memory exhausted on connected device",
            fatal: false,
        }
    );
    assert_eq!(
        RNodeHardwareError::from_code(ERROR_MODEM_TIMEOUT),
        RNodeHardwareError {
            code: ERROR_MODEM_TIMEOUT,
            description: "Modem communication timed out on connected device",
            fatal: false,
        }
    );
    assert_eq!(
        RNodeHardwareError::from_code(ERROR_INITRADIO),
        RNodeHardwareError {
            code: ERROR_INITRADIO,
            description: "Radio initialisation failure",
            fatal: true,
        }
    );
    assert_eq!(
        RNodeHardwareError::from_code(0xff),
        RNodeHardwareError { code: 0xff, description: "Unknown hardware failure", fatal: true }
    );
}

#[test]
fn lora_interface_records_nonfatal_hardware_errors_like_python() {
    let mut iface = LoraInterface::new("COM9", 115_200, LoraConfig::us915_default());

    assert!(iface.record_command_response(CMD_ERROR, &[ERROR_MEMORY_LOW]).expect("memory low"));

    assert_eq!(
        iface.hardware_errors(),
        &[RNodeHardwareError {
            code: ERROR_MEMORY_LOW,
            description: "Memory exhausted on connected device",
            fatal: false,
        }]
    );
}

#[test]
fn lora_interface_rejects_fatal_hardware_errors_like_python() {
    let mut iface = LoraInterface::new("COM9", 115_200, LoraConfig::us915_default());
    assert_eq!(iface.last_command_error(), None);

    let err = iface
        .record_command_response(CMD_ERROR, &[ERROR_TXFAILED])
        .expect_err("fatal TX error must fail");

    assert!(err.contains("Hardware transmit failure"), "unexpected hardware error: {err}");
    assert!(iface.hardware_errors().is_empty());
    assert_eq!(iface.last_command_error(), Some("Hardware transmit failure"));
}

#[test]
fn rnode_probe_status_validates_required_python_startup_probe() {
    let mut status = RNodeProbeStatus::default();
    status.accept_command(CMD_DETECT, &[DETECT_RESP]).expect("detect");
    status.accept_command(CMD_FW_VERSION, &[1, 52]).expect("firmware");
    status.accept_command(CMD_PLATFORM, &[0x80]).expect("platform");
    status.accept_command(CMD_MCU, &[0x01]).expect("mcu");

    status.validate_startup_probe().expect("minimum supported RNode probe");

    status.accept_command(CMD_FW_VERSION, &[2, 0]).expect("newer major firmware");

    status.validate_startup_probe().expect("newer major firmware is accepted");
}

#[test]
fn rnode_probe_status_rejects_missing_or_unsupported_startup_probe() {
    let err = RNodeProbeStatus::default()
        .validate_startup_probe()
        .expect_err("missing detect response must fail");
    assert!(err.contains("detect"), "unexpected validation error: {err}");

    let mut old_firmware = RNodeProbeStatus::default();
    old_firmware.accept_command(CMD_DETECT, &[DETECT_RESP]).expect("detect");
    old_firmware.accept_command(CMD_FW_VERSION, &[1, 51]).expect("old firmware");
    old_firmware.accept_command(CMD_PLATFORM, &[0x80]).expect("platform");
    old_firmware.accept_command(CMD_MCU, &[0x01]).expect("mcu");

    let err = old_firmware.validate_startup_probe().expect_err("old firmware must fail");
    assert!(err.contains("firmware"), "unexpected validation error: {err}");
    assert!(err.contains("1.52"), "unexpected validation error: {err}");

    let mut missing_mcu = RNodeProbeStatus::default();
    missing_mcu.accept_command(CMD_DETECT, &[DETECT_RESP]).expect("detect");
    missing_mcu.accept_command(CMD_FW_VERSION, &[1, 52]).expect("firmware");
    missing_mcu.accept_command(CMD_PLATFORM, &[0x80]).expect("platform");

    let err = missing_mcu.validate_startup_probe().expect_err("missing MCU response must fail");
    assert!(err.contains("mcu"), "unexpected validation error: {err}");
}

#[test]
fn lora_interface_defaults_flow_control_off_and_allows_enabling() {
    let iface = LoraInterface::new("COM9", 115_200, LoraConfig::us915_default());
    assert!(!iface.flow_control());

    let iface = iface.with_flow_control(true);
    assert!(iface.flow_control());
}

#[test]
fn lora_interface_supports_tcp_rnode_endpoint() {
    let iface = LoraInterface::new_tcp("192.0.2.10:8001", LoraConfig::us915_default());

    assert_eq!(iface.bearer(), rns_transport::iface::lora::LoraBearer::Tcp);
    assert_eq!(iface.endpoint(), "192.0.2.10:8001");
    assert_eq!(iface.baud_rate(), None);
}

#[test]
fn lora_tcp_rnode_uses_python_activity_detect_probe() {
    let serial = LoraInterface::new("/dev/ttyACM0", 115_200, LoraConfig::us915_default());
    assert_eq!(serial.activity_probe(), None);

    let tcp = LoraInterface::new_tcp("192.0.2.10:8001", LoraConfig::us915_default());
    let probe = tcp.activity_probe().expect("tcp rnode activity probe");

    assert_eq!(probe.interval, std::time::Duration::from_millis(3_500));
    assert_eq!(probe.frames, vec![vec![FEND, CMD_DETECT, DETECT_REQ, FEND]]);
}

#[test]
fn lora_config_rejects_invalid_radio_parameters() {
    let invalid = LoraConfig {
        frequency_hz: 136_000_000,
        bandwidth_hz: 125_000,
        spreading_factor: 9,
        coding_rate: 5,
        tx_power_dbm: 17,
        max_payload_bytes: 220,
        airtime_limit_short_hundredths: None,
        airtime_limit_long_hundredths: None,
    };

    let err = invalid.validate().expect_err("frequency below RNode range must fail");
    assert!(err.contains("frequency_hz"));

    let invalid = LoraConfig { spreading_factor: 13, ..LoraConfig::us915_default() };
    let err = invalid.validate().expect_err("invalid spreading factor must fail");
    assert!(err.contains("spreading_factor"));

    let invalid = LoraConfig { coding_rate: 9, ..LoraConfig::us915_default() };
    let err = invalid.validate().expect_err("invalid coding rate must fail");
    assert!(err.contains("coding_rate"));

    let invalid =
        LoraConfig { airtime_limit_short_hundredths: Some(10_001), ..LoraConfig::us915_default() };
    let err = invalid.validate().expect_err("airtime over 100 percent must fail");
    assert!(err.contains("airtime_limit_short"));
}

#[test]
fn lora_region_defaults_select_expected_frequency() {
    assert_eq!(LoraConfig::for_region("US915").expect("US915").frequency_hz, 915_000_000);
    assert_eq!(LoraConfig::for_region("EU868").expect("EU868").frequency_hz, 868_000_000);
    assert!(LoraConfig::for_region("MARS1").is_none());
}
