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
        if self.state_path.as_deref().map(str::trim).filter(|value| !value.is_empty()).is_none() {
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
            if !(1..=255).contains(&max_payload_bytes) {
                return Err(format!(
                    "interfaces[{index}].max_payload_bytes must be between 1 and 255 for lora"
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
