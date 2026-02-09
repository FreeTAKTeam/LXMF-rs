use anyhow::{anyhow, Context, Result};
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use serde_json::Value;
use std::io::Read;
use std::sync::atomic::{AtomicU64, Ordering};

#[derive(Debug, Serialize, Deserialize)]
struct RpcRequest {
    id: u64,
    method: String,
    params: Option<Value>,
}

#[derive(Debug, Serialize, Deserialize)]
struct RpcResponse {
    id: u64,
    result: Option<Value>,
    error: Option<RpcError>,
}

#[derive(Debug, Serialize, Deserialize)]
struct RpcError {
    code: String,
    message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RpcEvent {
    pub event_type: String,
    pub payload: Value,
}

#[derive(Debug)]
pub struct RpcClient {
    base_url: String,
    agent: ureq::Agent,
    next_id: AtomicU64,
}

impl RpcClient {
    pub fn new(rpc_addr: &str) -> Self {
        Self::new_with_timeouts(
            rpc_addr,
            std::time::Duration::from_secs(3),
            std::time::Duration::from_secs(10),
            std::time::Duration::from_secs(10),
        )
    }

    pub fn new_with_timeouts(
        rpc_addr: &str,
        connect_timeout: std::time::Duration,
        read_timeout: std::time::Duration,
        write_timeout: std::time::Duration,
    ) -> Self {
        let base_url = if rpc_addr.starts_with("http://") || rpc_addr.starts_with("https://") {
            rpc_addr.trim_end_matches('/').to_string()
        } else {
            format!("http://{}", rpc_addr)
        };

        Self {
            base_url,
            agent: ureq::AgentBuilder::new()
                .timeout_connect(connect_timeout)
                .timeout_read(read_timeout)
                .timeout_write(write_timeout)
                .build(),
            next_id: AtomicU64::new(1),
        }
    }

    pub fn call(&self, method: &str, params: Option<Value>) -> Result<Value> {
        let request = RpcRequest {
            id: self.next_id.fetch_add(1, Ordering::Relaxed),
            method: method.to_string(),
            params,
        };
        let body = encode_frame(&request)?;
        let url = format!("{}/rpc", self.base_url);

        let response = self
            .agent
            .post(&url)
            .set("Content-Type", "application/msgpack")
            .send_bytes(&body)
            .map_err(|err| anyhow!("rpc request failed: {err}"))?;

        let bytes = read_response_body(response).context("failed to read rpc response")?;

        let decoded: RpcResponse = decode_frame(&bytes)?;
        if let Some(err) = decoded.error {
            return Err(anyhow!(
                "rpc {} failed [{}]: {}",
                method,
                err.code,
                err.message
            ));
        }

        Ok(decoded.result.unwrap_or(Value::Null))
    }

    pub fn call_typed<T: DeserializeOwned>(
        &self,
        method: &str,
        params: Option<Value>,
    ) -> Result<T> {
        let value = self.call(method, params)?;
        serde_json::from_value(value)
            .with_context(|| format!("failed to decode rpc response for method {method}"))
    }

    pub fn poll_event(&self) -> Result<Option<RpcEvent>> {
        let url = format!("{}/events", self.base_url);
        let response = match self.agent.get(&url).call() {
            Ok(resp) => resp,
            Err(ureq::Error::Status(204, _)) => return Ok(None),
            Err(err) => return Err(anyhow!("event poll failed: {err}")),
        };

        if response.status() == 204 {
            return Ok(None);
        }

        let bytes = read_response_body(response).context("failed to read event response")?;
        let event = decode_frame(&bytes)?;
        Ok(Some(event))
    }
}

fn encode_frame<T: Serialize>(value: &T) -> Result<Vec<u8>> {
    let payload = rmp_serde::to_vec(value).context("failed to msgpack encode payload")?;
    let len = u32::try_from(payload.len()).context("frame too large")?;
    let mut framed = Vec::with_capacity(4 + payload.len());
    framed.extend_from_slice(&len.to_be_bytes());
    framed.extend_from_slice(&payload);
    Ok(framed)
}

fn decode_frame<T: DeserializeOwned>(bytes: &[u8]) -> Result<T> {
    if bytes.len() < 4 {
        return Err(anyhow!("frame missing header"));
    }
    let mut len_buf = [0u8; 4];
    len_buf.copy_from_slice(&bytes[..4]);
    let payload_len = u32::from_be_bytes(len_buf) as usize;
    if bytes.len() < 4 + payload_len {
        return Err(anyhow!("incomplete frame payload"));
    }
    let payload = &bytes[4..4 + payload_len];
    rmp_serde::from_slice(payload).context("failed to decode framed msgpack")
}

fn read_response_body(response: ureq::Response) -> Result<Vec<u8>> {
    let content_length = response
        .header("Content-Length")
        .and_then(|value| value.parse::<usize>().ok());
    let mut reader = response.into_reader();

    if let Some(length) = content_length {
        let mut bytes = vec![0u8; length];
        if length > 0 {
            reader
                .read_exact(&mut bytes)
                .context("failed to read content-length bytes")?;
        }
        return Ok(bytes);
    }

    let mut bytes = Vec::new();
    reader
        .read_to_end(&mut bytes)
        .context("failed to read response body")?;
    Ok(bytes)
}
