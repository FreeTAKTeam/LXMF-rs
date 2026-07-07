impl InterfaceConfig {

    fn validate_serial(&self, index: usize) -> Result<(), String> {
        self.reject_unknown_new_kind_keys(index, "serial")?;
        if !self.enabled() {
            return Ok(());
        }
        require_non_empty(
            self.device.as_deref(),
            &format!("interfaces[{index}].device is required for serial"),
        )?;
        if self.baud_rate.is_none() {
            return Err(format!("interfaces[{index}].baud_rate is required for serial"));
        }
        if self.baud_rate == Some(0) {
            return Err(format!("interfaces[{index}].baud_rate must be > 0 for serial"));
        }
        if let Some(data_bits) = self.data_bits {
            if !(5..=8).contains(&data_bits) {
                return Err(format!(
                    "interfaces[{index}].data_bits must be one of 5, 6, 7, 8 for serial"
                ));
            }
        }
        if let Some(stop_bits) = self.stop_bits {
            if stop_bits != 1 && stop_bits != 2 {
                return Err(format!(
                    "interfaces[{index}].stop_bits must be one of 1, 2 for serial"
                ));
            }
        }
        if let Some(parity) = self.parity.as_deref() {
            if !matches_normalized(parity, &["n", "none", "e", "even", "o", "odd"]) {
                return Err(format!(
                    "interfaces[{index}].parity must be one of n, none, e, even, o, odd for serial"
                ));
            }
        }
        if let Some(flow_control) = self.flow_control.as_ref() {
            let Some(flow_control) = flow_control.as_str() else {
                return Err(format!(
                    "interfaces[{index}].flow_control must be one of none, software, hardware for serial"
                ));
            };
            if !matches_normalized(flow_control, &["none", "software", "hardware"]) {
                return Err(format!(
                    "interfaces[{index}].flow_control must be one of none, software, hardware for serial"
                ));
            }
        }
        if let Some(mtu) = self.mtu {
            if !(256..=65535).contains(&mtu) {
                return Err(format!(
                    "interfaces[{index}].mtu must be between 256 and 65535 for serial"
                ));
            }
        }
        if let Some(reconnect_backoff_ms) = self.reconnect_backoff_ms {
            if reconnect_backoff_ms < 50 {
                return Err(format!(
                    "interfaces[{index}].reconnect_backoff_ms must be >= 50 for serial"
                ));
            }
        }
        if let (Some(reconnect_backoff_ms), Some(max_reconnect_backoff_ms)) =
            (self.reconnect_backoff_ms, self.max_reconnect_backoff_ms)
        {
            if max_reconnect_backoff_ms < reconnect_backoff_ms {
                return Err(format!(
                    "interfaces[{index}].max_reconnect_backoff_ms must be >= reconnect_backoff_ms for serial"
                ));
            }
        }
        Ok(())
    }

    fn validate_weave(&self, index: usize) -> Result<(), String> {
        self.reject_unknown_new_kind_keys(index, "weave")?;
        if !self.enabled() {
            return Ok(());
        }
        require_non_empty(
            self.device.as_deref(),
            &format!("interfaces[{index}].device or port is required for weave"),
        )?;
        if self.baud_rate.is_none() {
            return Err(format!("interfaces[{index}].baud_rate is required for weave"));
        }
        if self.baud_rate == Some(0) {
            return Err(format!("interfaces[{index}].baud_rate must be > 0 for weave"));
        }
        if let Some(mtu) = self.mtu {
            if !(256..=32768).contains(&mtu) {
                return Err(format!(
                    "interfaces[{index}].mtu must be between 256 and 32768 for weave"
                ));
            }
        }
        if let Some(reconnect_backoff_ms) = self.reconnect_backoff_ms {
            if reconnect_backoff_ms < 50 {
                return Err(format!(
                    "interfaces[{index}].reconnect_backoff_ms must be >= 50 for weave"
                ));
            }
        }
        if let (Some(reconnect_backoff_ms), Some(max_reconnect_backoff_ms)) =
            (self.reconnect_backoff_ms, self.max_reconnect_backoff_ms)
        {
            if max_reconnect_backoff_ms < reconnect_backoff_ms {
                return Err(format!(
                    "interfaces[{index}].max_reconnect_backoff_ms must be >= reconnect_backoff_ms for weave"
                ));
            }
        }
        Ok(())
    }

    fn validate_kiss(&self, index: usize) -> Result<(), String> {
        self.reject_unknown_new_kind_keys(index, "kiss")?;
        if !self.enabled() {
            return Ok(());
        }
        require_non_empty(
            self.device.as_deref(),
            &format!("interfaces[{index}].device is required for kiss"),
        )?;
        if self.baud_rate.is_none() {
            return Err(format!("interfaces[{index}].baud_rate is required for kiss"));
        }
        if self.baud_rate == Some(0) {
            return Err(format!("interfaces[{index}].baud_rate must be > 0 for kiss"));
        }
        if self.flow_control.as_ref().is_some_and(|value| !value.is_bool()) {
            return Err(format!("interfaces[{index}].flow_control must be a boolean for kiss"));
        }
        self.validate_id_beacon(index, "kiss")?;
        if let Some(mtu) = self.mtu {
            if !(64..=65535).contains(&mtu) {
                return Err(format!(
                    "interfaces[{index}].mtu must be between 64 and 65535 for kiss"
                ));
            }
        }
        if let Some(reconnect_backoff_ms) = self.reconnect_backoff_ms {
            if reconnect_backoff_ms < 50 {
                return Err(format!(
                    "interfaces[{index}].reconnect_backoff_ms must be >= 50 for kiss"
                ));
            }
        }
        if let (Some(reconnect_backoff_ms), Some(max_reconnect_backoff_ms)) =
            (self.reconnect_backoff_ms, self.max_reconnect_backoff_ms)
        {
            if max_reconnect_backoff_ms < reconnect_backoff_ms {
                return Err(format!(
                    "interfaces[{index}].max_reconnect_backoff_ms must be >= reconnect_backoff_ms for kiss"
                ));
            }
        }
        Ok(())
    }

    fn validate_ax25_kiss(&self, index: usize) -> Result<(), String> {
        self.reject_unknown_new_kind_keys(index, "ax25_kiss")?;
        if !self.enabled() {
            return Ok(());
        }
        require_non_empty(
            self.device.as_deref(),
            &format!("interfaces[{index}].device is required for ax25_kiss"),
        )?;
        if self.baud_rate.is_none() {
            return Err(format!("interfaces[{index}].baud_rate is required for ax25_kiss"));
        }
        if self.baud_rate == Some(0) {
            return Err(format!("interfaces[{index}].baud_rate must be > 0 for ax25_kiss"));
        }
        self.validate_id_beacon(index, "ax25_kiss")?;
        let callsign = self.callsign.as_deref().unwrap_or_default().trim();
        if !(3..=6).contains(&callsign.len())
            || !callsign.chars().all(|ch| ch.is_ascii_alphanumeric())
        {
            return Err(format!(
                "interfaces[{index}].callsign must be 3 to 6 ASCII alphanumeric characters for ax25_kiss"
            ));
        }
        if self.ssid.is_none_or(|ssid| ssid > 15) {
            return Err(format!("interfaces[{index}].ssid must be between 0 and 15 for ax25_kiss"));
        }
        if let Some(data_bits) = self.data_bits {
            if !(5..=8).contains(&data_bits) {
                return Err(format!(
                    "interfaces[{index}].data_bits must be one of 5, 6, 7, 8 for ax25_kiss"
                ));
            }
        }
        if let Some(stop_bits) = self.stop_bits {
            if stop_bits != 1 && stop_bits != 2 {
                return Err(format!(
                    "interfaces[{index}].stop_bits must be one of 1, 2 for ax25_kiss"
                ));
            }
        }
        if let Some(parity) = self.parity.as_deref() {
            if !matches_normalized(parity, &["n", "none", "e", "even", "o", "odd"]) {
                return Err(format!(
                    "interfaces[{index}].parity must be one of n, none, e, even, o, odd for ax25_kiss"
                ));
            }
        }
        if self.flow_control.as_ref().is_some_and(|value| !value.is_bool()) {
            return Err(format!("interfaces[{index}].flow_control must be a boolean for ax25_kiss"));
        }
        if let Some(mtu) = self.mtu {
            if !(64..=65535).contains(&mtu) {
                return Err(format!(
                    "interfaces[{index}].mtu must be between 64 and 65535 for ax25_kiss"
                ));
            }
        }
        if let Some(reconnect_backoff_ms) = self.reconnect_backoff_ms {
            if reconnect_backoff_ms < 50 {
                return Err(format!(
                    "interfaces[{index}].reconnect_backoff_ms must be >= 50 for ax25_kiss"
                ));
            }
        }
        if let (Some(reconnect_backoff_ms), Some(max_reconnect_backoff_ms)) =
            (self.reconnect_backoff_ms, self.max_reconnect_backoff_ms)
        {
            if max_reconnect_backoff_ms < reconnect_backoff_ms {
                return Err(format!(
                    "interfaces[{index}].max_reconnect_backoff_ms must be >= reconnect_backoff_ms for ax25_kiss"
                ));
            }
        }
        Ok(())
    }

    fn validate_kiss_tcp_client(&self, index: usize) -> Result<(), String> {
        self.reject_unknown_new_kind_keys(index, "kiss_tcp_client")?;
        if !self.enabled() {
            return Ok(());
        }
        require_non_empty(
            self.host.as_deref(),
            &format!("interfaces[{index}].host is required for kiss_tcp_client"),
        )?;
        if self.port.is_none() {
            return Err(format!("interfaces[{index}].port is required for kiss_tcp_client"));
        }
        if self.port == Some(0) {
            return Err(format!("interfaces[{index}].port must be > 0 for kiss_tcp_client"));
        }
        if self.flow_control.as_ref().is_some_and(|value| !value.is_bool()) {
            return Err(format!(
                "interfaces[{index}].flow_control must be a boolean for kiss_tcp_client"
            ));
        }
        self.validate_id_beacon(index, "kiss_tcp_client")?;
        if let Some(mtu) = self.mtu {
            if !(64..=65535).contains(&mtu) {
                return Err(format!(
                    "interfaces[{index}].mtu must be between 64 and 65535 for kiss_tcp_client"
                ));
            }
        }
        if let Some(reconnect_backoff_ms) = self.reconnect_backoff_ms {
            if reconnect_backoff_ms < 50 {
                return Err(format!(
                    "interfaces[{index}].reconnect_backoff_ms must be >= 50 for kiss_tcp_client"
                ));
            }
        }
        if let (Some(reconnect_backoff_ms), Some(max_reconnect_backoff_ms)) =
            (self.reconnect_backoff_ms, self.max_reconnect_backoff_ms)
        {
            if max_reconnect_backoff_ms < reconnect_backoff_ms {
                return Err(format!(
                    "interfaces[{index}].max_reconnect_backoff_ms must be >= reconnect_backoff_ms for kiss_tcp_client"
                ));
            }
        }
        Ok(())
    }

    fn validate_ble(&self, index: usize) -> Result<(), String> {
        self.reject_unknown_new_kind_keys(index, "ble_gatt")?;
        if !self.enabled() {
            return Ok(());
        }
        require_non_empty(
            self.peripheral_id.as_deref(),
            &format!("interfaces[{index}].peripheral_id is required for ble_gatt"),
        )?;
        require_non_empty(
            self.service_uuid.as_deref(),
            &format!("interfaces[{index}].service_uuid is required for ble_gatt"),
        )?;
        require_non_empty(
            self.write_char_uuid.as_deref(),
            &format!("interfaces[{index}].write_char_uuid is required for ble_gatt"),
        )?;
        require_non_empty(
            self.notify_char_uuid.as_deref(),
            &format!("interfaces[{index}].notify_char_uuid is required for ble_gatt"),
        )?;
        if let Some(adapter) = self.adapter.as_deref() {
            require_non_empty(
                Some(adapter),
                &format!("interfaces[{index}].adapter cannot be empty for ble_gatt"),
            )?;
        }
        let service_uuid = self.service_uuid.as_deref().unwrap_or_default();
        if !is_uuid_like(service_uuid) {
            return Err(format!(
                "interfaces[{index}].service_uuid must be a 16-, 32-, or 128-bit UUID for ble_gatt"
            ));
        }
        let write_char_uuid = self.write_char_uuid.as_deref().unwrap_or_default();
        if !is_uuid_like(write_char_uuid) {
            return Err(format!(
                "interfaces[{index}].write_char_uuid must be a 16-, 32-, or 128-bit UUID for ble_gatt"
            ));
        }
        let notify_char_uuid = self.notify_char_uuid.as_deref().unwrap_or_default();
        if !is_uuid_like(notify_char_uuid) {
            return Err(format!(
                "interfaces[{index}].notify_char_uuid must be a 16-, 32-, or 128-bit UUID for ble_gatt"
            ));
        }
        if let Some(scan_timeout_ms) = self.scan_timeout_ms {
            if scan_timeout_ms == 0 {
                return Err(format!(
                    "interfaces[{index}].scan_timeout_ms must be > 0 for ble_gatt"
                ));
            }
        }
        if let Some(connect_timeout_ms) = self.connect_timeout_ms {
            if connect_timeout_ms == 0 {
                return Err(format!(
                    "interfaces[{index}].connect_timeout_ms must be > 0 for ble_gatt"
                ));
            }
        }
        if let Some(mtu) = self.mtu {
            if !(23..=517).contains(&mtu) {
                return Err(format!(
                    "interfaces[{index}].mtu must be between 23 and 517 for ble_gatt"
                ));
            }
        }
        if let (Some(reconnect_backoff_ms), Some(max_reconnect_backoff_ms)) =
            (self.reconnect_backoff_ms, self.max_reconnect_backoff_ms)
        {
            if max_reconnect_backoff_ms < reconnect_backoff_ms {
                return Err(format!(
                    "interfaces[{index}].max_reconnect_backoff_ms must be >= reconnect_backoff_ms for ble_gatt"
                ));
            }
        }
        Ok(())
    }

    fn validate_reticulum_ble(&self, index: usize) -> Result<(), String> {
        self.reject_unknown_new_kind_keys(index, "reticulum_ble")?;
        if !self.enabled() {
            return Ok(());
        }
        if let Some(adapter) = self.adapter.as_deref() {
            require_non_empty(
                Some(adapter),
                &format!("interfaces[{index}].adapter cannot be empty for reticulum_ble"),
            )?;
        }
        for (field, value) in [
            ("service_uuid", self.service_uuid.as_deref()),
            ("tx_char_uuid", self.notify_char_uuid.as_deref()),
            ("rx_char_uuid", self.write_char_uuid.as_deref()),
            ("identity_char_uuid", self.identity_char_uuid.as_deref()),
        ] {
            let value = value.unwrap_or_default();
            if !is_uuid_like(value) {
                return Err(format!(
                    "interfaces[{index}].{field} must be a 16-, 32-, or 128-bit UUID for reticulum_ble"
                ));
            }
        }
        if let Some(mtu) = self.mtu {
            if !(23..=517).contains(&mtu) {
                return Err(format!(
                    "interfaces[{index}].mtu must be between 23 and 517 for reticulum_ble"
                ));
            }
        }
        if self.max_connections.is_some_and(|value| value == 0) {
            return Err(format!(
                "interfaces[{index}].max_connections must be > 0 for reticulum_ble"
            ));
        }
        if self.min_rssi_dbm.is_some_and(|value| !(-127..=20).contains(&value)) {
            return Err(format!(
                "interfaces[{index}].min_rssi_dbm must be between -127 and 20 for reticulum_ble"
            ));
        }
        for (field, value) in [
            ("scan_duration_ms", self.scan_duration_ms),
            ("discovery_interval_ms", self.discovery_interval_ms),
            ("discovery_interval_idle_ms", self.discovery_interval_idle_ms),
            ("advertising_refresh_interval_ms", self.advertising_refresh_interval_ms),
        ] {
            if value == Some(0) {
                return Err(format!("interfaces[{index}].{field} must be > 0 for reticulum_ble"));
            }
        }
        if self.enable_central == Some(false) && self.enable_peripheral == Some(false) {
            return Err(format!(
                "interfaces[{index}].enable_central and enable_peripheral cannot both be false for reticulum_ble"
            ));
        }
        Ok(())
    }

    fn validate_vrn76_kiss_ble(&self, index: usize) -> Result<(), String> {
        self.reject_unknown_new_kind_keys(index, "vrn76_kiss_ble")?;
        if let Some(flow_control) = self.flow_control.as_ref() {
            if !flow_control.is_bool() {
                return Err(format!(
                    "interfaces[{index}].flow_control must be a boolean for vrn76_kiss_ble"
                ));
            }
        }
        if let Some(frame_mode) = self.frame_mode.as_deref() {
            if !matches_vrn76_frame_mode(frame_mode) {
                return Err(format!(
                    "interfaces[{index}].frame_mode must be one of benshi_tnc_data, benshi, raw_kiss, raw for vrn76_kiss_ble"
                ));
            }
        }
        self.validate_id_beacon(index, "vrn76_kiss_ble")?;
        if !self.enabled() {
            return Ok(());
        }
        require_non_empty(
            self.peripheral_id.as_deref(),
            &format!("interfaces[{index}].peripheral_id is required for vrn76_kiss_ble"),
        )?;
        if let Some(adapter) = self.adapter.as_deref() {
            require_non_empty(
                Some(adapter),
                &format!("interfaces[{index}].adapter cannot be empty for vrn76_kiss_ble"),
            )?;
        }
        if let Some(scan_timeout_ms) = self.scan_timeout_ms {
            if scan_timeout_ms == 0 {
                return Err(format!(
                    "interfaces[{index}].scan_timeout_ms must be > 0 for vrn76_kiss_ble"
                ));
            }
        }
        if let Some(connect_timeout_ms) = self.connect_timeout_ms {
            if connect_timeout_ms == 0 {
                return Err(format!(
                    "interfaces[{index}].connect_timeout_ms must be > 0 for vrn76_kiss_ble"
                ));
            }
        }
        if let Some(mtu) = self.mtu {
            if !(64..=65535).contains(&mtu) {
                return Err(format!(
                    "interfaces[{index}].mtu must be between 64 and 65535 for vrn76_kiss_ble"
                ));
            }
        }
        if let Some(max_write_len) = self.max_write_len {
            if !(6..=65535).contains(&max_write_len) {
                return Err(format!(
                    "interfaces[{index}].max_write_len must be between 6 and 65535 for vrn76_kiss_ble"
                ));
            }
        }
        if let Some(reconnect_backoff_ms) = self.reconnect_backoff_ms {
            if reconnect_backoff_ms < 50 {
                return Err(format!(
                    "interfaces[{index}].reconnect_backoff_ms must be >= 50 for vrn76_kiss_ble"
                ));
            }
        }
        if let (Some(reconnect_backoff_ms), Some(max_reconnect_backoff_ms)) =
            (self.reconnect_backoff_ms, self.max_reconnect_backoff_ms)
        {
            if max_reconnect_backoff_ms < reconnect_backoff_ms {
                return Err(format!(
                    "interfaces[{index}].max_reconnect_backoff_ms must be >= reconnect_backoff_ms for vrn76_kiss_ble"
                ));
            }
        }
        Ok(())
    }
}
