impl InterfaceConfig {

    pub fn enabled(&self) -> bool {
        self.enabled.unwrap_or(false)
    }

    pub fn outgoing(&self) -> bool {
        self.outgoing.unwrap_or(true)
    }

    pub fn settings_json(&self) -> Option<JsonValue> {
        let mut settings = JsonMap::new();
        if self.interface_mode_raw().is_some() {
            if let Ok(mode) = self.interface_mode() {
                settings
                    .insert("interface_mode".to_string(), JsonValue::String(mode.as_str().into()));
            }
        }
        insert_opt_bool(&mut settings, "outgoing", self.outgoing);
        insert_opt_u64(&mut settings, "bitrate", self.bitrate);
        insert_opt_u64(&mut settings, "announce_cap", self.announce_cap);
        match self.kind.as_str() {
            "tcp_client" => {
                insert_opt_string(&mut settings, "host", self.host.as_ref());
                insert_opt_u64(&mut settings, "port", self.port.map(u64::from));
                insert_opt_u64(&mut settings, "mtu", self.mtu.map(|v| v as u64));
            }
            "udp" => {
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
            "udp" => self.validate_udp(index),
            "auto" => self.validate_auto(index),
            "serial" => self.validate_serial(index),
            "kiss" => self.validate_kiss(index),
            "kiss_tcp_client" => self.validate_kiss_tcp_client(index),
            "ble_gatt" => self.validate_ble(index),
            "vrn76_kiss_ble" => self.validate_vrn76_kiss_ble(index),
            "lora" => self.validate_lora(index, original_kind),
            _ if is_known_unsupported_python_interface(original_kind) => Err(format!(
                "interfaces[{index}].type {original_kind} is a known unsupported Reticulum interface family"
            )),
            _ => Ok(()),
        }
    }

    pub fn interface_mode(&self) -> Result<rns_transport::iface::InterfaceMode, String> {
        let Some((field, value)) = self.interface_mode_raw() else {
            return Ok(rns_transport::iface::InterfaceMode::Full);
        };
        rns_transport::iface::InterfaceMode::parse(value).ok_or_else(|| {
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

    pub fn flow_control_name(&self) -> Option<&str> {
        self.flow_control.as_ref().and_then(toml::Value::as_str)
    }

    pub fn auto_discovery_multicast_address(&self) -> Option<String> {
        if self.kind != "auto" {
            return None;
        }
        let scope = rns_transport::iface::auto::AutoDiscoveryScope::parse(
            self.discovery_scope.as_deref()?,
        )?;
        let address_type = rns_transport::iface::auto::MulticastAddressType::parse(
            self.multicast_address_type.as_deref()?,
        )?;
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
        if let Some(announce_cap) = self.announce_cap {
            if !(1..=100).contains(&announce_cap) {
                return Err(format!("interfaces[{index}].announce_cap must be between 1 and 100"));
            }
        }
        Ok(())
    }

    fn normalize_aliases(&mut self, index: usize, original_kind: &str) -> Result<(), String> {
        self.normalize_port_alias(index)?;
        if self.kind == "tcp_client" {
            self.normalize_tcp_client_aliases(index)?;
        }
        if self.kind == "tcp_server" {
            self.normalize_tcp_server_aliases(index)?;
        }
        if self.kind == "udp" {
            self.normalize_udp_aliases(index)?;
        }
        if self.kind == "auto" {
            self.normalize_auto_aliases(index)?;
        }
        if self.kind == "serial" {
            self.normalize_serial_aliases(index)?;
        }
        if self.kind == "vrn76_kiss_ble" {
            self.normalize_vrn76_kiss_ble_aliases(index)?;
        }
        if self.kind == "kiss" {
            self.normalize_kiss_aliases(index, original_kind)?;
        }
        if self.kind == "lora" {
            self.normalize_lora_aliases(index, original_kind)?;
        }
        Ok(())
    }

    fn normalize_port_alias(&mut self, index: usize) -> Result<(), String> {
        let Some(value) = self.extra.remove("port") else {
            return Ok(());
        };
        match self.kind.as_str() {
            "tcp_client" | "tcp_server" | "udp" | "kiss_tcp_client" => {
                if self.port.is_none() {
                    self.port = Some(port_number_from_value(value, index)?);
                }
            }
            "serial" | "kiss" | "lora" => {
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
        if self.mtu.is_none() {
            self.mtu = self
                .take_u64_alias_for_kind("fixed_mtu", index, "tcp_client")?
                .map(|value| {
                    usize::try_from(value).map_err(|_| {
                        format!("interfaces[{index}].fixed_mtu must fit in usize for tcp_client")
                    })
                })
                .transpose()?;
        } else {
            let _ = self.take_u64_alias_for_kind("fixed_mtu", index, "tcp_client")?;
        }
        if self.take_bool_alias_for_kind("kiss_framing", index, "tcp_client")?.unwrap_or(false) {
            self.kind = "kiss_tcp_client".to_string();
        }
        Ok(())
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

    fn normalize_udp_aliases(&mut self, index: usize) -> Result<(), String> {
        if self.host.is_none() {
            self.host = self.take_string_alias_for_kind("listen_ip", index, "udp")?;
        } else {
            let _ = self.take_string_alias_for_kind("listen_ip", index, "udp")?;
        }
        if self.port.is_none() {
            self.port = self.take_u16_alias_for_kind("listen_port", index, "udp")?;
        } else {
            let _ = self.take_u16_alias_for_kind("listen_port", index, "udp")?;
        }
        let used_forward_ip_alias = if self.target_host.is_none() {
            let forward_ip = self.take_string_alias_for_kind("forward_ip", index, "udp")?;
            let used = forward_ip.is_some();
            self.target_host = forward_ip;
            used
        } else {
            let _ = self.take_string_alias_for_kind("forward_ip", index, "udp")?;
            false
        };
        if self.target_port.is_none() {
            self.target_port = self.take_u16_alias_for_kind("forward_port", index, "udp")?;
        } else {
            let _ = self.take_u16_alias_for_kind("forward_port", index, "udp")?;
        }
        if used_forward_ip_alias && self.target_port.is_none() {
            self.target_port = self.port;
        }
        Ok(())
    }
}
