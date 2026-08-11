use super::model::{address_hex, ControlRequest, SharedState};
use super::node::handle_request;
use rns_transport::destination::SingleInputDestination;
use rns_transport::transport::Transport;
use serde_json::{json, Value};
use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{watch, Mutex};

pub async fn serve(
    bind: &str,
    transport: Arc<Transport>,
    destination: Arc<Mutex<SingleInputDestination>>,
    state: SharedState,
) -> Result<(), String> {
    let listener = TcpListener::bind(bind)
        .await
        .map_err(|error| format!("bind control listener {bind}: {error}"))?;
    let (shutdown_tx, mut shutdown_rx) = watch::channel(false);
    println!(
        "{}",
        json!({
            "ready": true,
            "control": listener.local_addr().map_err(|error| error.to_string())?.to_string(),
            "destination_hash": address_hex(&state.destination_hash),
            "identity_hash": address_hex(&state.identity_hash),
        })
    );

    loop {
        tokio::select! {
            changed = shutdown_rx.changed() => {
                if changed.is_err() || *shutdown_rx.borrow() {
                    break;
                }
            }
            accepted = listener.accept() => {
                let (stream, _) = accepted.map_err(|error| format!("accept control connection: {error}"))?;
                let transport = transport.clone();
                let destination = destination.clone();
                let state = state.clone();
                let shutdown_tx = shutdown_tx.clone();
                tokio::spawn(async move {
                    if let Err(error) = serve_connection(
                        stream,
                        transport,
                        destination,
                        state,
                        shutdown_tx,
                    ).await {
                        log::warn!("independent interop control request failed: {error}");
                    }
                });
            }
        }
    }
    Ok(())
}

async fn serve_connection(
    stream: TcpStream,
    transport: Arc<Transport>,
    destination: Arc<Mutex<SingleInputDestination>>,
    state: SharedState,
    shutdown_tx: watch::Sender<bool>,
) -> Result<(), String> {
    let (reader, mut writer) = stream.into_split();
    let mut reader = BufReader::new(reader);
    let mut line = String::new();
    reader.read_line(&mut line).await.map_err(|error| format!("read control request: {error}"))?;
    let request: ControlRequest = serde_json::from_str(line.trim())
        .map_err(|error| format!("decode control request: {error}"))?;
    let response = match handle_request(&transport, &destination, &state, &request).await {
        Ok(result) => json!({"ok": true, "result": result}),
        Err(error) => json!({"ok": false, "error": error}),
    };
    let mut encoded = serde_json::to_vec(&response).map_err(|error| error.to_string())?;
    encoded.push(b'\n');
    writer.write_all(&encoded).await.map_err(|error| format!("write control response: {error}"))?;
    if request.method == "shutdown" && response.get("ok") == Some(&Value::Bool(true)) {
        shutdown_tx.send(true).map_err(|_| {
            "signal independent interop probe shutdown: receiver closed".to_string()
        })?;
    }
    Ok(())
}
