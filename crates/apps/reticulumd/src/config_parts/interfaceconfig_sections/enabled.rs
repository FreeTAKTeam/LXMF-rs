const RETICULUM_CONFIG_MTU: usize = lxmf::constants::RETICULUM_MTU;

impl InterfaceConfig {

    pub fn enabled(&self) -> bool {
        self.enabled.unwrap_or(false) || self.interface_enabled.unwrap_or(false)
    }

    pub fn outgoing(&self) -> bool {
        self.outgoing.unwrap_or(true)
    }

    pub fn settings_json(&self) -> Option<JsonValue> {
        let mut settings = JsonMap::new();
        if let Ok(configured_mode) = self.configured_interface_mode() {
            let override_mode = self.discoverable_mode_override(configured_mode);
            let mode = override_mode.unwrap_or(configured_mode);
            if self.interface_mode_raw().is_some()
                || override_mode.is_some()
            {
                settings
                    .insert("interface_mode".to_string(), JsonValue::String(mode.as_str().into()));
            }
        }
        insert_opt_bool(&mut settings, "outgoing", self.outgoing);
        insert_opt_u64(&mut settings, "bitrate", self.bitrate);
        insert_opt_u64(&mut settings, "announce_cap", self.announce_cap);
        insert_opt_u64(&mut settings, "announce_rate_target", self.announce_rate_target);
        if self.announce_rate_target.is_some() {
            insert_opt_u64(
                &mut settings,
                "announce_rate_grace",
                Some(self.announce_rate_grace.unwrap_or(0)),
            );
            insert_opt_u64(
                &mut settings,
                "announce_rate_penalty",
                Some(self.announce_rate_penalty.unwrap_or(0)),
            );
        } else {
            insert_opt_u64(&mut settings, "announce_rate_grace", self.announce_rate_grace);
            insert_opt_u64(&mut settings, "announce_rate_penalty", self.announce_rate_penalty);
        }
        insert_opt_bool(&mut settings, "bootstrap_only", self.bootstrap_only);
        insert_opt_bool(&mut settings, "ignore_config_warnings", self.ignore_config_warnings);
        insert_opt_u64(&mut settings, "ifac_size", self.ifac_size);
        insert_opt_string(&mut settings, "network_name", self.ifac_network_name());
        insert_opt_string(&mut settings, "passphrase", self.ifac_passphrase());
        insert_opt_bool(&mut settings, "ingress_control", self.ingress_control);
        insert_opt_bool(&mut settings, "egress_control", self.egress_control);
        insert_opt_u64(
            &mut settings,
            "ic_max_held_announces",
            self.ic_max_held_announces,
        );
        insert_opt_f64(&mut settings, "ic_burst_hold", self.ic_burst_hold);
        insert_opt_f64(&mut settings, "ic_burst_freq_new", self.ic_burst_freq_new);
        insert_opt_f64(&mut settings, "ic_burst_freq", self.ic_burst_freq);
        insert_opt_f64(
            &mut settings,
            "ic_pr_burst_freq_new",
            self.ic_pr_burst_freq_new,
        );
        insert_opt_f64(&mut settings, "ic_pr_burst_freq", self.ic_pr_burst_freq);
        insert_opt_f64(&mut settings, "ec_pr_freq", self.ec_pr_freq);
        insert_opt_f64(&mut settings, "ic_new_time", self.ic_new_time);
        insert_opt_f64(&mut settings, "ic_burst_penalty", self.ic_burst_penalty);
        insert_opt_f64(
            &mut settings,
            "ic_held_release_interval",
            self.ic_held_release_interval,
        );
        insert_opt_bool(&mut settings, "discoverable", self.discoverable);
        insert_opt_u64(
            &mut settings,
            "announce_interval",
            self.discovery_announce_interval_secs(),
        );
        insert_opt_u64(
            &mut settings,
            "discovery_stamp_value",
            self.discovery_stamp_value,
        );
        insert_opt_string(&mut settings, "discovery_name", self.discovery_name.as_ref());
        insert_opt_bool(&mut settings, "discovery_encrypt", self.discovery_encrypt);
        insert_opt_string(&mut settings, "reachable_on", self.reachable_on.as_ref());
        insert_opt_bool(&mut settings, "publish_ifac", self.publish_ifac);
        insert_opt_f64(&mut settings, "latitude", self.latitude);
        insert_opt_f64(&mut settings, "longitude", self.longitude);
        insert_opt_f64(&mut settings, "height", self.height);
        insert_opt_u64(
            &mut settings,
            "discovery_frequency",
            self.discovery_frequency,
        );
        insert_opt_u64(&mut settings, "discovery_bandwidth", self.discovery_bandwidth);
        insert_opt_u64(
            &mut settings,
            "discovery_modulation",
            self.discovery_modulation,
        );
        match self.kind.as_str() {
            "tcp_client" => {
                insert_opt_string(&mut settings, "host", self.host.as_ref());
                insert_opt_u64(&mut settings, "port", self.port.map(u64::from));
                insert_opt_bool(&mut settings, "prefer_ipv6", self.prefer_ipv6);
                insert_opt_bool(&mut settings, "i2p_tunneled", self.i2p_tunneled);
                insert_opt_u64(&mut settings, "connect_timeout", self.connect_timeout);
                insert_opt_u64(&mut settings, "max_reconnect_tries", self.max_reconnect_tries);
                insert_opt_u64(&mut settings, "mtu", self.mtu.map(|v| v as u64));
            }
            "tcp_server" => {
                insert_opt_string(&mut settings, "host", self.host.as_ref());
                insert_opt_string(&mut settings, "device", self.device.as_ref());
                insert_opt_u64(&mut settings, "port", self.port.map(u64::from));
                insert_opt_bool(&mut settings, "prefer_ipv6", self.prefer_ipv6);
                insert_opt_bool(&mut settings, "i2p_tunneled", self.i2p_tunneled);
                insert_opt_u64(&mut settings, "mtu", self.mtu.map(|v| v as u64));
            }
            "backbone" => {
                insert_opt_string(&mut settings, "host", self.host.as_ref());
                insert_opt_string(&mut settings, "device", self.device.as_ref());
                insert_opt_u64(&mut settings, "port", self.port.map(u64::from));
                insert_opt_bool(&mut settings, "prefer_ipv6", self.prefer_ipv6);
                insert_opt_bool(&mut settings, "i2p_tunneled", self.i2p_tunneled);
                insert_opt_u64(&mut settings, "mtu", self.mtu.map(|v| v as u64));
            }
            "backbone_client" => {
                insert_opt_string(&mut settings, "host", self.host.as_ref());
                insert_opt_u64(&mut settings, "port", self.port.map(u64::from));
                insert_opt_string(&mut settings, "target_host", self.target_host.as_ref());
                insert_opt_u64(&mut settings, "target_port", self.target_port.map(u64::from));
                insert_opt_bool(&mut settings, "prefer_ipv6", self.prefer_ipv6);
                insert_opt_bool(&mut settings, "i2p_tunneled", self.i2p_tunneled);
                insert_opt_u64(&mut settings, "connect_timeout", self.connect_timeout);
                insert_opt_u64(&mut settings, "max_reconnect_tries", self.max_reconnect_tries);
                insert_opt_u64(&mut settings, "mtu", self.mtu.map(|v| v as u64));
            }
            "local" | "local_client" => {
                insert_opt_string(
                    &mut settings,
                    "shared_instance_type",
                    self.shared_instance_type.as_ref(),
                );
                insert_opt_string(&mut settings, "instance_name", self.instance_name.as_ref());
                insert_opt_string(&mut settings, "socket_path", self.socket_path.as_ref());
                insert_opt_string(&mut settings, "host", self.host.as_ref());
                insert_opt_u64(&mut settings, "port", self.port.map(u64::from));
                insert_opt_u64(&mut settings, "connect_timeout", self.connect_timeout);
                insert_opt_u64(&mut settings, "max_reconnect_tries", self.max_reconnect_tries);
                insert_opt_u64(&mut settings, "mtu", self.mtu.map(|v| v as u64));
            }
            "pipe" => {
                insert_opt_string(&mut settings, "command", self.command.as_ref());
                insert_opt_f64(&mut settings, "respawn_delay", self.respawn_delay);
                insert_opt_u64(&mut settings, "mtu", self.mtu.map(|v| v as u64));
            }
            "i2p" => {
                insert_opt_string_array(&mut settings, "peers", self.peers.as_ref());
                insert_opt_bool(&mut settings, "connectable", self.connectable);
                insert_opt_string(&mut settings, "sam_host", self.sam_host.as_ref());
                insert_opt_u64(&mut settings, "sam_port", self.sam_port.map(u64::from));
                insert_opt_u64(&mut settings, "mtu", self.mtu.map(|value| value as u64));
                insert_opt_u64(&mut settings, "reconnect_backoff_ms", self.reconnect_backoff_ms);
                insert_opt_string(&mut settings, "state_path", self.state_path.as_ref());
            }
            "udp" => {
                insert_opt_string(&mut settings, "host", self.host.as_ref());
                insert_opt_u64(&mut settings, "port", self.port.map(u64::from));
                insert_opt_string(&mut settings, "device", self.device.as_ref());
                insert_opt_string(&mut settings, "target_host", self.target_host.as_ref());
                insert_opt_u64(&mut settings, "target_port", self.target_port.map(u64::from));
            }
            "auto" => {
                insert_opt_string(&mut settings, "group_id", self.group_id.as_ref());
                insert_opt_string(&mut settings, "discovery_scope", self.discovery_scope.as_ref());
                insert_opt_u64(&mut settings, "discovery_port", self.discovery_port.map(u64::from));
                insert_opt_u64(&mut settings, "data_port", self.data_port.map(u64::from));
                insert_opt_string(
                    &mut settings,
                    "multicast_address_type",
                    self.multicast_address_type.as_ref(),
                );
                if let Some(address) = self.auto_discovery_multicast_address() {
                    settings.insert(
                        "discovery_multicast_address".to_string(),
                        JsonValue::String(address),
                    );
                }
                insert_opt_string_array(&mut settings, "devices", self.devices.as_ref());
                insert_opt_string_array(
                    &mut settings,
                    "ignored_devices",
                    self.ignored_devices.as_ref(),
                );
            }
            "serial" => {
                insert_opt_string(&mut settings, "device", self.device.as_ref());
                insert_opt_u64(&mut settings, "baud_rate", self.baud_rate.map(u64::from));
                insert_opt_u64(&mut settings, "data_bits", self.data_bits.map(u64::from));
                insert_opt_string(&mut settings, "parity", self.parity.as_ref());
                insert_opt_u64(&mut settings, "stop_bits", self.stop_bits.map(u64::from));
                if let Some(flow_control) = self.flow_control_name() {
                    settings.insert(
                        "flow_control".to_string(),
                        JsonValue::String(flow_control.to_string()),
                    );
                }
                insert_opt_u64(&mut settings, "mtu", self.mtu.map(|v| v as u64));
                insert_opt_u64(&mut settings, "reconnect_backoff_ms", self.reconnect_backoff_ms);
                insert_opt_u64(
                    &mut settings,
                    "max_reconnect_backoff_ms",
                    self.max_reconnect_backoff_ms,
                );
            }
            "weave" => {
                insert_opt_string(&mut settings, "device", self.device.as_ref());
                insert_opt_u64(&mut settings, "baud_rate", self.baud_rate.map(u64::from));
                insert_opt_u64(&mut settings, "mtu", self.mtu.map(|value| value as u64));
                insert_opt_u64(&mut settings, "reconnect_backoff_ms", self.reconnect_backoff_ms);
                insert_opt_u64(
                    &mut settings,
                    "max_reconnect_backoff_ms",
                    self.max_reconnect_backoff_ms,
                );
            }
            "kiss" => {
                insert_opt_string(&mut settings, "device", self.device.as_ref());
                insert_opt_u64(&mut settings, "baud_rate", self.baud_rate.map(u64::from));
                insert_opt_u64(&mut settings, "mtu", self.mtu.map(|v| v as u64));
                insert_opt_u64(&mut settings, "preamble_ms", self.preamble_ms.map(u64::from));
                insert_opt_u64(&mut settings, "tx_tail_ms", self.tx_tail_ms.map(u64::from));
                insert_opt_u64(&mut settings, "persistence", self.persistence.map(u64::from));
                insert_opt_u64(&mut settings, "slot_time_ms", self.slot_time_ms.map(u64::from));
                if let Some(flow_control) = self.kiss_flow_control {
                    settings.insert("kiss_flow_control".to_string(), JsonValue::Bool(flow_control));
                }
                insert_opt_string(&mut settings, "id_callsign", self.id_callsign.as_ref());
                insert_opt_u64(&mut settings, "id_interval", self.id_interval);
                insert_opt_u64(&mut settings, "reconnect_backoff_ms", self.reconnect_backoff_ms);
                insert_opt_u64(
                    &mut settings,
                    "max_reconnect_backoff_ms",
                    self.max_reconnect_backoff_ms,
                );
            }
            "ax25_kiss" => {
                insert_opt_string(&mut settings, "device", self.device.as_ref());
                insert_opt_u64(&mut settings, "baud_rate", self.baud_rate.map(u64::from));
                insert_opt_u64(&mut settings, "mtu", self.mtu.map(|v| v as u64));
                insert_opt_u64(&mut settings, "preamble_ms", self.preamble_ms.map(u64::from));
                insert_opt_u64(&mut settings, "tx_tail_ms", self.tx_tail_ms.map(u64::from));
                insert_opt_u64(&mut settings, "persistence", self.persistence.map(u64::from));
                insert_opt_u64(&mut settings, "slot_time_ms", self.slot_time_ms.map(u64::from));
                if let Some(flow_control) = self.kiss_flow_control {
                    settings.insert("kiss_flow_control".to_string(), JsonValue::Bool(flow_control));
                }
                insert_opt_string(&mut settings, "callsign", self.callsign.as_ref());
                insert_opt_u64(&mut settings, "ssid", self.ssid.map(u64::from));
                insert_opt_string(&mut settings, "id_callsign", self.id_callsign.as_ref());
                insert_opt_u64(&mut settings, "id_interval", self.id_interval);
                insert_opt_u64(&mut settings, "reconnect_backoff_ms", self.reconnect_backoff_ms);
                insert_opt_u64(
                    &mut settings,
                    "max_reconnect_backoff_ms",
                    self.max_reconnect_backoff_ms,
                );
            }
            "kiss_tcp_client" => {
                insert_opt_string(&mut settings, "host", self.host.as_ref());
                insert_opt_u64(&mut settings, "port", self.port.map(u64::from));
                insert_opt_u64(&mut settings, "mtu", self.mtu.map(|v| v as u64));
                insert_opt_u64(&mut settings, "preamble_ms", self.preamble_ms.map(u64::from));
                insert_opt_u64(&mut settings, "tx_tail_ms", self.tx_tail_ms.map(u64::from));
                insert_opt_u64(&mut settings, "persistence", self.persistence.map(u64::from));
                insert_opt_u64(&mut settings, "slot_time_ms", self.slot_time_ms.map(u64::from));
                if let Some(flow_control) = self.kiss_flow_control {
                    settings.insert("kiss_flow_control".to_string(), JsonValue::Bool(flow_control));
                }
                insert_opt_string(&mut settings, "id_callsign", self.id_callsign.as_ref());
                insert_opt_u64(&mut settings, "id_interval", self.id_interval);
                insert_opt_u64(&mut settings, "reconnect_backoff_ms", self.reconnect_backoff_ms);
                insert_opt_u64(
                    &mut settings,
                    "max_reconnect_backoff_ms",
                    self.max_reconnect_backoff_ms,
                );
            }
            "ble_gatt" => {
                insert_opt_string(&mut settings, "adapter", self.adapter.as_ref());
                insert_opt_string(&mut settings, "peripheral_id", self.peripheral_id.as_ref());
                insert_opt_string(&mut settings, "service_uuid", self.service_uuid.as_ref());
                insert_opt_string(&mut settings, "write_char_uuid", self.write_char_uuid.as_ref());
                insert_opt_string(
                    &mut settings,
                    "notify_char_uuid",
                    self.notify_char_uuid.as_ref(),
                );
                insert_opt_u64(&mut settings, "scan_timeout_ms", self.scan_timeout_ms);
                insert_opt_u64(
                    &mut settings,
                    "ble_connect_timeout_ms",
                    self.ble_connect_timeout_ms,
                );
                insert_opt_u64(&mut settings, "connect_timeout_ms", self.connect_timeout_ms);
                insert_opt_u64(&mut settings, "mtu", self.mtu.map(|v| v as u64));
                insert_opt_u64(&mut settings, "reconnect_backoff_ms", self.reconnect_backoff_ms);
                insert_opt_u64(
                    &mut settings,
                    "max_reconnect_backoff_ms",
                    self.max_reconnect_backoff_ms,
                );
            }
            "reticulum_ble" => {
                insert_opt_string(&mut settings, "adapter", self.adapter.as_ref());
                insert_opt_string(&mut settings, "service_uuid", self.service_uuid.as_ref());
                insert_opt_string(&mut settings, "tx_char_uuid", self.notify_char_uuid.as_ref());
                insert_opt_string(&mut settings, "rx_char_uuid", self.write_char_uuid.as_ref());
                insert_opt_string(
                    &mut settings,
                    "identity_char_uuid",
                    self.identity_char_uuid.as_ref(),
                );
                insert_opt_u64(&mut settings, "mtu", self.mtu.map(|v| v as u64));
                insert_opt_u64(&mut settings, "max_connections", self.max_connections.map(|v| v as u64));
                insert_opt_u64(&mut settings, "scan_duration_ms", self.scan_duration_ms);
                insert_opt_u64(&mut settings, "discovery_interval_ms", self.discovery_interval_ms);
                insert_opt_u64(
                    &mut settings,
                    "discovery_interval_idle_ms",
                    self.discovery_interval_idle_ms,
                );
                insert_opt_u64(
                    &mut settings,
                    "advertising_refresh_interval_ms",
                    self.advertising_refresh_interval_ms,
                );
                insert_opt_i64(&mut settings, "min_rssi_dbm", self.min_rssi_dbm.map(i64::from));
                insert_opt_bool(&mut settings, "enable_central", self.enable_central);
                insert_opt_bool(&mut settings, "enable_peripheral", self.enable_peripheral);
            }
            "vrn76_kiss_ble" => {
                insert_opt_string(&mut settings, "adapter", self.adapter.as_ref());
                insert_opt_string(&mut settings, "peripheral_id", self.peripheral_id.as_ref());
                insert_opt_string(&mut settings, "frame_mode", self.frame_mode.as_ref());
                insert_opt_u64(&mut settings, "mtu", self.mtu.map(|v| v as u64));
                insert_opt_u64(
                    &mut settings,
                    "max_write_len",
                    self.max_write_len.map(|v| v as u64),
                );
                insert_opt_u64(&mut settings, "preamble_ms", self.preamble_ms.map(u64::from));
                insert_opt_u64(&mut settings, "tx_tail_ms", self.tx_tail_ms.map(u64::from));
                insert_opt_u64(&mut settings, "persistence", self.persistence.map(u64::from));
                insert_opt_u64(&mut settings, "slot_time_ms", self.slot_time_ms.map(u64::from));
                if let Some(flow_control) = self.kiss_flow_control {
                    settings.insert("kiss_flow_control".to_string(), JsonValue::Bool(flow_control));
                }
                insert_opt_string(&mut settings, "id_callsign", self.id_callsign.as_ref());
                insert_opt_u64(&mut settings, "id_interval", self.id_interval);
                insert_opt_u64(&mut settings, "scan_timeout_ms", self.scan_timeout_ms);
                insert_opt_u64(&mut settings, "connect_timeout_ms", self.connect_timeout_ms);
                insert_opt_u64(&mut settings, "reconnect_backoff_ms", self.reconnect_backoff_ms);
                insert_opt_u64(
                    &mut settings,
                    "max_reconnect_backoff_ms",
                    self.max_reconnect_backoff_ms,
                );
            }
            "lora" => {
                insert_opt_string(&mut settings, "adapter", self.adapter.as_ref());
                insert_opt_string(&mut settings, "device", self.device.as_ref());
                insert_opt_u64(&mut settings, "baud_rate", self.baud_rate.map(u64::from));
                insert_opt_u64(&mut settings, "mtu", self.mtu.map(|v| v as u64));
                insert_opt_u64(
                    &mut settings,
                    "max_write_len",
                    self.max_write_len.map(|v| v as u64),
                );
                insert_opt_string(&mut settings, "region", self.region.as_ref());
                insert_opt_u64(&mut settings, "frequency_hz", self.frequency_hz);
                insert_opt_u64(&mut settings, "bandwidth_hz", self.bandwidth_hz.map(u64::from));
                insert_opt_u64(
                    &mut settings,
                    "spreading_factor",
                    self.spreading_factor.map(u64::from),
                );
                insert_opt_string(&mut settings, "coding_rate", self.coding_rate.as_ref());
                if let Some(tx_power_dbm) = self.tx_power_dbm {
                    settings
                        .insert("tx_power_dbm".to_string(), JsonValue::Number(tx_power_dbm.into()));
                }
                if let Some(flow_control) =
                    self.flow_control.as_ref().and_then(toml::Value::as_bool)
                {
                    settings.insert("flow_control".to_string(), JsonValue::Bool(flow_control));
                }
                insert_opt_u64(&mut settings, "scan_timeout_ms", self.scan_timeout_ms);
                insert_opt_u64(
                    &mut settings,
                    "ble_connect_timeout_ms",
                    self.ble_connect_timeout_ms,
                );
                insert_opt_u64(&mut settings, "connect_timeout_ms", self.connect_timeout_ms);
                insert_opt_string(&mut settings, "id_callsign", self.id_callsign.as_ref());
                insert_opt_u64(&mut settings, "id_interval", self.id_interval);
                insert_opt_u64(&mut settings, "reconnect_backoff_ms", self.reconnect_backoff_ms);
                insert_opt_u64(
                    &mut settings,
                    "max_reconnect_backoff_ms",
                    self.max_reconnect_backoff_ms,
                );
                insert_opt_f64(&mut settings, "airtime_limit_short", self.airtime_limit_short);
                insert_opt_f64(&mut settings, "airtime_limit_long", self.airtime_limit_long);
                insert_opt_u64(&mut settings, "sync_word", self.sync_word.map(u64::from));
                insert_opt_u64(
                    &mut settings,
                    "preamble_symbols",
                    self.preamble_symbols.map(u64::from),
                );
                insert_opt_u64(
                    &mut settings,
                    "max_payload_bytes",
                    self.max_payload_bytes.map(u64::from),
                );
                insert_opt_string(&mut settings, "state_path", self.state_path.as_ref());
            }
            "rnode_multi" => {
                insert_opt_string(&mut settings, "device", self.device.as_ref());
                insert_opt_u64(&mut settings, "baud_rate", self.baud_rate.map(u64::from));
                insert_opt_u64(&mut settings, "mtu", self.mtu.map(|value| value as u64));
                insert_opt_string(&mut settings, "id_callsign", self.id_callsign.as_ref());
                insert_opt_u64(&mut settings, "id_interval", self.id_interval);
                if let Some(subinterfaces) = rnode_multi_subinterfaces_settings_json(self) {
                    settings.insert("subinterfaces".to_string(), subinterfaces);
                }
            }
            _ => {}
        }
        (!settings.is_empty()).then_some(JsonValue::Object(settings))
    }

    fn validate(&self, index: usize, original_kind: &str) -> Result<(), String> {
        let kind = self.kind.trim();
        if kind.is_empty() {
            return Err(format!("interfaces[{index}].type is required"));
        }
        self.interface_mode().map_err(|err| format!("interfaces[{index}].{err}"))?;
        self.validate_announce_pacing(index)?;
        match kind {
            "tcp_client" => self.validate_tcp_client(index),
            "tcp_server" => self.validate_tcp_server(index),
            "udp" => self.validate_udp(index),
            "auto" => self.validate_auto(index),
            "serial" => self.validate_serial(index),
            "weave" => self.validate_weave(index),
            "kiss" => self.validate_kiss(index),
            "ax25_kiss" => self.validate_ax25_kiss(index),
            "kiss_tcp_client" => self.validate_kiss_tcp_client(index),
            "backbone" => self.validate_backbone(index),
            "backbone_client" => self.validate_backbone_client(index),
            "local" | "local_client" => self.validate_local(index),
            "pipe" => self.validate_pipe(index),
            "i2p" => self.validate_i2p(index),
            "ble_gatt" => self.validate_ble(index),
            "reticulum_ble" => self.validate_reticulum_ble(index),
            "vrn76_kiss_ble" => self.validate_vrn76_kiss_ble(index),
            "lora" => self.validate_lora(index, original_kind),
            "rnode_multi" => self.validate_rnode_multi(index),
            _ => Ok(()),
        }
    }

    pub fn interface_mode(&self) -> Result<rns_transport::iface::InterfaceMode, String> {
        let mode = self.configured_interface_mode()?;
        Ok(self.discoverable_mode_override(mode).unwrap_or(mode))
    }

    fn configured_interface_mode(&self) -> Result<rns_transport::iface::InterfaceMode, String> {
        let Some((field, value)) = self.interface_mode_raw() else {
            return Ok(rns_transport::iface::InterfaceMode::Full);
        };
        rns_transport::iface::InterfaceMode::parse(value)
            .ok()
            .flatten()
            .ok_or_else(|| {
                format!(
                    "{field} must be one of full, access_point, accesspoint, ap, pointtopoint, ptp, roaming, boundary, gateway, gw"
                )
            })
    }

    fn interface_mode_raw(&self) -> Option<(&'static str, &str)> {
        self.interface_mode
            .as_deref()
            .map(|value| ("interface_mode", value))
            .or_else(|| self.mode.as_deref().map(|value| ("mode", value)))
    }

    fn discoverable_mode_override(
        &self,
        configured: rns_transport::iface::InterfaceMode,
    ) -> Option<rns_transport::iface::InterfaceMode> {
        if self.discoverable != Some(true) || self.ignore_config_warnings == Some(true) {
            return None;
        }
        if matches!(
            configured,
            rns_transport::iface::InterfaceMode::Gateway
                | rns_transport::iface::InterfaceMode::AccessPoint
        ) {
            return None;
        }
        if matches!(self.kind.as_str(), "lora" | "rnode_multi") {
            Some(rns_transport::iface::InterfaceMode::AccessPoint)
        } else {
            Some(rns_transport::iface::InterfaceMode::Gateway)
        }
    }

    pub fn discovery_announce_interval_secs(&self) -> Option<u64> {
        if self.discoverable != Some(true) {
            return self.announce_interval;
        }
        let interval = self.announce_interval.map(|minutes| minutes.saturating_mul(60));
        Some(interval.unwrap_or(6 * 60 * 60).max(5 * 60))
    }

    pub fn flow_control_name(&self) -> Option<&str> {
        self.flow_control.as_ref().and_then(toml::Value::as_str)
    }

    pub fn auto_discovery_multicast_address(&self) -> Option<String> {
        if self.kind != "auto" {
            return None;
        }
        let scope = rns_transport::iface::auto::AutoDiscoveryScope::parse(
            self.discovery_scope.as_deref()?,
        )
        .ok()
        .flatten()?;
        let address_type = rns_transport::iface::auto::MulticastAddressType::parse(
            self.multicast_address_type.as_deref()?,
        )
        .ok()
        .flatten()?;
        Some(rns_transport::iface::auto::multicast_discovery_address(
            self.group_id.as_deref()?.as_bytes(),
            scope,
            address_type,
        ))
    }

    fn validate_announce_pacing(&self, index: usize) -> Result<(), String> {
        if self.bitrate == Some(0) {
            return Err(format!("interfaces[{index}].bitrate must be > 0"));
        }
        if self.announce_rate_target == Some(0) {
            return Err(format!("interfaces[{index}].announce_rate_target must be > 0"));
        }
        if let Some(announce_cap) = self.announce_cap {
            if !(1..=100).contains(&announce_cap) {
                return Err(format!("interfaces[{index}].announce_cap must be between 1 and 100"));
            }
        }
        self.validate_finite_shared_float(index, "ic_burst_hold", self.ic_burst_hold)?;
        self.validate_finite_shared_float(index, "ic_burst_freq_new", self.ic_burst_freq_new)?;
        self.validate_finite_shared_float(index, "ic_burst_freq", self.ic_burst_freq)?;
        self.validate_finite_shared_float(
            index,
            "ic_pr_burst_freq_new",
            self.ic_pr_burst_freq_new,
        )?;
        self.validate_finite_shared_float(index, "ic_pr_burst_freq", self.ic_pr_burst_freq)?;
        self.validate_finite_shared_float(index, "ec_pr_freq", self.ec_pr_freq)?;
        self.validate_finite_shared_float(index, "ic_new_time", self.ic_new_time)?;
        self.validate_finite_shared_float(index, "ic_burst_penalty", self.ic_burst_penalty)?;
        self.validate_finite_shared_float(
            index,
            "ic_held_release_interval",
            self.ic_held_release_interval,
        )?;
        self.validate_finite_shared_float(index, "latitude", self.latitude)?;
        self.validate_finite_shared_float(index, "longitude", self.longitude)?;
        self.validate_finite_shared_float(index, "height", self.height)?;
        Ok(())
    }

    fn validate_finite_shared_float(
        &self,
        index: usize,
        field: &str,
        value: Option<f64>,
    ) -> Result<(), String> {
        if value.is_some_and(|value| !value.is_finite()) {
            return Err(format!("interfaces[{index}].{field} must be finite"));
        }
        Ok(())
    }

    pub fn ifac_network_name(&self) -> Option<&String> {
        self.network_name
            .as_ref()
            .filter(|value| !value.trim().is_empty())
            .or_else(|| self.networkname.as_ref().filter(|value| !value.trim().is_empty()))
    }

    pub fn ifac_passphrase(&self) -> Option<&String> {
        self.passphrase
            .as_ref()
            .filter(|value| !value.trim().is_empty())
            .or_else(|| self.pass_phrase.as_ref().filter(|value| !value.trim().is_empty()))
    }

    fn normalize_aliases(&mut self, index: usize, original_kind: &str) -> Result<(), String> {
        if self.kind == "udp" {
            self.normalize_udp_aliases(index)?;
        } else {
            self.normalize_port_alias(index)?;
        }
        if self.kind == "tcp_client" {
            self.normalize_tcp_client_aliases(index)?;
        }
        if self.kind == "tcp_server" {
            self.normalize_tcp_server_aliases(index)?;
        }
        if self.kind == "backbone" || self.kind == "backbone_client" {
            self.normalize_backbone_aliases(index, original_kind)?;
        }
        if self.kind == "local" || self.kind == "local_client" {
            self.normalize_local_aliases(index)?;
        }
        if self.kind == "pipe" {
            self.normalize_pipe_aliases(index);
        }
        if self.kind == "i2p" {
            self.normalize_i2p_aliases(index)?;
        }
        if self.kind == "auto" {
            self.normalize_auto_aliases(index)?;
        }
        if self.kind == "serial" {
            self.normalize_serial_aliases(index, original_kind)?;
        }
        if self.kind == "weave" {
            self.normalize_weave_aliases(index)?;
        }
        if self.kind == "vrn76_kiss_ble" {
            self.normalize_vrn76_kiss_ble_aliases(index)?;
        }
        if self.kind == "reticulum_ble" {
            self.normalize_reticulum_ble_aliases(index)?;
        }
        if self.kind == "kiss" {
            self.normalize_kiss_aliases(index, original_kind)?;
        }
        if self.kind == "kiss_tcp_client" {
            self.normalize_kiss_tcp_client_aliases(index);
        }
        if self.kind == "ax25_kiss" {
            self.normalize_ax25_kiss_aliases(index)?;
        }
        if self.kind == "lora" {
            self.normalize_lora_aliases(index, original_kind)?;
        }
        if self.kind == "rnode_multi" {
            self.normalize_rnode_multi_aliases(index)?;
        }
        Ok(())
    }

    fn normalize_port_alias(&mut self, index: usize) -> Result<(), String> {
        let Some(value) = self.extra.remove("port") else {
            return Ok(());
        };
        match self.kind.as_str() {
            "tcp_client"
            | "tcp_server"
            | "udp"
            | "kiss_tcp_client"
            | "backbone"
            | "backbone_client"
            | "local"
            | "local_client" => {
                if self.port.is_none() {
                    self.port = Some(port_number_from_value(value, index)?);
                }
            }
            "serial" | "weave" | "kiss" | "ax25_kiss" | "lora" | "rnode_multi" => {
                if self.device.is_none() {
                    self.device =
                        Some(string_from_value(value, "port", index, self.kind.as_str())?);
                }
            }
            _ => {
                self.extra.insert("port".to_string(), value);
            }
        }
        Ok(())
    }

    fn normalize_tcp_client_aliases(&mut self, index: usize) -> Result<(), String> {
        if self.host.is_none() {
            self.host = self.target_host.clone().and_then(non_empty_string);
        }
        if self.port.is_none() {
            self.port = self.target_port;
        }
        if let Some(fixed_mtu) = self.take_tcp_fixed_mtu_alias(index)? {
            if self.mtu.is_none() {
                self.mtu = Some(fixed_mtu);
            }
        }
        if self.take_bool_alias_for_kind("kiss_framing", index, "tcp_client")?.unwrap_or(false) {
            self.kind = "kiss_tcp_client".to_string();
        }
        Ok(())
    }

    fn normalize_reticulum_ble_aliases(&mut self, index: usize) -> Result<(), String> {
        const SERVICE_UUID: &str = "37145b00-442d-4a94-917f-8f42c5da28e3";
        const TX_UUID: &str = "37145b00-442d-4a94-917f-8f42c5da28e4";
        const RX_UUID: &str = "37145b00-442d-4a94-917f-8f42c5da28e5";
        const IDENTITY_UUID: &str = "37145b00-442d-4a94-917f-8f42c5da28e6";

        if self.service_uuid.is_none() {
            self.service_uuid = Some(SERVICE_UUID.to_string());
        }
        if self.notify_char_uuid.is_none() {
            self.notify_char_uuid = self
                .take_string_alias_for_kind("tx_char_uuid", index, "reticulum_ble")?
                .and_then(non_empty_string)
                .or_else(|| Some(TX_UUID.to_string()));
        } else {
            let _ = self.take_string_alias_for_kind("tx_char_uuid", index, "reticulum_ble")?;
        }
        if self.write_char_uuid.is_none() {
            self.write_char_uuid = self
                .take_string_alias_for_kind("rx_char_uuid", index, "reticulum_ble")?
                .and_then(non_empty_string)
                .or_else(|| Some(RX_UUID.to_string()));
        } else {
            let _ = self.take_string_alias_for_kind("rx_char_uuid", index, "reticulum_ble")?;
        }
        if self.identity_char_uuid.is_none() {
            self.identity_char_uuid = Some(IDENTITY_UUID.to_string());
        }
        if self.mtu.is_none() {
            self.mtu = Some(185);
        }
        if self.max_connections.is_none() {
            self.max_connections = Some(7);
        }
        if self.min_rssi_dbm.is_none() {
            self.min_rssi_dbm = Some(-85);
        }
        if self.scan_duration_ms.is_none() {
            self.scan_duration_ms = Some(10_000);
        }
        if self.discovery_interval_ms.is_none() {
            self.discovery_interval_ms = Some(5_000);
        }
        if self.discovery_interval_idle_ms.is_none() {
            self.discovery_interval_idle_ms = Some(30_000);
        }
        if self.advertising_refresh_interval_ms.is_none() {
            self.advertising_refresh_interval_ms = Some(30_000);
        }
        if self.enable_central.is_none() {
            self.enable_central = Some(true);
        }
        if self.enable_peripheral.is_none() {
            self.enable_peripheral = Some(true);
        }
        Ok(())
    }

    fn take_tcp_fixed_mtu_alias(&mut self, index: usize) -> Result<Option<usize>, String> {
        let Some(value) = self.take_u64_alias_for_kind("fixed_mtu", index, "tcp_client")? else {
            return Ok(None);
        };
        if value == 0 {
            return Ok(None);
        }
        let mtu = usize::try_from(value).map_err(|_| {
            format!("interfaces[{index}].fixed_mtu must fit in usize for tcp_client")
        })?;
        if mtu < RETICULUM_CONFIG_MTU {
            return Err(format!(
                "interfaces[{index}].fixed_mtu must be 0 or at least {RETICULUM_CONFIG_MTU} for tcp_client"
            ));
        }
        Ok(Some(mtu))
    }

    fn normalize_tcp_server_aliases(&mut self, index: usize) -> Result<(), String> {
        if self.host.is_none() {
            self.host = self.take_string_alias_for_kind("listen_ip", index, "tcp_server")?;
        } else {
            let _ = self.take_string_alias_for_kind("listen_ip", index, "tcp_server")?;
        }
        if self.port.is_none() {
            self.port = self.take_u16_alias_for_kind("listen_port", index, "tcp_server")?;
        } else {
            let _ = self.take_u16_alias_for_kind("listen_port", index, "tcp_server")?;
        }
        Ok(())
    }

    fn validate_tcp_client(&self, index: usize) -> Result<(), String> {
        if !self.enabled() {
            return Ok(());
        }
        require_non_empty(
            self.host.as_deref(),
            &format!("interfaces[{index}].host or target_host is required for tcp_client"),
        )?;
        if self.port.is_none() {
            return Err(format!("interfaces[{index}].port or target_port is required for tcp_client"));
        }
        if self.port == Some(0) {
            return Err(format!("interfaces[{index}].port must be > 0 for tcp_client"));
        }
        if let Some(mtu) = self.mtu {
            if mtu == 0 {
                return Err(format!("interfaces[{index}].mtu must be > 0 for tcp_client"));
            }
        }
        if self.connect_timeout == Some(0) {
            return Err(format!("interfaces[{index}].connect_timeout must be > 0 for tcp_client"));
        }
        Ok(())
    }

    fn validate_tcp_server(&self, index: usize) -> Result<(), String> {
        if !self.enabled() {
            return Ok(());
        }
        if let Some(host) = self.host.as_deref() {
            require_non_empty(
                Some(host),
                &format!("interfaces[{index}].host or listen_ip cannot be empty for tcp_server"),
            )?;
        }
        if let Some(device) = self.device.as_deref() {
            require_non_empty(
                Some(device),
                &format!("interfaces[{index}].device cannot be empty for tcp_server"),
            )?;
        }
        if let Some(mtu) = self.mtu {
            if mtu == 0 {
                return Err(format!("interfaces[{index}].mtu must be > 0 for tcp_server"));
            }
        }
        Ok(())
    }

    fn normalize_backbone_aliases(
        &mut self,
        index: usize,
        _original_kind: &str,
    ) -> Result<(), String> {
        if self.target_host.is_none() {
            self.target_host = self
                .take_string_alias_for_kind("remote", index, "backbone")?
                .and_then(non_empty_string);
        } else {
            let _ = self.take_string_alias_for_kind("remote", index, "backbone")?;
        }

        if self.host.is_none() {
            let listen_on = self
                .take_string_alias_for_kind("listen_on", index, "backbone")?
                .and_then(non_empty_string);
            let listen_ip = self
                .take_string_alias_for_kind("listen_ip", index, "backbone")?
                .and_then(non_empty_string);
            self.host = listen_on.or(listen_ip);
        } else {
            let _ = self.take_string_alias_for_kind("listen_on", index, "backbone")?;
            let _ = self.take_string_alias_for_kind("listen_ip", index, "backbone")?;
        }

        if self.port.is_none() {
            self.port = self.take_u16_alias_for_kind("listen_port", index, "backbone")?;
        } else {
            let _ = self.take_u16_alias_for_kind("listen_port", index, "backbone")?;
        }
        if self.port.is_none() {
            self.port = self.target_port;
        }
        if self.target_port.is_none() {
            self.target_port = self.port;
        }

        if self.mtu.is_none() {
            self.mtu = Some(1_048_576);
        }

        if self.kind == "backbone"
            && self.target_host.as_deref().map(str::trim).is_some_and(|value| !value.is_empty())
        {
            self.kind = "backbone_client".to_string();
        }
        if self.kind == "backbone_client" {
            if self.host.is_none() {
                self.host = self.target_host.clone().and_then(non_empty_string);
            }
            if self.port.is_none() {
                self.port = self.target_port;
            }
        }

        Ok(())
    }

    fn validate_backbone(&self, index: usize) -> Result<(), String> {
        if !self.enabled() {
            return Ok(());
        }
        if let Some(host) = self.host.as_deref() {
            require_non_empty(
                Some(host),
                &format!("interfaces[{index}].listen_ip or listen_on cannot be empty for backbone"),
            )?;
        }
        if let Some(device) = self.device.as_deref() {
            require_non_empty(
                Some(device),
                &format!("interfaces[{index}].device cannot be empty for backbone"),
            )?;
        }
        if self.host.as_deref().map(str::trim).filter(|value| !value.is_empty()).is_none()
            && self.device.as_deref().map(str::trim).filter(|value| !value.is_empty()).is_none()
        {
            return Err(format!(
                "interfaces[{index}].listen_ip, listen_on, or device is required for backbone"
            ));
        }
        if self.port.is_none() {
            return Err(format!("interfaces[{index}].port is required for backbone"));
        }
        Ok(())
    }

    fn validate_backbone_client(&self, index: usize) -> Result<(), String> {
        if !self.enabled() {
            return Ok(());
        }
        require_non_empty(
            self.target_host.as_deref().or(self.host.as_deref()),
            &format!("interfaces[{index}].target_host or remote is required for backbone_client"),
        )?;
        if self.target_port.or(self.port).is_none() {
            return Err(format!("interfaces[{index}].target_port or port is required for backbone_client"));
        }
        if self.connect_timeout == Some(0) {
            return Err(format!(
                "interfaces[{index}].connect_timeout must be > 0 for backbone_client"
            ));
        }
        Ok(())
    }

    fn normalize_local_aliases(&mut self, index: usize) -> Result<(), String> {
        if self.socket_path.is_none() {
            self.socket_path = self
                .take_string_alias_for_kind("unix_socket_path", index, "local")?
                .and_then(non_empty_string);
        } else {
            let _ = self.take_string_alias_for_kind("unix_socket_path", index, "local")?;
        }
        if self.instance_name.is_none() {
            self.instance_name = self
                .take_string_alias_for_kind("instance_name", index, "local")?
                .and_then(non_empty_string);
        } else {
            let _ = self.take_string_alias_for_kind("instance_name", index, "local")?;
        }

        let shared_instance_type = self
            .shared_instance_type
            .clone()
            .and_then(non_empty_string)
            .or_else(|| {
                self.extra
                    .remove("shared_instance_type")
                    .and_then(|value| value.as_str().map(ToOwned::to_owned))
                    .and_then(non_empty_string)
            });
        let shared_instance_type = shared_instance_type
            .map(|value| value.trim().to_ascii_lowercase())
            .unwrap_or_else(|| "tcp".to_string());
        if !matches!(shared_instance_type.as_str(), "tcp" | "unix") {
            return Err(format!(
                "interfaces[{index}].shared_instance_type must be one of tcp, unix for local"
            ));
        }
        self.shared_instance_type = Some(shared_instance_type.clone());

        if shared_instance_type == "unix" {
            if self.socket_path.is_none() {
                let instance_name = self.instance_name.as_deref().unwrap_or("default");
                self.socket_path = Some(default_local_unix_socket_value(instance_name));
            }
            self.host = None;
            self.port = None;
        } else {
            if self.host.is_none() {
                let listen_ip = self
                    .take_string_alias_for_kind("listen_ip", index, "local")?
                    .and_then(non_empty_string);
                let bind_ip = self
                    .take_string_alias_for_kind("bind_ip", index, "local")?
                    .and_then(non_empty_string);
                self.host = listen_ip.or(bind_ip).or_else(|| Some("127.0.0.1".to_string()));
            } else {
                let _ = self.take_string_alias_for_kind("listen_ip", index, "local")?;
                let _ = self.take_string_alias_for_kind("bind_ip", index, "local")?;
            }

            if self.port.is_none() {
                self.port = self.take_u16_alias_for_kind("listen_port", index, "local")?;
            } else {
                let _ = self.take_u16_alias_for_kind("listen_port", index, "local")?;
            }
            if self.port.is_none() {
                self.port =
                    self.take_u16_alias_for_kind("shared_instance_port", index, "local")?;
            } else {
                let _ = self.take_u16_alias_for_kind("shared_instance_port", index, "local")?;
            }
            if self.port.is_none() {
                self.port = Some(37_428);
            }
        }

        if self.mtu.is_none() {
            self.mtu = self
                .take_u64_alias_for_kind("fixed_mtu", index, "local")?
                .map(|value| {
                    usize::try_from(value).map_err(|_| {
                        format!("interfaces[{index}].fixed_mtu must fit in usize for local")
                    })
                })
                .transpose()?;
        } else {
            let _ = self.take_u64_alias_for_kind("fixed_mtu", index, "local")?;
        }
        if self.mtu.is_none() {
            self.mtu = Some(rns_transport::iface::tcp_client::TcpClient::DEFAULT_MTU);
        }

        let forced_bitrate =
            self.take_u64_alias_for_kind("force_shared_instance_bitrate", index, "local")?;
        if self.force_shared_instance_bitrate.is_none() {
            self.force_shared_instance_bitrate = forced_bitrate;
        }
        if self.bitrate.is_none() {
            self.bitrate = self.force_shared_instance_bitrate;
        }
        if self.bitrate.is_none() {
            self.bitrate = Some(1_000_000_000);
        }

        Ok(())
    }

    fn validate_local(&self, index: usize) -> Result<(), String> {
        self.reject_unknown_new_kind_keys(index, "local")?;
        if !self.enabled() {
            return Ok(());
        }
        let host = self.host.as_deref().unwrap_or_default().trim();
        if self.shared_instance_type.as_deref() == Some("unix") {
            require_non_empty(
                self.socket_path.as_deref(),
                &format!("interfaces[{index}].socket_path is required for local unix"),
            )?;
            return Ok(());
        }
        if host.is_empty() {
            return Err(format!("interfaces[{index}].host is required for local"));
        }
        if !matches!(host, "127.0.0.1" | "::1" | "localhost") {
            return Err(format!(
                "interfaces[{index}].host must be loopback for local"
            ));
        }
        if self.port.is_none() {
            return Err(format!("interfaces[{index}].port is required for local"));
        }
        if let Some(mtu) = self.mtu {
            if !(256..=1_048_576).contains(&mtu) {
                return Err(format!(
                    "interfaces[{index}].mtu must be between 256 and 1048576 for local"
                ));
            }
        }
        if self.connect_timeout == Some(0) {
            return Err(format!("interfaces[{index}].connect_timeout must be > 0 for local"));
        }
        Ok(())
    }

    fn normalize_pipe_aliases(&mut self, _index: usize) {
        if self.mtu.is_none() {
            self.mtu = Some(1_064);
        }
        if self.respawn_delay.is_none() {
            self.respawn_delay = Some(5.0);
        }
    }

    fn validate_pipe(&self, index: usize) -> Result<(), String> {
        if !self.enabled() {
            return Ok(());
        }
        require_non_empty(
            self.command.as_deref(),
            &format!("interfaces[{index}].command is required for pipe"),
        )?;
        if self.respawn_delay.is_some_and(|value| value < 0.0 || !value.is_finite()) {
            return Err(format!("interfaces[{index}].respawn_delay must be finite and >= 0"));
        }
        Ok(())
    }

    fn validate_i2p(&self, index: usize) -> Result<(), String> {
        self.reject_unknown_new_kind_keys(index, "i2p")?;
        if !self.enabled() {
            return Ok(());
        }
        require_non_empty(
            self.sam_host.as_deref(),
            &format!("interfaces[{index}].sam_host is required for i2p"),
        )?;
        if self.sam_port.is_none() {
            return Err(format!("interfaces[{index}].sam_port is required for i2p"));
        }
        if let Some(peers) = self.peers.as_ref() {
            if peers.iter().any(|peer| peer.trim().is_empty()) {
                return Err(format!("interfaces[{index}].peers entries must be non-empty for i2p"));
            }
        }
        if let Some(mtu) = self.mtu {
            if !(256..=262_144).contains(&mtu) {
                return Err(format!(
                    "interfaces[{index}].mtu must be between 256 and 262144 for i2p"
                ));
            }
        }
        Ok(())
    }

    fn normalize_udp_aliases(&mut self, index: usize) -> Result<(), String> {
        let shared_port = self.take_u16_alias_for_kind("port", index, "udp")?;
        if self.host.is_none() {
            self.host = self.take_string_alias_for_kind("listen_ip", index, "udp")?;
        } else {
            let _ = self.take_string_alias_for_kind("listen_ip", index, "udp")?;
        }
        if self.port.is_none() {
            self.port =
                self.take_u16_alias_for_kind("listen_port", index, "udp")?.or(shared_port);
        } else {
            let _ = self.take_u16_alias_for_kind("listen_port", index, "udp")?;
        }
        let forward_ip = if self.target_host.is_none() {
            self.take_string_alias_for_kind("forward_ip", index, "udp")?
                .and_then(non_empty_string)
        } else {
            let _ = self.take_string_alias_for_kind("forward_ip", index, "udp")?;
            None
        };
        let forward_port = self.take_u16_alias_for_kind("forward_port", index, "udp")?;
        let target_port = forward_port.or(shared_port);
        if self.target_host.is_none() && target_port.is_some() {
            self.target_host = forward_ip;
        }
        if self.target_port.is_none() {
            self.target_port = if self.target_host.is_some() { target_port } else { None };
        }
        Ok(())
    }
}

#[cfg(any(target_os = "linux", target_os = "android"))]
fn default_local_unix_socket_value(instance_name: &str) -> String {
    format!("@rns/{instance_name}")
}

#[cfg(not(any(target_os = "linux", target_os = "android")))]
fn default_local_unix_socket_value(instance_name: &str) -> String {
    std::env::temp_dir()
        .join(format!("rns-{instance_name}.sock"))
        .to_string_lossy()
        .to_string()
}
