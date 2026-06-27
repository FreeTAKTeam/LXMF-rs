impl InterfaceConfig {

    fn validate_lora(&self, index: usize, original_kind: &str) -> Result<(), String> {
        self.reject_unknown_new_kind_keys(index, "lora")?;
        if !self.enabled() {
            return Ok(());
        }
        require_non_empty(
            self.region.as_deref(),
            &format!("interfaces[{index}].region is required for lora"),
        )?;
        let region = self.region.as_deref().unwrap_or_default();
        if !is_supported_lora_region(region) {
            return Err(format!(
                "interfaces[{index}].region must be one of EU868, US915, AU915, AS923, IN865, KR920, RU864 for lora"
            ));
        }
        if original_kind != "RNodeInterface"
            && self.state_path.as_deref().map(str::trim).filter(|value| !value.is_empty()).is_none()
        {
            return Err(format!("interfaces[{index}].state_path is required for lora"));
        }
        let has_device =
            self.device.as_deref().map(str::trim).is_some_and(|value| !value.is_empty());
        let has_tcp_device = self.device.as_deref().is_some_and(is_tcp_lora_port);
        let has_ble_device = self.device.as_deref().is_some_and(is_ble_lora_port);
        if has_device && !has_tcp_device && !has_ble_device && self.baud_rate.is_none() {
            return Err(format!("interfaces[{index}].baud_rate is required for active lora"));
        }
        if !has_device && self.baud_rate.is_some() {
            return Err(format!("interfaces[{index}].device is required for active lora"));
        }
        if self.baud_rate == Some(0) {
            return Err(format!("interfaces[{index}].baud_rate must be > 0 for lora"));
        }
        if let Some(adapter) = self.adapter.as_deref() {
            require_non_empty(
                Some(adapter),
                &format!("interfaces[{index}].adapter cannot be empty for lora"),
            )?;
        }
        if original_kind == "RNodeInterface" {
            self.validate_rnode_required_radio_parameters(index)?;
        }
        if let Some(scan_timeout_ms) = self.scan_timeout_ms {
            if scan_timeout_ms == 0 {
                return Err(format!("interfaces[{index}].scan_timeout_ms must be > 0 for lora"));
            }
        }
        if let Some(connect_timeout_ms) = self.connect_timeout_ms {
            if connect_timeout_ms == 0 {
                return Err(format!("interfaces[{index}].connect_timeout_ms must be > 0 for lora"));
            }
        }
        if let Some(ble_connect_timeout_ms) = self.ble_connect_timeout_ms {
            if ble_connect_timeout_ms == 0 {
                return Err(format!(
                    "interfaces[{index}].ble_connect_timeout_ms must be > 0 for lora"
                ));
            }
        }
        if let Some(mtu) = self.mtu {
            if mtu == 0 {
                return Err(format!("interfaces[{index}].mtu must be > 0 for lora"));
            }
        }
        if let Some(max_write_len) = self.max_write_len {
            if max_write_len == 0 {
                return Err(format!("interfaces[{index}].max_write_len must be > 0 for lora"));
            }
        }
        self.validate_id_beacon(index, "lora")?;
        if let Some(flow_control) = self.flow_control.as_ref() {
            if !flow_control.is_bool() {
                return Err(format!("interfaces[{index}].flow_control must be a boolean for lora"));
            }
        }
        if let Some(frequency_hz) = self.frequency_hz {
            if !(137_000_000..=3_000_000_000).contains(&frequency_hz) {
                return Err(format!(
                    "interfaces[{index}].frequency_hz must be between 137000000 and 3000000000 for lora"
                ));
            }
        }
        if let Some(spreading_factor) = self.spreading_factor {
            if !(5..=12).contains(&spreading_factor) {
                return Err(format!(
                    "interfaces[{index}].spreading_factor must be between 5 and 12 for lora"
                ));
            }
        }
        if let Some(coding_rate) = self.coding_rate.as_deref() {
            if !matches_normalized(coding_rate, &["4/5", "4/6", "4/7", "4/8", "5", "6", "7", "8"]) {
                return Err(format!(
                    "interfaces[{index}].coding_rate must be one of 4/5, 4/6, 4/7, 4/8, 5, 6, 7, 8 for lora"
                ));
            }
        }
        if let Some(bandwidth_hz) = self.bandwidth_hz {
            if !(7_800..=1_625_000).contains(&bandwidth_hz) {
                return Err(format!(
                    "interfaces[{index}].bandwidth_hz must be between 7800 and 1625000 for lora"
                ));
            }
        }
        if let Some(tx_power_dbm) = self.tx_power_dbm {
            if !(0..=37).contains(&tx_power_dbm) {
                return Err(format!(
                    "interfaces[{index}].tx_power_dbm must be between 0 and 37 for lora"
                ));
            }
        }
        if let Some(max_payload_bytes) = self.max_payload_bytes {
            let max_payload_limit = if original_kind == "RNodeInterface" { 508 } else { 255 };
            if !(1..=max_payload_limit).contains(&max_payload_bytes) {
                return Err(format!(
                    "interfaces[{index}].max_payload_bytes must be between 1 and {max_payload_limit} for lora"
                ));
            }
        }
        if let Some(airtime_limit_short) = self.airtime_limit_short {
            if !(0.0..=100.0).contains(&airtime_limit_short) {
                return Err(format!(
                    "interfaces[{index}].airtime_limit_short must be between 0 and 100 for lora"
                ));
            }
        }
        if let Some(airtime_limit_long) = self.airtime_limit_long {
            if !(0.0..=100.0).contains(&airtime_limit_long) {
                return Err(format!(
                    "interfaces[{index}].airtime_limit_long must be between 0 and 100 for lora"
                ));
            }
        }
        Ok(())
    }

    fn validate_rnode_required_radio_parameters(&self, index: usize) -> Result<(), String> {
        if self.frequency_hz.is_none() {
            return Err(format!("interfaces[{index}].frequency is required for RNodeInterface"));
        }
        if self.bandwidth_hz.is_none() {
            return Err(format!("interfaces[{index}].bandwidth is required for RNodeInterface"));
        }
        if self.spreading_factor.is_none() {
            return Err(format!(
                "interfaces[{index}].spreadingfactor is required for RNodeInterface"
            ));
        }
        if self.coding_rate.is_none() {
            return Err(format!("interfaces[{index}].codingrate is required for RNodeInterface"));
        }
        Ok(())
    }

    fn validate_rnode_multi(&self, index: usize) -> Result<(), String> {
        if !self.enabled() {
            return Ok(());
        }
        self.validate_id_beacon(index, "rnode_multi")?;
        require_non_empty(
            self.device.as_deref(),
            &format!("interfaces[{index}].device or port is required for RNodeMultiInterface"),
        )?;
        let device = self.device.as_deref().unwrap_or_default().trim();
        if is_ble_lora_port(device) {
            return Err(format!(
                "interfaces[{index}].RNodeMultiInterface currently supports serial and tcp ports only"
            ));
        }
        if is_tcp_lora_port(device) {
            let addr = device
                .strip_prefix("tcp://")
                .or_else(|| device.strip_prefix("TCP://"))
                .map(str::trim)
                .unwrap_or_default();
            if addr.is_empty() {
                return Err(format!(
                    "interfaces[{index}].RNodeMultiInterface tcp port must include an address after tcp://"
                ));
            }
        } else {
            match self.baud_rate {
                Some(0) => {
                    return Err(format!(
                        "interfaces[{index}].baud_rate must be > 0 for rnode_multi"
                    ));
                }
                None => {
                    return Err(format!(
                        "interfaces[{index}].baud_rate is required for rnode_multi"
                    ));
                }
                Some(_) => {}
            }
        }
        if let Some(mtu) = self.mtu {
            if !(1..=u16::MAX as usize).contains(&mtu) {
                return Err(format!(
                    "interfaces[{index}].mtu must be between 1 and 65535 for rnode_multi"
                ));
            }
        }
        if let Some(flow_control) = self.flow_control.as_ref() {
            if !flow_control.is_bool() {
                return Err(format!(
                    "interfaces[{index}].flow_control must be a boolean for rnode_multi"
                ));
            }
        }
        validate_rnode_multi_airtime_limit(
            self.airtime_limit_short,
            "airtime_limit_short",
            index,
            None,
        )?;
        validate_rnode_multi_airtime_limit(
            self.airtime_limit_long,
            "airtime_limit_long",
            index,
            None,
        )?;

        let mut enabled_subinterfaces = 0usize;
        let mut vports = std::collections::BTreeSet::new();
        for (name, value) in &self.extra {
            let Some(table) = value.as_table() else {
                return Err(format!(
                    "interfaces[{index}].{name} must be a subinterface table for rnode_multi"
                ));
            };
            let interface_enabled = rnode_multi_table_enabled(table, index, name)?;
            if !interface_enabled {
                continue;
            }
            enabled_subinterfaces += 1;
            let vport = rnode_multi_required_u8(table, "vport", index, name)?;
            if vport > 11 {
                return Err(format!("interfaces[{index}].{name}.vport must be between 0 and 11"));
            }
            if !vports.insert(vport) {
                return Err(format!("interfaces[{index}].{name}.vport {vport} is duplicated"));
            }
            rnode_multi_required_u64_alias(table, "frequency_hz", "frequency", index, name)?;
            rnode_multi_required_u32_alias(table, "bandwidth_hz", "bandwidth", index, name)?;
            let spreading_factor = rnode_multi_required_u8_alias(
                table,
                "spreading_factor",
                "spreadingfactor",
                index,
                name,
            )?;
            if !(5..=12).contains(&spreading_factor) {
                return Err(format!(
                    "interfaces[{index}].{name}.spreadingfactor must be between 5 and 12"
                ));
            }
            let coding_rate =
                rnode_multi_required_coding_rate(table, "coding_rate", "codingrate", index, name)?;
            if !(5..=8).contains(&coding_rate) {
                return Err(format!(
                    "interfaces[{index}].{name}.codingrate must be one of 4/5, 4/6, 4/7, 4/8, 5, 6, 7, 8"
                ));
            }
            let tx_power_dbm =
                rnode_multi_required_i8_alias(table, "tx_power_dbm", "txpower", index, name)?;
            if !(-9..=37).contains(&tx_power_dbm) {
                return Err(format!(
                    "interfaces[{index}].{name}.txpower must be between -9 and 37"
                ));
            }
            if let Some(flow_control) = table.get("flow_control") {
                if !flow_control.is_bool() {
                    return Err(format!(
                        "interfaces[{index}].{name}.flow_control must be a boolean for rnode_multi"
                    ));
                }
            }
            if let Some(airtime_limit_short) =
                rnode_multi_optional_f64(table, "airtime_limit_short", index, name)?
            {
                validate_rnode_multi_airtime_limit(
                    Some(airtime_limit_short),
                    "airtime_limit_short",
                    index,
                    Some(name),
                )?;
            }
            if let Some(airtime_limit_long) =
                rnode_multi_optional_f64(table, "airtime_limit_long", index, name)?
            {
                validate_rnode_multi_airtime_limit(
                    Some(airtime_limit_long),
                    "airtime_limit_long",
                    index,
                    Some(name),
                )?;
            }
        }
        if enabled_subinterfaces == 0 {
            return Err(format!(
                "interfaces[{index}] must contain at least one enabled RNodeMultiInterface subinterface table"
            ));
        }
        Ok(())
    }

    fn validate_id_beacon(&self, index: usize, kind: &str) -> Result<(), String> {
        if let Some(callsign) = self.id_callsign.as_deref() {
            let callsign = callsign.trim();
            if callsign.is_empty() {
                return Err(format!("interfaces[{index}].id_callsign cannot be empty for {kind}"));
            }
            if callsign.len() > 32 {
                return Err(format!(
                    "interfaces[{index}].id_callsign must be 32 bytes or fewer for {kind}"
                ));
            }
        }
        if self.id_interval == Some(0) {
            return Err(format!("interfaces[{index}].id_interval must be > 0 for {kind}"));
        }
        Ok(())
    }

    fn reject_unknown_new_kind_keys(&self, index: usize, kind: &str) -> Result<(), String> {
        self.reject_unknown_new_kind_keys_except(index, kind, &[])
    }

    fn reject_unknown_new_kind_keys_except(
        &self,
        index: usize,
        kind: &str,
        allowed: &[&str],
    ) -> Result<(), String> {
        if self.extra.is_empty() {
            return Ok(());
        }
        let mut unknown = self
            .extra
            .keys()
            .filter(|key| !allowed.iter().any(|allowed| allowed == &key.as_str()))
            .cloned()
            .collect::<Vec<_>>();
        if unknown.is_empty() {
            return Ok(());
        }
        unknown.sort();
        Err(format!(
            "interfaces[{index}] ({kind}) contains unknown settings key(s): {}",
            unknown.join(", ")
        ))
    }
}

fn rnode_multi_table_bool(
    table: &toml::value::Table,
    key: &str,
    default: bool,
    index: usize,
    name: &str,
) -> Result<bool, String> {
    table.get(key).map_or(Ok(default), |value| {
        value.as_bool().ok_or_else(|| {
            format!("interfaces[{index}].{name}.{key} must be a boolean for rnode_multi")
        })
    })
}

fn rnode_multi_table_enabled(
    table: &toml::value::Table,
    index: usize,
    name: &str,
) -> Result<bool, String> {
    if table.contains_key("interface_enabled") {
        rnode_multi_table_bool(table, "interface_enabled", true, index, name)
    } else {
        rnode_multi_table_bool(table, "enabled", true, index, name)
    }
}

fn rnode_multi_required_u8(
    table: &toml::value::Table,
    key: &str,
    index: usize,
    name: &str,
) -> Result<u8, String> {
    table
        .get(key)
        .and_then(toml::Value::as_integer)
        .and_then(|value| u8::try_from(value).ok())
        .ok_or_else(|| {
            format!("interfaces[{index}].{name}.{key} must be an integer for rnode_multi")
        })
}

fn rnode_multi_required_u64_alias(
    table: &toml::value::Table,
    primary: &str,
    alias: &str,
    index: usize,
    name: &str,
) -> Result<u64, String> {
    let value = table.get(primary).or_else(|| table.get(alias)).and_then(toml::Value::as_integer);
    let value = value
        .and_then(|value| u64::try_from(value).ok())
        .ok_or_else(|| format!("interfaces[{index}].{name}.{alias} is required for rnode_multi"))?;
    if !(137_000_000..=3_000_000_000).contains(&value) {
        return Err(format!(
            "interfaces[{index}].{name}.{alias} must be between 137000000 and 3000000000"
        ));
    }
    Ok(value)
}

fn rnode_multi_required_u32_alias(
    table: &toml::value::Table,
    primary: &str,
    alias: &str,
    index: usize,
    name: &str,
) -> Result<u32, String> {
    let value = table.get(primary).or_else(|| table.get(alias)).and_then(toml::Value::as_integer);
    let value = value
        .and_then(|value| u32::try_from(value).ok())
        .ok_or_else(|| format!("interfaces[{index}].{name}.{alias} is required for rnode_multi"))?;
    if !(7_800..=1_625_000).contains(&value) {
        return Err(format!(
            "interfaces[{index}].{name}.{alias} must be between 7800 and 1625000"
        ));
    }
    Ok(value)
}

fn rnode_multi_required_u8_alias(
    table: &toml::value::Table,
    primary: &str,
    alias: &str,
    index: usize,
    name: &str,
) -> Result<u8, String> {
    table
        .get(primary)
        .or_else(|| table.get(alias))
        .and_then(toml::Value::as_integer)
        .and_then(|value| u8::try_from(value).ok())
        .ok_or_else(|| format!("interfaces[{index}].{name}.{alias} is required for rnode_multi"))
}

fn rnode_multi_required_coding_rate(
    table: &toml::value::Table,
    primary: &str,
    alias: &str,
    index: usize,
    name: &str,
) -> Result<u8, String> {
    let value = table
        .get(primary)
        .or_else(|| table.get(alias))
        .ok_or_else(|| format!("interfaces[{index}].{name}.{alias} is required for rnode_multi"))?;
    match value {
        toml::Value::String(value) => match value.trim() {
            "4/5" | "5" => Ok(5),
            "4/6" | "6" => Ok(6),
            "4/7" | "7" => Ok(7),
            "4/8" | "8" => Ok(8),
            _ => Err(format!("interfaces[{index}].{name}.{alias} has unsupported coding rate")),
        },
        toml::Value::Integer(value) => u8::try_from(*value)
            .ok()
            .filter(|value| (5..=8).contains(value))
            .ok_or_else(|| {
                format!("interfaces[{index}].{name}.{alias} has unsupported coding rate")
            }),
        _ => Err(format!(
            "interfaces[{index}].{name}.{alias} must be a string or integer for rnode_multi"
        )),
    }
}

fn rnode_multi_required_i8_alias(
    table: &toml::value::Table,
    primary: &str,
    alias: &str,
    index: usize,
    name: &str,
) -> Result<i8, String> {
    table
        .get(primary)
        .or_else(|| table.get(alias))
        .ok_or_else(|| format!("interfaces[{index}].{name}.{alias} is required for rnode_multi"))?
        .as_integer()
        .and_then(|value| i8::try_from(value).ok())
        .ok_or_else(|| format!("interfaces[{index}].{name}.{alias} must be an integer"))
}

fn rnode_multi_optional_f64(
    table: &toml::value::Table,
    key: &str,
    index: usize,
    name: &str,
) -> Result<Option<f64>, String> {
    let Some(value) = table.get(key) else {
        return Ok(None);
    };
    match value {
        toml::Value::Float(value) => Ok(Some(*value)),
        toml::Value::Integer(value) => Ok(Some(*value as f64)),
        _ => Err(format!(
            "interfaces[{index}].{name}.{key} must be a number for rnode_multi"
        )),
    }
}

fn validate_rnode_multi_airtime_limit(
    value: Option<f64>,
    key: &str,
    index: usize,
    subinterface: Option<&str>,
) -> Result<(), String> {
    let Some(value) = value else {
        return Ok(());
    };
    if !(0.0..=100.0).contains(&value) {
        if let Some(name) = subinterface {
            return Err(format!(
                "interfaces[{index}].{name}.{key} must be between 0 and 100 for rnode_multi"
            ));
        }
        return Err(format!(
            "interfaces[{index}].{key} must be between 0 and 100 for rnode_multi"
        ));
    }
    Ok(())
}
