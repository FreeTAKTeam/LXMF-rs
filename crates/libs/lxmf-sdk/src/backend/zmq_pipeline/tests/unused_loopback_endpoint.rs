fn unused_loopback_endpoint() -> String {
    static NEXT_OFFSET: std::sync::OnceLock<std::sync::atomic::AtomicU32> =
        std::sync::OnceLock::new();
    static USED_PORTS: std::sync::OnceLock<
        std::sync::Mutex<std::collections::HashSet<u16>>,
    > = std::sync::OnceLock::new();

    loop {
        // Keep separate nextest processes from selecting the same port while
        // the server thread is still racing to bind it. The probe below still
        // rejects ports occupied by unrelated processes.
        let offset = NEXT_OFFSET
            .get_or_init(|| std::sync::atomic::AtomicU32::new(0))
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let pid = std::process::id();
        let port = 10_000 + ((pid.wrapping_mul(97).wrapping_add(offset)) % 50_000) as u16;
        if let Ok(listener) = std::net::TcpListener::bind(("127.0.0.1", port)) {
            drop(listener);
            if USED_PORTS
                .get_or_init(|| std::sync::Mutex::new(std::collections::HashSet::new()))
                .lock()
                .expect("used ports")
                .insert(port)
            {
                return format!("tcp://localhost:{port}");
            }
        }
    }
}
