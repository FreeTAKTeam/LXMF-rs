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
        if self.multicast_address_type.is_none() {
            self.multicast_address_type = Some("temporary".to_string());
        }
        if self.bitrate.is_none() {
            self.bitrate = self.take_u64_alias_for_kind("configured_bitrate", index, "auto")?;
        } else {
            let _ = self.take_u64_alias_for_kind("configured_bitrate", index, "auto")?;
        }
        Ok(())
    }

    fn normalize_serial_aliases(&mut self, index: usize) -> Result<(), String> {
        if self.baud_rate.is_none() {
            self.baud_rate = self
                .take_u64_alias_for_kind("speed", index, "serial")?
                .map(|value| {
                    u32::try_from(value).map_err(|_| {
                        format!("interfaces[{index}].speed must fit in u32 for serial")
                    })
                })
                .transpose()?;
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

    fn normalize_lora_aliases(&mut self, index: usize, original_kind: &str) -> Result<(), String> {
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
        if !has_bind_host {
            return Err(format!("interfaces[{index}].host is required for udp"));
        }
        if self.port.is_none() {
            return Err(format!("interfaces[{index}].port is required for udp"));
        }
        let has_target_host =
            self.target_host.as_deref().map(str::trim).is_some_and(|value| !value.is_empty());
        if has_target_host ^ self.target_port.is_some() {
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
        .is_none()
        {
            return Err(format!(
                "interfaces[{index}].discovery_scope must be one of link, admin, site, organisation, organization, global for auto"
            ));
        }
        if rns_transport::iface::auto::MulticastAddressType::parse(
            self.multicast_address_type.as_deref().unwrap_or_default(),
        )
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
