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
