impl InterfaceConfig {

    fn normalize_auto_aliases(&mut self, index: usize) -> Result<(), String> {
        if self.group_id.is_none() {
            self.group_id = Some("reticulum".to_string());
        }
        if self.discovery_scope.is_none() {
            self.discovery_scope = Some("link".to_string());
        }
        if self.discovery_port.is_none() {
            self.discovery_port = Some(29_716);
        }
        if self.data_port.is_none() {
            self.data_port = Some(42_671);
        }
        if self
            .multicast_address_type
            .as_deref()
            .map(rns_transport::iface::auto::MulticastAddressType::parse)
            .is_none_or(|parsed| parsed.ok().flatten().is_none())
        {
            self.multicast_address_type = Some("temporary".to_string());
        }
        if self.bitrate.is_none() {
            self.bitrate = self.take_u64_alias_for_kind("configured_bitrate", index, "auto")?;
        } else {
            let _ = self.take_u64_alias_for_kind("configured_bitrate", index, "auto")?;
        }
        Ok(())
    }

    fn normalize_serial_aliases(&mut self, index: usize, original_kind: &str) -> Result<(), String> {
        if self.baud_rate.is_none() {
            self.baud_rate = self
                .take_u64_alias_for_kind("speed", index, "serial")?
                .map(|value| {
                    u32::try_from(value).map_err(|_| {
                        format!("interfaces[{index}].speed must fit in u32 for serial")
                    })
                })
                .transpose()?
                .or_else(|| (original_kind == "SerialInterface").then_some(9_600));
        } else {
            let _ = self.take_u64_alias_for_kind("speed", index, "serial")?;
        }
        if self.data_bits.is_none() {
            self.data_bits = self.take_u8_alias_for_kind("databits", index, "serial")?;
        } else {
            let _ = self.take_u8_alias_for_kind("databits", index, "serial")?;
        }
        if self.stop_bits.is_none() {
            self.stop_bits = self.take_u8_alias_for_kind("stopbits", index, "serial")?;
        } else {
            let _ = self.take_u8_alias_for_kind("stopbits", index, "serial")?;
        }
        Ok(())
    }

    fn normalize_weave_aliases(&mut self, index: usize) -> Result<(), String> {
        if self.baud_rate.is_none() {
            self.baud_rate = self
                .take_u64_alias_for_kind("speed", index, "weave")?
                .map(|value| {
                    u32::try_from(value).map_err(|_| {
                        format!("interfaces[{index}].speed must fit in u32 for weave")
                    })
                })
                .transpose()?
                .or(Some(3_000_000));
        } else {
            let _ = self.take_u64_alias_for_kind("speed", index, "weave")?;
        }
        if self.mtu.is_none() {
            self.mtu = Some(1024);
        }
        if self.bitrate.is_none() {
            self.bitrate = self.take_u64_alias_for_kind("configured_bitrate", index, "weave")?;
        } else {
            let _ = self.take_u64_alias_for_kind("configured_bitrate", index, "weave")?;
        }
        Ok(())
    }

    fn normalize_i2p_aliases(&mut self, index: usize) -> Result<(), String> {
        let sam_host_alias = if self.sam_host.is_none() {
            self.take_string_alias_for_kind("sam_ip", index, "i2p")?.and_then(non_empty_string)
        } else {
            let _ = self.take_string_alias_for_kind("sam_ip", index, "i2p")?;
            None
        };
        let sam_port_alias = if self.sam_port.is_none() {
            self.take_u16_alias_for_kind("sam_port", index, "i2p")?
        } else {
            let _ = self.take_u16_alias_for_kind("sam_port", index, "i2p")?;
            None
        };
        let env_default = if self.sam_host.is_none()
            && sam_host_alias.is_none()
            && self.sam_port.is_none()
            && sam_port_alias.is_none()
        {
            i2p_sam_address_env_default()
        } else {
            None
        };
        if self.sam_host.is_none() {
            self.sam_host =
                sam_host_alias.or_else(|| env_default.as_ref().map(|(host, _)| host.clone()));
            if self.sam_host.is_none() {
                self.sam_host = Some("127.0.0.1".to_string());
            }
        }
        if self.sam_port.is_none() {
            self.sam_port = sam_port_alias.or_else(|| env_default.as_ref().map(|(_, port)| *port));
            if self.sam_port.is_none() {
                self.sam_port = Some(7656);
            }
        }
        if self.mtu.is_none() {
            self.mtu = Some(1064);
        }
        if self.bitrate.is_none() {
            self.bitrate = self
                .take_u64_alias_for_kind("configured_bitrate", index, "i2p")?
                .or(Some(256_000));
        } else {
            let _ = self.take_u64_alias_for_kind("configured_bitrate", index, "i2p")?;
        }
        if self.state_path.is_none() {
            self.state_path = self
                .take_string_alias_for_kind("storagepath", index, "i2p")?
                .and_then(non_empty_string);
        } else {
            let _ = self.take_string_alias_for_kind("storagepath", index, "i2p")?;
        }
        if self.network_name.is_none() && self.networkname.is_none() {
            self.network_name = self
                .take_string_alias_for_kind("ifac_netname", index, "i2p")?
                .and_then(non_empty_string);
        } else {
            let _ = self.take_string_alias_for_kind("ifac_netname", index, "i2p")?;
        }
        if self.passphrase.is_none() && self.pass_phrase.is_none() {
            self.passphrase = self
                .take_string_alias_for_kind("ifac_netkey", index, "i2p")?
                .and_then(non_empty_string);
        } else {
            let _ = self.take_string_alias_for_kind("ifac_netkey", index, "i2p")?;
        }
        Ok(())
    }

    fn normalize_lora_aliases(&mut self, index: usize, original_kind: &str) -> Result<(), String> {
        if original_kind == "RNodeInterface" {
            self.rnode_profile = true;
            self.normalize_android_rnode_selector_aliases(index)?;
            if self.region.is_none() {
                self.region = Some("US915".to_string());
            }
            if self.max_payload_bytes.is_none() {
                self.max_payload_bytes = Some(508);
            }
        }
        if self.frequency_hz.is_none() {
            self.frequency_hz = self.take_u64_alias_for_kind("frequency", index, "lora")?;
        } else {
            let _ = self.take_u64_alias_for_kind("frequency", index, "lora")?;
        }
        if self.bandwidth_hz.is_none() {
            self.bandwidth_hz = self
                .take_u64_alias_for_kind("bandwidth", index, "lora")?
                .map(|value| {
                    u32::try_from(value).map_err(|_| {
                        format!("interfaces[{index}].bandwidth must fit in u32 for lora")
                    })
                })
                .transpose()?;
        } else {
            let _ = self.take_u64_alias_for_kind("bandwidth", index, "lora")?;
        }
        if self.spreading_factor.is_none() {
            self.spreading_factor =
                self.take_u8_alias_for_kind("spreadingfactor", index, "lora")?;
        } else {
            let _ = self.take_u8_alias_for_kind("spreadingfactor", index, "lora")?;
        }
        if self.coding_rate.is_none() {
            self.coding_rate = self.take_string_or_integer_alias("codingrate", index, "lora")?;
        } else {
            let _ = self.take_string_or_integer_alias("codingrate", index, "lora")?;
        }
        if self.tx_power_dbm.is_none() {
            self.tx_power_dbm = self.take_i8_alias_for_kind("txpower", index, "lora")?;
        } else {
            let _ = self.take_i8_alias_for_kind("txpower", index, "lora")?;
        }
        if self.connect_timeout_ms.is_none() {
            self.connect_timeout_ms =
                self.take_u64_alias_for_kind("command_timeout_ms", index, "lora")?;
        } else {
            let _ = self.take_u64_alias_for_kind("command_timeout_ms", index, "lora")?;
        }
        if self.baud_rate.is_none()
            && original_kind == "RNodeInterface"
            && self
                .device
                .as_deref()
                .is_some_and(|device| !is_tcp_lora_port(device) && !is_ble_lora_port(device))
        {
            self.baud_rate = Some(115_200);
        }
        Ok(())
    }

    fn normalize_android_rnode_selector_aliases(&mut self, index: usize) -> Result<(), String> {
        let tcp_requested = self.force_tcp.unwrap_or(false)
            || self.tcp_host.as_deref().map(str::trim).is_some_and(|value| !value.is_empty());
        let ble_requested = self.force_ble.unwrap_or(false)
            || self.allow_bluetooth.unwrap_or(false)
            || self.ble_addr.as_deref().map(str::trim).is_some_and(|value| !value.is_empty())
            || self.ble_name.as_deref().map(str::trim).is_some_and(|value| !value.is_empty())
            || self
                .target_device_address
                .as_deref()
                .map(str::trim)
                .is_some_and(|value| !value.is_empty())
            || self
                .target_device_name
                .as_deref()
                .map(str::trim)
                .is_some_and(|value| !value.is_empty());

        if tcp_requested && ble_requested {
            return Err(format!(
                "interfaces[{index}] cannot combine RNodeInterface force_tcp/tcp_host with Bluetooth selector fields"
            ));
        }
        if self.device.is_some() || (!tcp_requested && !ble_requested) {
            return Ok(());
        }

        if tcp_requested {
            let Some(tcp_host) = self.tcp_host.as_deref().map(str::trim).filter(|value| !value.is_empty()) else {
                return Err(format!(
                    "interfaces[{index}].tcp_host is required when force_tcp is true for RNodeInterface"
                ));
            };
            self.device = Some(android_rnode_tcp_device(tcp_host));
        } else if ble_requested {
            let ble_target = self
                .ble_addr
                .as_deref()
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .or_else(|| {
                    self.target_device_address
                        .as_deref()
                        .map(str::trim)
                        .filter(|value| !value.is_empty())
                })
                .or_else(|| {
                    self.ble_name
                        .as_deref()
                        .map(str::trim)
                        .filter(|value| !value.is_empty())
                })
                .or_else(|| {
                    self.target_device_name
                        .as_deref()
                        .map(str::trim)
                        .filter(|value| !value.is_empty())
                })
                .ok_or_else(|| {
                    format!(
                        "interfaces[{index}].ble_name, ble_addr, target_device_name, or target_device_address is required when Bluetooth is requested for RNodeInterface"
                    )
                })?;
            self.device = Some(format!("ble://{ble_target}"));
        }

        Ok(())
    }

    fn normalize_rnode_multi_aliases(&mut self, index: usize) -> Result<(), String> {
        if self.baud_rate.is_none() {
            self.baud_rate = self
                .take_u64_alias_for_kind("speed", index, "rnode_multi")?
                .map(|value| {
                    u32::try_from(value).map_err(|_| {
                        format!("interfaces[{index}].speed must fit in u32 for rnode_multi")
                    })
                })
                .transpose()?
                .or(Some(115_200));
        } else {
            let _ = self.take_u64_alias_for_kind("speed", index, "rnode_multi")?;
        }
        if self.mtu.is_none() {
            self.mtu = Some(508);
        }
        Ok(())
    }

    fn normalize_kiss_aliases(&mut self, index: usize, original_kind: &str) -> Result<(), String> {
        if self.baud_rate.is_none() {
            self.baud_rate = self
                .take_u64_alias_for_kind("speed", index, "kiss")?
                .map(|value| {
                    u32::try_from(value)
                        .map_err(|_| format!("interfaces[{index}].speed must fit in u32 for kiss"))
                })
                .transpose()?;
            if self.baud_rate.is_none() && original_kind == "KISSInterface" {
                self.baud_rate = Some(9_600);
            }
        } else {
            let _ = self.take_u64_alias_for_kind("speed", index, "kiss")?;
        }
        if self.data_bits.is_none() {
            self.data_bits = self.take_u8_alias_for_kind("databits", index, "kiss")?;
        } else {
            let _ = self.take_u8_alias_for_kind("databits", index, "kiss")?;
        }
        if self.stop_bits.is_none() {
            self.stop_bits = self.take_u8_alias_for_kind("stopbits", index, "kiss")?;
        } else {
            let _ = self.take_u8_alias_for_kind("stopbits", index, "kiss")?;
        }
        if self.preamble_ms.is_none() {
            self.preamble_ms = self.take_u16_alias_for_kind("preamble", index, "kiss")?;
        } else {
            let _ = self.take_u16_alias_for_kind("preamble", index, "kiss")?;
        }
        if self.tx_tail_ms.is_none() {
            self.tx_tail_ms = self.take_u16_alias_for_kind("txtail", index, "kiss")?;
        } else {
            let _ = self.take_u16_alias_for_kind("txtail", index, "kiss")?;
        }
        if self.slot_time_ms.is_none() {
            self.slot_time_ms = self.take_u16_alias_for_kind("slottime", index, "kiss")?;
        } else {
            let _ = self.take_u16_alias_for_kind("slottime", index, "kiss")?;
        }
        if self.kiss_flow_control.is_none() {
            if let Some(flow_control) = self.flow_control.as_ref().and_then(toml::Value::as_bool) {
                self.kiss_flow_control = Some(flow_control);
            }
        }
        if self.id_interval.is_none() {
            self.id_interval = self.take_u64_alias_for_kind("beacon_interval", index, "kiss")?;
        } else {
            let _ = self.take_u64_alias_for_kind("beacon_interval", index, "kiss")?;
        }
        if self.id_callsign.is_none() {
            self.id_callsign = self
                .take_string_alias_for_kind("beacon_data", index, "kiss")?
                .and_then(non_empty_string);
        } else {
            let _ = self.take_string_alias_for_kind("beacon_data", index, "kiss")?;
        }
        Ok(())
    }

    fn normalize_kiss_tcp_client_aliases(&mut self, _index: usize) {
        if self.kiss_flow_control.is_none() {
            if let Some(flow_control) = self.flow_control.as_ref().and_then(toml::Value::as_bool) {
                self.kiss_flow_control = Some(flow_control);
            }
        }
    }

    fn normalize_ax25_kiss_aliases(&mut self, index: usize) -> Result<(), String> {
        if self.baud_rate.is_none() {
            self.baud_rate = self
                .take_u64_alias_for_kind("speed", index, "ax25_kiss")?
                .map(|value| {
                    u32::try_from(value).map_err(|_| {
                        format!("interfaces[{index}].speed must fit in u32 for ax25_kiss")
                    })
                })
                .transpose()?
                .or(Some(9_600));
        } else {
            let _ = self.take_u64_alias_for_kind("speed", index, "ax25_kiss")?;
        }
        if self.data_bits.is_none() {
            self.data_bits = self.take_u8_alias_for_kind("databits", index, "ax25_kiss")?;
        } else {
            let _ = self.take_u8_alias_for_kind("databits", index, "ax25_kiss")?;
        }
        if self.stop_bits.is_none() {
            self.stop_bits = self.take_u8_alias_for_kind("stopbits", index, "ax25_kiss")?;
        } else {
            let _ = self.take_u8_alias_for_kind("stopbits", index, "ax25_kiss")?;
        }
        if self.preamble_ms.is_none() {
            self.preamble_ms = self.take_u16_alias_for_kind("preamble", index, "ax25_kiss")?;
        } else {
            let _ = self.take_u16_alias_for_kind("preamble", index, "ax25_kiss")?;
        }
        if self.tx_tail_ms.is_none() {
            self.tx_tail_ms = self.take_u16_alias_for_kind("txtail", index, "ax25_kiss")?;
        } else {
            let _ = self.take_u16_alias_for_kind("txtail", index, "ax25_kiss")?;
        }
        if self.slot_time_ms.is_none() {
            self.slot_time_ms = self.take_u16_alias_for_kind("slottime", index, "ax25_kiss")?;
        } else {
            let _ = self.take_u16_alias_for_kind("slottime", index, "ax25_kiss")?;
        }
        if self.kiss_flow_control.is_none() {
            if let Some(flow_control) = self.flow_control.as_ref().and_then(toml::Value::as_bool) {
                self.kiss_flow_control = Some(flow_control);
            }
        }
        if self.mtu.is_none() {
            self.mtu = Some(564);
        }
        Ok(())
    }

    fn normalize_vrn76_kiss_ble_aliases(&mut self, index: usize) -> Result<(), String> {
        if self.peripheral_id.is_none() {
            let device_address = self.take_string_alias("device_address", index)?;
            let device_name_filter = self.take_string_alias("device_name_filter", index)?;
            self.peripheral_id = device_address
                .and_then(non_empty_string)
                .or_else(|| device_name_filter.and_then(non_empty_string));
        } else {
            let _ = self.take_string_alias("device_address", index)?;
            let _ = self.take_string_alias("device_name_filter", index)?;
        }
        if self.scan_timeout_ms.is_none() {
            self.scan_timeout_ms = self.take_u64_alias("ble_scan_timeout_ms", index)?;
        } else {
            let _ = self.take_u64_alias("ble_scan_timeout_ms", index)?;
        }
        if self.connect_timeout_ms.is_none() {
            self.connect_timeout_ms = self.take_u64_alias("command_timeout_ms", index)?;
        } else {
            let _ = self.take_u64_alias("command_timeout_ms", index)?;
        }
        if self.preamble_ms.is_none() {
            self.preamble_ms = self.take_u16_alias("preamble", index)?;
        } else {
            let _ = self.take_u16_alias("preamble", index)?;
        }
        if self.tx_tail_ms.is_none() {
            self.tx_tail_ms = self.take_u16_alias("txtail", index)?;
        } else {
            let _ = self.take_u16_alias("txtail", index)?;
        }
        if self.slot_time_ms.is_none() {
            self.slot_time_ms = self.take_u16_alias("slottime", index)?;
        } else {
            let _ = self.take_u16_alias("slottime", index)?;
        }
        if self.kiss_flow_control.is_none() {
            if let Some(flow_control) = self.flow_control.as_ref().and_then(toml::Value::as_bool) {
                self.kiss_flow_control = Some(flow_control);
            }
        }
        Ok(())
    }

    fn take_string_alias(&mut self, key: &str, index: usize) -> Result<Option<String>, String> {
        self.take_string_alias_for_kind(key, index, "vrn76_kiss_ble")
    }

    fn take_string_alias_for_kind(
        &mut self,
        key: &str,
        index: usize,
        kind: &str,
    ) -> Result<Option<String>, String> {
        let Some(value) = self.extra.remove(key) else {
            return Ok(None);
        };
        value
            .as_str()
            .map(|value| Some(value.to_string()))
            .ok_or_else(|| format!("interfaces[{index}].{key} must be a string for {kind}"))
    }

    fn take_u64_alias(&mut self, key: &str, index: usize) -> Result<Option<u64>, String> {
        let Some(value) = self.extra.remove(key) else {
            return Ok(None);
        };
        value.as_integer().and_then(|value| u64::try_from(value).ok()).map(Some).ok_or_else(|| {
            format!("interfaces[{index}].{key} must be a non-negative integer for vrn76_kiss_ble")
        })
    }

    fn take_u64_alias_for_kind(
        &mut self,
        key: &str,
        index: usize,
        kind: &str,
    ) -> Result<Option<u64>, String> {
        let Some(value) = self.extra.remove(key) else {
            return Ok(None);
        };
        value.as_integer().and_then(|value| u64::try_from(value).ok()).map(Some).ok_or_else(|| {
            format!("interfaces[{index}].{key} must be a non-negative integer for {kind}")
        })
    }

    fn take_u8_alias_for_kind(
        &mut self,
        key: &str,
        index: usize,
        kind: &str,
    ) -> Result<Option<u8>, String> {
        self.take_u64_alias_for_kind(key, index, kind)?
            .map(|value| {
                u8::try_from(value)
                    .map_err(|_| format!("interfaces[{index}].{key} must fit in u8 for {kind}"))
            })
            .transpose()
    }

    fn take_i8_alias_for_kind(
        &mut self,
        key: &str,
        index: usize,
        kind: &str,
    ) -> Result<Option<i8>, String> {
        let Some(value) = self.extra.remove(key) else {
            return Ok(None);
        };
        value
            .as_integer()
            .and_then(|value| i8::try_from(value).ok())
            .map(Some)
            .ok_or_else(|| format!("interfaces[{index}].{key} must fit in i8 for {kind}"))
    }

    fn take_bool_alias_for_kind(
        &mut self,
        key: &str,
        index: usize,
        kind: &str,
    ) -> Result<Option<bool>, String> {
        let Some(value) = self.extra.remove(key) else {
            return Ok(None);
        };
        value
            .as_bool()
            .map(Some)
            .ok_or_else(|| format!("interfaces[{index}].{key} must be a boolean for {kind}"))
    }

    fn take_string_or_integer_alias(
        &mut self,
        key: &str,
        index: usize,
        kind: &str,
    ) -> Result<Option<String>, String> {
        let Some(value) = self.extra.remove(key) else {
            return Ok(None);
        };
        if let Some(value) = value.as_str() {
            return Ok(Some(value.to_string()));
        }
        value.as_integer().map(|value| Some(value.to_string())).ok_or_else(|| {
            format!("interfaces[{index}].{key} must be a string or integer for {kind}")
        })
    }

    fn take_u16_alias(&mut self, key: &str, index: usize) -> Result<Option<u16>, String> {
        let Some(value) = self.extra.remove(key) else {
            return Ok(None);
        };
        value.as_integer().and_then(|value| u16::try_from(value).ok()).map(Some).ok_or_else(|| {
            format!("interfaces[{index}].{key} must be a 16-bit integer for vrn76_kiss_ble")
        })
    }

    fn take_u16_alias_for_kind(
        &mut self,
        key: &str,
        index: usize,
        kind: &str,
    ) -> Result<Option<u16>, String> {
        self.take_u64_alias_for_kind(key, index, kind)?
            .map(|value| {
                u16::try_from(value)
                    .map_err(|_| format!("interfaces[{index}].{key} must fit in u16 for {kind}"))
            })
            .transpose()
    }

    fn validate_udp(&self, index: usize) -> Result<(), String> {
        self.reject_unknown_new_kind_keys(index, "udp")?;
        if !self.enabled() {
            return Ok(());
        }
        let has_bind_host =
            self.host.as_deref().map(str::trim).is_some_and(|value| !value.is_empty());
        let has_device =
            self.device.as_deref().map(str::trim).is_some_and(|value| !value.is_empty());
        if !has_bind_host && !has_device {
            return Err(format!("interfaces[{index}].host or device is required for udp"));
        }
        if self.port.is_none() {
            return Err(format!("interfaces[{index}].port is required for udp"));
        }
        let has_target_host =
            self.target_host.as_deref().map(str::trim).is_some_and(|value| !value.is_empty());
        let has_target_port = self.target_port.is_some();
        let missing_target_pair = has_target_host ^ has_target_port;
        let device_only_udp = has_device && !has_target_host;
        if missing_target_pair && !device_only_udp {
            return Err(format!(
                "interfaces[{index}].target_host and target_port must be provided together for udp"
            ));
        }
        Ok(())
    }

    fn validate_auto(&self, index: usize) -> Result<(), String> {
        self.reject_unknown_new_kind_keys(index, "auto")?;
        if !self.enabled() {
            return Ok(());
        }
        require_non_empty(
            self.group_id.as_deref(),
            &format!("interfaces[{index}].group_id is required for auto"),
        )?;
        if rns_transport::iface::auto::AutoDiscoveryScope::parse(
            self.discovery_scope.as_deref().unwrap_or_default(),
        )
        .ok()
        .flatten()
        .is_none()
        {
            return Err(format!(
                "interfaces[{index}].discovery_scope must be one of link, admin, site, organisation, organization, global for auto"
            ));
        }
        if rns_transport::iface::auto::MulticastAddressType::parse(
            self.multicast_address_type.as_deref().unwrap_or_default(),
        )
        .ok()
        .flatten()
        .is_none()
        {
            return Err(format!(
                "interfaces[{index}].multicast_address_type must be temporary or permanent for auto"
            ));
        }
        if self.discovery_port == Some(0) {
            return Err(format!("interfaces[{index}].discovery_port must be > 0 for auto"));
        }
        if self.data_port == Some(0) {
            return Err(format!("interfaces[{index}].data_port must be > 0 for auto"));
        }
        Ok(())
    }
}

fn android_rnode_tcp_device(tcp_host: &str) -> String {
    const ANDROID_RNODE_TCP_PORT: u16 = 7633;
    let tcp_host = tcp_host.trim();
    if tcp_host.to_ascii_lowercase().starts_with("tcp://") {
        tcp_host.to_string()
    } else if tcp_host.rsplit_once(':').is_some_and(|(_, port)| port.parse::<u16>().is_ok()) {
        format!("tcp://{tcp_host}")
    } else {
        format!("tcp://{tcp_host}:{ANDROID_RNODE_TCP_PORT}")
    }
}

fn i2p_sam_address_env_default() -> Option<(String, u16)> {
    std::env::var("I2P_SAM_ADDRESS")
        .ok()
        .and_then(|value| parse_i2p_sam_address(value.as_str()))
}

fn parse_i2p_sam_address(value: &str) -> Option<(String, u16)> {
    let (host, port) = value.trim().split_once(':')?;
    let host = host.trim();
    if host.is_empty() {
        return None;
    }
    Some((host.to_string(), port.trim().parse().ok()?))
}
