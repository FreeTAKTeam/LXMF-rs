#[cfg(test)]
mod tests {
    use super::{
        run_startup_lifecycle, runtime_settings, stage_result, BleBackend, BleBackendError,
        BleLifecycleOutcome, BleLifecyclePhase, BleRuntimeSettings, BLE_STARTUP_MAX_RETRY_ATTEMPTS,
    };
    use reticulum_daemon::config::InterfaceConfig;
    use std::time::Duration;

    #[derive(Clone)]
    struct PlannedFailure {
        phase: BleLifecyclePhase,
        attempts_remaining: u32,
        retryable: bool,
        message: &'static str,
    }

    #[derive(Default)]
    struct MockBackend {
        planned_failures: Vec<PlannedFailure>,
        notification_payload_override: Option<Vec<u8>>,
        last_probe_payload: Vec<u8>,
    }

    impl MockBackend {
        fn maybe_fail(&mut self, phase: BleLifecyclePhase) -> Option<BleBackendError> {
            self.planned_failures.iter_mut().find_map(|failure| {
                if failure.phase == phase && failure.attempts_remaining > 0 {
                    failure.attempts_remaining -= 1;
                    Some(if failure.retryable {
                        BleBackendError::retryable(failure.message)
                    } else {
                        BleBackendError::terminal(failure.message)
                    })
                } else {
                    None
                }
            })
        }
    }

    impl BleBackend for MockBackend {
        fn backend_name(&self) -> &'static str {
            "mock"
        }

        async fn scan(&mut self, _settings: &BleRuntimeSettings) -> Result<(), BleBackendError> {
            if let Some(err) = self.maybe_fail(BleLifecyclePhase::Scan) {
                return Err(err);
            }
            Ok(())
        }

        async fn connect(&mut self, _settings: &BleRuntimeSettings) -> Result<(), BleBackendError> {
            if let Some(err) = self.maybe_fail(BleLifecyclePhase::Connect) {
                return Err(err);
            }
            Ok(())
        }

        async fn subscribe(
            &mut self,
            _settings: &BleRuntimeSettings,
        ) -> Result<(), BleBackendError> {
            if let Some(err) = self.maybe_fail(BleLifecyclePhase::Subscribe) {
                return Err(err);
            }
            Ok(())
        }

        async fn write_probe(
            &mut self,
            payload: &[u8],
            _settings: &BleRuntimeSettings,
        ) -> Result<(), BleBackendError> {
            if let Some(err) = self.maybe_fail(BleLifecyclePhase::WriteProbe) {
                return Err(err);
            }
            self.last_probe_payload = payload.to_vec();
            Ok(())
        }

        async fn read_probe_notification(
            &mut self,
            _settings: &BleRuntimeSettings,
        ) -> Result<Vec<u8>, BleBackendError> {
            if let Some(err) = self.maybe_fail(BleLifecyclePhase::NotificationProbe) {
                return Err(err);
            }
            Ok(self
                .notification_payload_override
                .clone()
                .unwrap_or_else(|| self.last_probe_payload.clone()))
        }
    }

    fn ble_iface() -> InterfaceConfig {
        InterfaceConfig {
            kind: "ble_gatt".to_string(),
            enabled: Some(true),
            peripheral_id: Some("AA:BB:CC:DD:EE:FF".to_string()),
            service_uuid: Some("12345678-1234-1234-1234-1234567890ab".to_string()),
            write_char_uuid: Some("2A37".to_string()),
            notify_char_uuid: Some("2A38".to_string()),
            ..InterfaceConfig::default()
        }
    }

    #[test]
    fn runtime_settings_use_safe_defaults() {
        let iface = ble_iface();
        let settings = runtime_settings(&iface).expect("runtime settings");
        assert_eq!(settings.mtu, 247);
        assert_eq!(settings.scan_timeout.as_millis(), 5_000);
        assert_eq!(settings.connect_timeout.as_millis(), 10_000);
        assert_eq!(settings.reconnect_backoff.as_millis(), 500);
        assert_eq!(settings.max_reconnect_backoff.as_millis(), 5_000);
    }

    #[test]
    fn runtime_settings_rejects_max_backoff_below_base() {
        let mut iface = ble_iface();
        iface.reconnect_backoff_ms = Some(5_000);
        iface.max_reconnect_backoff_ms = Some(100);
        let err = runtime_settings(&iface).expect_err("backoff bounds should fail");
        assert!(err.contains("max_reconnect_backoff_ms"));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn ble_lifecycle_transitions_cover_scan_connect_subscribe_and_probe() {
        let settings = runtime_settings(&ble_iface()).expect("runtime settings");
        let mut backend = MockBackend::default();

        let report =
            run_startup_lifecycle(&mut backend, &settings).await.expect("lifecycle report");

        assert_eq!(report.attempts, 1);
        assert_eq!(report.transitions.len(), 5);
        assert_eq!(report.transitions[0].phase, BleLifecyclePhase::Scan);
        assert_eq!(report.transitions[1].phase, BleLifecyclePhase::Connect);
        assert_eq!(report.transitions[2].phase, BleLifecyclePhase::Subscribe);
        assert_eq!(report.transitions[3].phase, BleLifecyclePhase::WriteProbe);
        assert_eq!(report.transitions[4].phase, BleLifecyclePhase::NotificationProbe);
        assert!(report
            .transitions
            .iter()
            .all(|transition| transition.outcome == BleLifecycleOutcome::Ok));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn ble_lifecycle_retries_on_retryable_connect_failures_with_bounded_backoff() {
        let settings = runtime_settings(&ble_iface()).expect("runtime settings");
        let mut backend = MockBackend {
            planned_failures: vec![PlannedFailure {
                phase: BleLifecyclePhase::Connect,
                attempts_remaining: 2,
                retryable: true,
                message: "mock connect retryable failure",
            }],
            ..Default::default()
        };

        let report =
            run_startup_lifecycle(&mut backend, &settings).await.expect("lifecycle report");
        assert_eq!(report.attempts, 3);
        assert!(report
            .transitions
            .iter()
            .any(|transition| transition.phase == BleLifecyclePhase::Connect
                && transition.outcome == BleLifecycleOutcome::Retry));
        assert!(report
            .transitions
            .iter()
            .filter_map(|transition| transition.backoff_ms)
            .all(|backoff_ms| backoff_ms <= settings.max_reconnect_backoff.as_millis() as u64));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn ble_lifecycle_fails_after_retry_budget_exhaustion() {
        let settings = runtime_settings(&ble_iface()).expect("runtime settings");
        let mut backend = MockBackend {
            planned_failures: vec![PlannedFailure {
                phase: BleLifecyclePhase::Connect,
                attempts_remaining: BLE_STARTUP_MAX_RETRY_ATTEMPTS + 1,
                retryable: true,
                message: "mock connect retryable failure",
            }],
            ..Default::default()
        };

        let err = run_startup_lifecycle(&mut backend, &settings)
            .await
            .expect_err("retryable failures should exhaust startup attempts");
        assert!(err.contains("phase=connect"));
        assert!(err.contains(&format!("attempt={BLE_STARTUP_MAX_RETRY_ATTEMPTS}")));
    }

    #[test]
    fn stage_result_marks_retryable_error_as_failed_when_retry_budget_is_exhausted() {
        let mut transitions = Vec::new();

        let err = stage_result(
            "mock",
            BLE_STARTUP_MAX_RETRY_ATTEMPTS,
            BleLifecyclePhase::Connect,
            Duration::from_millis(250),
            false,
            &mut transitions,
            Err(BleBackendError::retryable("mock retryable exhaustion")),
        )
        .expect("stage should return error");

        assert!(err.retryable);
        assert_eq!(transitions.len(), 1);
        assert_eq!(transitions[0].outcome, BleLifecycleOutcome::Failed);
        assert_eq!(transitions[0].backoff_ms, None);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn ble_lifecycle_terminal_subscribe_failure_is_not_retried() {
        let settings = runtime_settings(&ble_iface()).expect("runtime settings");
        let mut backend = MockBackend {
            planned_failures: vec![PlannedFailure {
                phase: BleLifecyclePhase::Subscribe,
                attempts_remaining: 1,
                retryable: false,
                message: "mock subscribe terminal failure",
            }],
            ..Default::default()
        };

        let err = run_startup_lifecycle(&mut backend, &settings)
            .await
            .expect_err("terminal subscribe failures should fail immediately");
        assert!(err.contains("phase=subscribe"));
        assert!(err.contains("attempt=1"));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn ble_lifecycle_terminal_notification_failure_is_not_retried() {
        let settings = runtime_settings(&ble_iface()).expect("runtime settings");
        let mut backend = MockBackend {
            planned_failures: vec![PlannedFailure {
                phase: BleLifecyclePhase::NotificationProbe,
                attempts_remaining: 1,
                retryable: false,
                message: "mock notification terminal failure",
            }],
            ..Default::default()
        };

        let err = run_startup_lifecycle(&mut backend, &settings)
            .await
            .expect_err("terminal notification failures should fail immediately");
        assert!(err.contains("phase=notification_probe"));
        assert!(err.contains("attempt=1"));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn ble_lifecycle_roundtrip_rejects_mismatched_notification_payload() {
        let settings = runtime_settings(&ble_iface()).expect("runtime settings");
        let mut backend = MockBackend {
            notification_payload_override: Some(vec![1, 2, 3]),
            ..Default::default()
        };

        let err = run_startup_lifecycle(&mut backend, &settings)
            .await
            .expect_err("mismatched probe payload should fail lifecycle");
        assert!(err.contains("probe payload mismatch"));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn ble_lifecycle_reports_likely_att_mtu_truncation() {
        let settings = runtime_settings(&ble_iface()).expect("runtime settings");
        let mut backend = MockBackend {
            notification_payload_override: Some(vec![0x42; 20]),
            ..Default::default()
        };

        let err = run_startup_lifecycle(&mut backend, &settings)
            .await
            .expect_err("20-byte notification should fail lifecycle with MTU diagnostic");

        assert!(err.contains("likely ATT MTU 23"));
        assert!(err.contains("btleplug cannot request a larger MTU"));
    }
}
