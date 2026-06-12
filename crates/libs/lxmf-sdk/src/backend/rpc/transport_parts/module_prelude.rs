use super::*;

use hmac::{Hmac, Mac};

use rns_rpc::e2e_harness::{build_rpc_frame, parse_http_response_body, parse_rpc_frame};

use rns_rpc::rpc::{codec, http};

use rns_rpc::{RpcError, RpcResponse};

use rustls::pki_types::ServerName;

use rustls::{ClientConfig, RootCertStore};

use rustls_pemfile::private_key;

use sha2::Sha256;

use std::fs::File;

use std::io::{self, BufReader, Read, Write};

use std::net::{IpAddr, Shutdown, TcpStream};

#[cfg(unix)]
use std::os::unix::net::UnixStream as StdUnixStream;

use std::path::Path;

#[cfg(feature = "sdk-async")]
use std::pin::Pin;

use std::sync::Arc;

#[cfg(feature = "sdk-async")]
use std::task::{Context, Poll};

#[cfg(feature = "sdk-async")]
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

#[cfg(feature = "sdk-async")]
use tokio::sync::mpsc;

#[cfg(feature = "sdk-async")]
use tokio::task::JoinHandle;

#[cfg(feature = "sdk-async")]
use tokio_rustls::TlsConnector;

#[cfg(feature = "sdk-async")]
use tokio_stream::wrappers::ReceiverStream;

#[cfg(feature = "sdk-async")]
use tokio_stream::Stream;

use zeroize::{Zeroize, Zeroizing};

#[cfg(feature = "sdk-async")]
const RPC_EVENT_STREAM_MAX_FRAME_BYTES: usize = 2 * 1024 * 1024;

const RPC_HTTP_HEADER_MAX_BYTES: usize = 64 * 1024;

const RPC_FRAME_PAYLOAD_MAX_BYTES: usize = 16 * 1024 * 1024;

const RPC_HTTP_RESPONSE_MAX_BYTES: usize =
    RPC_HTTP_HEADER_MAX_BYTES + 4 + RPC_FRAME_PAYLOAD_MAX_BYTES + 4;

#[derive(Clone, Copy)]
enum RpcEndpoint<'a> {
    Tcp(&'a str),
    Unix(&'a str),
}

impl RpcEndpoint<'_> {
    fn host_header(&self) -> &str {
        match self {
            Self::Tcp(authority) => authority,
            Self::Unix(_) => "localhost",
        }
    }
}
