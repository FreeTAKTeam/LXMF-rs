#[cfg(test)]
mod tests {
    use crate::buffer::InputBuffer;
    use crate::iface::auto::{AutoDiscoveryScope, MulticastAddressType};

    // Only this child can own sockets checked by immediate exclusive rebinds.
    #[cfg(unix)]
    fn auto_lifecycle_runs_in_child() -> bool {
        const MARKER: &str = "RETICULUM_AUTO_LIFECYCLE_CHILD";
        let case = std::thread::current().name().expect("exact test name").to_owned();
        if std::env::var_os(MARKER).as_deref() == Some(std::ffi::OsStr::new(&case)) {
            return false;
        }
        let output = std::process::Command::new(std::env::current_exe().expect("test binary"))
            .args([&case, "--exact", "--nocapture", "--test-threads=1"])
            .env(MARKER, &case)
            .output()
            .expect("isolated native Auto lifecycle test");
        assert!(
            output.status.success()
                && String::from_utf8_lossy(&output.stdout)
                    .contains("test result: ok. 1 passed; 0 failed; 0 ignored;"),
            "isolated native lifecycle case {case} failed or selected no test: {}\n{}\n{}",
            output.status,
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        true
    }

    include!("tests_sections/core_tests.rs");
    include!("tests/socket_lifecycle.rs");
    include!("tests_sections/auto_multicast_discovery_bind_resolv.rs");
    include!("tests_sections/auto_peer_data_transport_bridge_regi.rs");
    include!("tests_sections/auto_peer_route_origination.rs");
    #[cfg(target_os = "linux")]
    mod receive_shutdown {
        include!("tests/receive_shutdown.rs");
    }
}
