async fn read_http_request<S>(stream: &mut S) -> io::Result<Vec<u8>>
where
    S: AsyncRead + Unpin,
{
    read_http_request_with_timeout(stream, RPC_REQUEST_TIMEOUT).await
}

async fn read_http_request_with_timeout<S>(
    stream: &mut S,
    request_timeout: Duration,
) -> io::Result<Vec<u8>>
where
    S: AsyncRead + Unpin,
{
    timeout(request_timeout, read_http_request_inner(stream))
        .await
        .map_err(|_| io::Error::new(io::ErrorKind::TimedOut, "rpc read timed out"))?
}

async fn read_http_request_inner<S>(stream: &mut S) -> io::Result<Vec<u8>>
where
    S: AsyncRead + Unpin,
{
    let mut buffer = Vec::new();
    let mut expected_len: Option<usize> = None;

    loop {
        let mut chunk = [0_u8; 4096];
        let read = stream.read(&mut chunk).await?;
        if read == 0 {
            break;
        }
        buffer.extend_from_slice(&chunk[..read]);

        if let Some(header_end) = http::find_header_end(&buffer) {
            if header_end > RPC_MAX_HEADER_BYTES {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "rpc headers exceed maximum size",
                ));
            }
            let headers = &buffer[..header_end];
            let content_length = http::parse_content_length(headers).unwrap_or(0);
            if content_length > RPC_MAX_BODY_BYTES {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "rpc body exceeds maximum size",
                ));
            }
            let total_len = header_end
                .checked_add(4)
                .and_then(|body_start| body_start.checked_add(content_length))
                .ok_or_else(|| {
                    io::Error::new(io::ErrorKind::InvalidData, "rpc request too large")
                })?;
            expected_len = Some(total_len);
            if buffer.len() >= total_len {
                break;
            }
        } else if buffer.len() > RPC_MAX_HEADER_BYTES {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "rpc headers exceed maximum size",
            ));
        }

        if let Some(total_len) = expected_len {
            if buffer.len() > total_len {
                break;
            }
        }
    }

    Ok(buffer)
}

fn request_read_error_response(error: &io::Error) -> &'static [u8] {
    match error.kind() {
        io::ErrorKind::TimedOut => {
            b"HTTP/1.1 408 Request Timeout\r\nContent-Length: 18\r\n\r\nrpc read timed out"
        }
        _ => b"HTTP/1.1 400 Bad Request\r\nContent-Length: 19\r\n\r\ninvalid rpc request",
    }
}

fn build_tls_server_config(config: &RpcTlsConfig) -> io::Result<std::sync::Arc<ServerConfig>> {
    let server_chain = load_cert_chain(config.cert_chain_path.as_path())?;
    let private_key = load_private_key(config.private_key_path.as_path())?;

    let builder = ServerConfig::builder();
    let server_config = if let Some(client_ca_path) = config.client_ca_path.as_ref() {
        let roots = load_root_store(client_ca_path.as_path())?;
        let verifier =
            WebPkiClientVerifier::builder(std::sync::Arc::new(roots)).build().map_err(|err| {
                io::Error::new(
                    io::ErrorKind::InvalidInput,
                    format!(
                        "failed to build client verifier from {}: {}",
                        client_ca_path.display(),
                        err
                    ),
                )
            })?;
        builder
            .with_client_cert_verifier(verifier)
            .with_single_cert(server_chain, private_key)
            .map_err(|err| {
                io::Error::new(
                    io::ErrorKind::InvalidInput,
                    format!("invalid rpc tls server certificate/key configuration: {}", err),
                )
            })?
    } else {
        builder.with_no_client_auth().with_single_cert(server_chain, private_key).map_err(
            |err| {
                io::Error::new(
                    io::ErrorKind::InvalidInput,
                    format!("invalid rpc tls server certificate/key configuration: {}", err),
                )
            },
        )?
    };

    Ok(std::sync::Arc::new(server_config))
}

fn load_cert_chain(path: &Path) -> io::Result<Vec<rustls::pki_types::CertificateDer<'static>>> {
    let file = File::open(path)?;
    let certificates = rustls::pki_types::CertificateDer::pem_reader_iter(file)
        .collect::<Result<Vec<_>, _>>()
        .map_err(|err| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                format!("failed to parse PEM certs from {}: {}", path.display(), err),
            )
        })?;
    if certificates.is_empty() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("no certificates found in {}", path.display()),
        ));
    }
    Ok(certificates)
}

fn load_private_key(path: &Path) -> io::Result<rustls::pki_types::PrivateKeyDer<'static>> {
    let file = File::open(path)?;
    let key = rustls::pki_types::PrivateKeyDer::pem_reader_iter(file)
        .next()
        .transpose()
        .map_err(|err| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                format!("failed to parse private key {}: {}", path.display(), err),
            )
        })?;
    key.ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!("no private key found in {}", path.display()),
        )
    })
}

fn load_root_store(path: &Path) -> io::Result<RootCertStore> {
    let certificates = load_cert_chain(path)?;
    let mut roots = RootCertStore::empty();
    let (added, _ignored) = roots.add_parsable_certificates(certificates);
    if added == 0 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("no valid CA certificates found in {}", path.display()),
        ));
    }
    Ok(roots)
}

fn extract_transport_auth(stream: &TlsStream<TcpStream>) -> http::TransportAuthContext {
    let mut context = http::TransportAuthContext::default();
    let (_tcp_stream, session) = stream.get_ref();
    let Some(peer_certs) = session.peer_certificates() else {
        return context;
    };
    let Some(leaf) = peer_certs.first() else {
        return context;
    };
    context.client_cert_present = true;
    let (subject, sans) = parse_client_identity(leaf.as_ref());
    context.client_subject = subject;
    context.client_sans = sans;
    context
}

fn parse_client_identity(cert_der: &[u8]) -> (Option<String>, Vec<String>) {
    let Ok((_remaining, cert)) = X509Certificate::from_der(cert_der) else {
        return (None, Vec::new());
    };
    let subject = cert
        .subject()
        .iter_common_name()
        .find_map(|name| name.as_str().ok().map(str::to_string))
        .or_else(|| Some(cert.subject().to_string()));
    let sans = parse_subject_alt_names(&cert);
    (subject, sans)
}

fn parse_subject_alt_names(cert: &X509Certificate<'_>) -> Vec<String> {
    let mut sans = Vec::new();
    for extension in cert.extensions() {
        if let ParsedExtension::SubjectAlternativeName(subject_alt_name) =
            extension.parsed_extension()
        {
            for name in &subject_alt_name.general_names {
                let value = match name {
                    GeneralName::DNSName(value) => Some((*value).to_string()),
                    GeneralName::URI(value) => Some((*value).to_string()),
                    GeneralName::RFC822Name(value) => Some((*value).to_string()),
                    GeneralName::IPAddress(raw) if raw.len() == 4 => {
                        Some(IpAddr::from([raw[0], raw[1], raw[2], raw[3]]).to_string())
                    }
                    GeneralName::IPAddress(raw) if raw.len() == 16 => {
                        let mut octets = [0_u8; 16];
                        octets.copy_from_slice(raw);
                        Some(IpAddr::from(octets).to_string())
                    }
                    _ => None,
                };
                if let Some(value) = value {
                    let value = value.trim();
                    if !value.is_empty() {
                        sans.push(value.to_string());
                    }
                }
            }
        }
    }
    sans
}

#[cfg(test)]
mod rpc_loop_tests {
    use super::*;
    use tokio::io::{duplex, AsyncReadExt, AsyncWriteExt};
    #[cfg(unix)]
    use tokio::net::UnixStream;

    #[test]
    fn rpc_ready_line_matches_e2e_harness_marker() {
        let line = rpc_ready_line("http", "127.0.0.1:4242");

        assert!(line.contains("listening on http://127.0.0.1:4242"));
    }

    #[test]
    fn tls_pem_loaders_accept_certificate_and_private_key_sections() {
        let temp = tempfile::tempdir().expect("tempdir");
        let cert_path = temp.path().join("certificate.pem");
        let key_path = temp.path().join("private-key.pem");
        std::fs::write(
            &cert_path,
            b"-----BEGIN CERTIFICATE-----\nAQID\n-----END CERTIFICATE-----\n",
        )
        .expect("write certificate");
        std::fs::write(
            &key_path,
            b"-----BEGIN PRIVATE KEY-----\nAQID\n-----END PRIVATE KEY-----\n",
        )
        .expect("write private key");

        let certificates = load_cert_chain(&cert_path).expect("load certificate");
        let private_key = load_private_key(&key_path).expect("load private key");

        assert_eq!(certificates.len(), 1);
        assert_eq!(certificates[0].as_ref(), &[1, 2, 3]);
        assert_eq!(private_key.secret_der(), &[1, 2, 3]);
    }

    #[test]
    fn tls_pem_loaders_report_files_without_matching_sections() {
        let temp = tempfile::tempdir().expect("tempdir");
        let path = temp.path().join("empty.pem");
        std::fs::write(&path, b"").expect("write empty PEM file");

        let cert_error = load_cert_chain(&path).expect_err("empty certificate file rejected");
        let key_error = load_private_key(&path).expect_err("empty private key file rejected");

        assert!(cert_error.to_string().contains("no certificates found"));
        assert!(key_error.to_string().contains("no private key found"));
    }

    #[test]
    fn read_http_request_collects_complete_post_body() {
        let runtime =
            tokio::runtime::Builder::new_current_thread().enable_all().build().expect("runtime");
        runtime.block_on(async {
            let (mut client, mut server) = duplex(4096);
            let body = b"hello";
            let request = format!(
                "POST /rpc HTTP/1.1\r\nHost: localhost\r\nContent-Length: {}\r\n\r\n",
                body.len()
            );
            let writer = tokio::spawn(async move {
                client.write_all(request.as_bytes()).await.expect("write headers");
                client.write_all(body).await.expect("write body");
            });

            let raw = read_http_request(&mut server).await.expect("read request");
            writer.await.expect("join writer");
            assert!(raw.ends_with(body));
        });
    }

    #[test]
    fn read_http_request_rejects_oversized_body() {
        let runtime =
            tokio::runtime::Builder::new_current_thread().enable_all().build().expect("runtime");
        runtime.block_on(async {
            let (mut client, mut server) = duplex(4096);
            let request = format!(
                "POST /rpc HTTP/1.1\r\nHost: localhost\r\nContent-Length: {}\r\n\r\n",
                RPC_MAX_BODY_BYTES + 1
            );
            client.write_all(request.as_bytes()).await.expect("write headers");

            let err = read_http_request(&mut server).await.expect_err("oversized request");
            assert_eq!(err.kind(), io::ErrorKind::InvalidData);
            assert!(err.to_string().contains("maximum size"));
        });
    }

    #[test]
    fn read_http_request_enforces_total_deadline_across_reads() {
        let runtime =
            tokio::runtime::Builder::new_current_thread().enable_all().build().expect("runtime");
        runtime.block_on(async {
            let (mut client, mut server) = duplex(4096);
            let writer = tokio::spawn(async move {
                for byte in b"GET /rpc HTTP/1.1\r\n" {
                    if client.write_all(&[*byte]).await.is_err() {
                        break;
                    }
                    tokio::time::sleep(Duration::from_millis(40)).await;
                }
            });

            let err =
                read_http_request_with_timeout(&mut server, Duration::from_millis(75))
                    .await
                    .expect_err("slow request should exceed its total deadline");
            assert_eq!(err.kind(), io::ErrorKind::TimedOut);
            writer.abort();
        });
    }

    #[test]
    fn remote_connection_budget_recovers_when_a_connection_finishes() {
        let permits = Arc::new(Semaphore::new(1));
        let permit =
            reserve_remote_connection(&permits).expect("first connection should be admitted");
        assert!(
            reserve_remote_connection(&permits).is_err(),
            "connection above the configured limit should be rejected"
        );

        drop(permit);
        assert!(
            reserve_remote_connection(&permits).is_ok(),
            "finishing a connection should restore listener capacity"
        );
    }

    #[test]
    fn event_stream_invalid_cursor_returns_framed_rpc_error() {
        let runtime =
            tokio::runtime::Builder::new_current_thread().enable_all().build().expect("runtime");
        runtime.block_on(async {
            let daemon = RpcDaemon::test_instance();
            let (mut client, server) = duplex(4096);
            let peer_addr = SocketAddr::from(([127, 0, 0, 1], 4242));

            let server_task = tokio::spawn(async move {
                handle_event_stream(
                    server,
                    peer_addr,
                    &daemon,
                    "/events/stream?cursor=bad-cursor".to_string(),
                    Vec::new(),
                    None,
                )
                .await;
            });

            let mut response = Vec::new();
            client.read_to_end(&mut response).await.expect("read stream rejection");
            server_task.await.expect("event stream task");
            assert!(response.starts_with(b"HTTP/1.1 200 OK\r\n"));
            let header_end = http::find_header_end(&response).expect("response header end");
            let frame = &response[(header_end + 4)..];
            let rpc_response =
                codec::decode_frame::<RpcResponse>(frame).expect("decode framed rpc error");
            let error = rpc_response.error.expect("rpc error");
            assert_eq!(error.code, "SDK_RUNTIME_INVALID_CURSOR");
            assert_eq!(error.machine_code.as_deref(), Some("SDK_RUNTIME_INVALID_CURSOR"));
        });
    }

    #[cfg(unix)]
    #[test]
    fn prepare_rpc_unix_socket_path_refuses_regular_file() {
        let temp = tempfile::tempdir().expect("tempdir");
        let path = temp.path().join("reticulumd.sock");
        std::fs::write(&path, b"not a socket").expect("write regular file");

        let err = prepare_rpc_unix_socket_path(&path).expect_err("regular file rejected");
        assert_eq!(err.kind(), io::ErrorKind::AlreadyExists);
        assert!(err.to_string().contains("non-socket"));
        assert!(path.exists());
    }

    #[cfg(unix)]
    #[test]
    fn prepare_rpc_unix_socket_path_refuses_symlink() {
        use std::os::unix::fs::symlink;

        let temp = tempfile::tempdir().expect("tempdir");
        let target = temp.path().join("target");
        let path = temp.path().join("reticulumd.sock");
        std::fs::write(&target, b"target").expect("write target");
        symlink(&target, &path).expect("create symlink");

        let err = prepare_rpc_unix_socket_path(&path).expect_err("symlink rejected");
        assert_eq!(err.kind(), io::ErrorKind::AlreadyExists);
        assert!(path.is_symlink());
        assert!(target.exists());
    }

    #[cfg(unix)]
    #[test]
    fn rpc_unix_socket_is_private() {
        use std::os::unix::fs::PermissionsExt;

        let runtime =
            tokio::runtime::Builder::new_current_thread().enable_all().build().expect("runtime");
        runtime.block_on(async {
            let temp = tempfile::tempdir().expect("tempdir");
            let path = temp.path().join("reticulumd.sock");
            let listener = bind_private_rpc_unix_listener(&path).expect("bind private socket");

            let mode =
                std::fs::metadata(&path).expect("socket metadata").permissions().mode() & 0o777;
            assert_eq!(mode, RPC_UNIX_SOCKET_MODE);

            drop(listener);
            cleanup_rpc_unix_socket_path(&path).expect("cleanup socket");
        });
    }

    #[cfg(unix)]
    #[test]
    fn rpc_unix_socket_is_private_at_creation_under_permissive_umask() {
        const CHILD_MARKER: &str = "RETICULUMD_RPC_UMASK_TEST_CHILD";

        if std::env::var_os(CHILD_MARKER).is_none() {
            let executable = std::env::current_exe().expect("current test executable");
            let status = std::process::Command::new("sh")
                .arg("-c")
                .arg("umask 000; exec \"$1\" \"$2\" --nocapture")
                .arg("sh")
                .arg(executable)
                .arg("rpc_unix_socket_is_private_at_creation_under_permissive_umask")
                .env(CHILD_MARKER, "1")
                .status()
                .expect("run permissive-umask test child");
            assert!(status.success(), "permissive-umask test child failed");
            return;
        }

        use std::os::unix::fs::PermissionsExt;

        let runtime =
            tokio::runtime::Builder::new_current_thread().enable_all().build().expect("runtime");
        runtime.block_on(async {
            let temp = tempfile::tempdir().expect("tempdir");
            let path = temp.path().join("reticulumd.sock");
            let listener = bind_private_rpc_unix_listener_with_pre_publish(
                &path,
                |staging_path| {
                    assert!(
                        !path.exists(),
                        "final socket must not exist before private publication"
                    );
                    assert!(
                        std::os::unix::net::UnixStream::connect(&path).is_err(),
                        "final socket must not be connectable before private publication"
                    );
                    let socket_mode =
                        std::fs::metadata(staging_path)?.permissions().mode() & 0o777;
                    assert_eq!(socket_mode, RPC_UNIX_SOCKET_MODE);
                    let staging_mode =
                        std::fs::metadata(staging_path.parent().expect("staging parent"))?
                            .permissions()
                            .mode()
                            & 0o777;
                    assert_eq!(staging_mode, RPC_UNIX_STAGING_DIR_MODE);
                    Ok(())
                },
            )
            .expect("bind private socket");

            let mode =
                std::fs::metadata(&path).expect("socket metadata").permissions().mode() & 0o777;
            assert_eq!(mode, RPC_UNIX_SOCKET_MODE);
            std::os::unix::net::UnixStream::connect(&path)
                .expect("connect after private publication");

            drop(listener);
            cleanup_rpc_unix_socket_path(&path).expect("cleanup socket");
        });
    }

    #[cfg(unix)]
    #[test]
    fn unix_rpc_loop_removes_stale_socket_and_cleans_up_on_shutdown() {
        let runtime =
            tokio::runtime::Builder::new_current_thread().enable_all().build().expect("runtime");
        runtime.block_on(async {
            let temp = tempfile::tempdir().expect("tempdir");
            let path = temp.path().join("reticulumd.sock");
            let stale_listener = UnixListener::bind(&path).expect("bind stale socket");
            drop(stale_listener);
            assert!(path.exists());

            let daemon = Arc::new(RpcDaemon::test_instance());
            let (shutdown_tx, shutdown_rx) = watch::channel(false);
            let task_path = path.clone();
            let task =
                tokio::spawn(
                    async move { run_unix_rpc_loop(task_path, daemon, shutdown_rx).await },
                );

            assert!(wait_for_unix_connect(&path).await, "unix listener did not become ready");
            shutdown_tx.send(true).expect("send shutdown");
            timeout(Duration::from_secs(2), task)
                .await
                .expect("unix loop shutdown timeout")
                .expect("join unix loop");
            assert!(wait_for_path_removed(&path).await, "unix socket was not cleaned up");
        });
    }

    #[cfg(unix)]
    async fn wait_for_unix_connect(path: &Path) -> bool {
        for _ in 0..50 {
            if UnixStream::connect(path).await.is_ok() {
                return true;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        false
    }

    #[cfg(unix)]
    async fn wait_for_path_removed(path: &Path) -> bool {
        for _ in 0..50 {
            if !path.exists() {
                return true;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        false
    }
}
