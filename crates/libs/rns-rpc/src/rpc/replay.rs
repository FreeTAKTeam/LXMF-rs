use super::{RpcDaemon, RpcRequest, RpcResponse};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value as JsonValue};
use sha2::{Digest, Sha256};
use std::fmt;
use std::fs;
use std::path::Path;
use std::time::Instant;

const TRACE_SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RpcReplayTrace {
    pub version: u32,
    pub name: String,
    #[serde(default)]
    pub seed: Option<u64>,
    pub steps: Vec<RpcReplayStep>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RpcReplayStep {
    #[serde(default)]
    pub label: Option<String>,
    pub request: RpcRequest,
    #[serde(default)]
    pub expect: RpcReplayExpectation,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct RpcReplayExpectation {
    #[serde(default)]
    pub ok: Option<bool>,
    #[serde(default)]
    pub error_code: Option<String>,
    #[serde(default)]
    pub result_subset: Option<JsonValue>,
    #[serde(default)]
    pub response_subset: Option<JsonValue>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RpcReplayCapture {
    pub trace_name: String,
    pub trace_version: u32,
    pub steps_executed: usize,
    pub total_duration_ms: u128,
    pub response_digest_sha256: String,
    pub steps: Vec<RpcReplayCaptureStep>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RpcReplayCaptureStep {
    pub index: usize,
    #[serde(default)]
    pub label: Option<String>,
    pub request: RpcRequest,
    pub response: RpcResponse,
    pub duration_ms: u128,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RpcReplayFailure {
    pub step_index: usize,
    #[serde(default)]
    pub label: Option<String>,
    pub reason: String,
}

impl fmt::Display for RpcReplayFailure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if let Some(label) = self.label.as_deref() {
            write!(f, "replay step {} ({label}) failed: {}", self.step_index, self.reason)
        } else {
            write!(f, "replay step {} failed: {}", self.step_index, self.reason)
        }
    }
}

impl std::error::Error for RpcReplayFailure {}

pub fn load_trace_file(path: impl AsRef<Path>) -> Result<RpcReplayTrace, std::io::Error> {
    let path = path.as_ref();
    let bytes = fs::read(path)?;
    serde_json::from_slice::<RpcReplayTrace>(&bytes)
        .map_err(|error| std::io::Error::new(std::io::ErrorKind::InvalidData, error))
}

pub fn save_capture_file(
    path: impl AsRef<Path>,
    capture: &RpcReplayCapture,
) -> Result<(), std::io::Error> {
    let bytes = serde_json::to_vec_pretty(capture)
        .map_err(|error| std::io::Error::new(std::io::ErrorKind::InvalidData, error))?;
    fs::write(path, bytes)
}

pub fn execute_trace(
    daemon: &RpcDaemon,
    trace: &RpcReplayTrace,
) -> Result<RpcReplayCapture, RpcReplayFailure> {
    if trace.version != TRACE_SCHEMA_VERSION {
        return Err(RpcReplayFailure {
            step_index: 0,
            label: None,
            reason: format!(
                "unsupported replay version {}; expected {}",
                trace.version, TRACE_SCHEMA_VERSION
            ),
        });
    }
    if trace.steps.is_empty() {
        return Err(RpcReplayFailure {
            step_index: 0,
            label: None,
            reason: "trace has no steps".to_string(),
        });
    }

    let started = Instant::now();
    let mut captures = Vec::with_capacity(trace.steps.len());
    for (index, step) in trace.steps.iter().enumerate() {
        let step_started = Instant::now();
        let response =
            daemon.handle_rpc(step.request.clone()).map_err(|error| RpcReplayFailure {
                step_index: index,
                label: step.label.clone(),
                reason: format!("rpc dispatch error: {error}"),
            })?;
        let step_capture = RpcReplayCaptureStep {
            index,
            label: step.label.clone(),
            request: step.request.clone(),
            response: response.clone(),
            duration_ms: step_started.elapsed().as_millis(),
        };
        validate_step_expectation(index, step, &response)?;
        captures.push(step_capture);
    }

    let digest = digest_replay_steps(&captures).map_err(|error| RpcReplayFailure {
        step_index: trace.steps.len().saturating_sub(1),
        label: trace.steps.last().and_then(|step| step.label.clone()),
        reason: format!("failed to hash replay steps: {error}"),
    })?;

    Ok(RpcReplayCapture {
        trace_name: trace.name.clone(),
        trace_version: trace.version,
        steps_executed: captures.len(),
        total_duration_ms: started.elapsed().as_millis(),
        response_digest_sha256: digest,
        steps: captures,
    })
}

fn validate_step_expectation(
    index: usize,
    step: &RpcReplayStep,
    response: &RpcResponse,
) -> Result<(), RpcReplayFailure> {
    let expected_ok = step.expect.ok.unwrap_or_else(|| step.expect.error_code.is_none());

    if expected_ok {
        if let Some(error) = response.error.as_ref() {
            return Err(RpcReplayFailure {
                step_index: index,
                label: step.label.clone(),
                reason: format!("unexpected error {} ({})", error.message, error.code),
            });
        }
    } else if response.error.is_none() {
        return Err(RpcReplayFailure {
            step_index: index,
            label: step.label.clone(),
            reason: "expected an error response but received success".to_string(),
        });
    }

    if let Some(expected_code) = step.expect.error_code.as_deref() {
        let actual_code = response.error.as_ref().map(|value| value.code.as_str());
        if actual_code != Some(expected_code) {
            return Err(RpcReplayFailure {
                step_index: index,
                label: step.label.clone(),
                reason: format!(
                    "expected error code '{expected_code}', received {:?}",
                    actual_code
                ),
            });
        }
    }

    if let Some(expected_subset) = step.expect.result_subset.as_ref() {
        let actual = response.result.as_ref().ok_or_else(|| RpcReplayFailure {
            step_index: index,
            label: step.label.clone(),
            reason: "expected result subset but response result was null".to_string(),
        })?;
        if !json_contains(actual, expected_subset) {
            return Err(RpcReplayFailure {
                step_index: index,
                label: step.label.clone(),
                reason: format!(
                    "result subset mismatch; expected subset={}, actual={}",
                    expected_subset, actual
                ),
            });
        }
    }

    if let Some(expected_subset) = step.expect.response_subset.as_ref() {
        let actual = serde_json::to_value(response).map_err(|error| RpcReplayFailure {
            step_index: index,
            label: step.label.clone(),
            reason: format!("failed to convert response to json: {error}"),
        })?;
        if !json_contains(&actual, expected_subset) {
            return Err(RpcReplayFailure {
                step_index: index,
                label: step.label.clone(),
                reason: format!(
                    "response subset mismatch; expected subset={}, actual={}",
                    expected_subset, actual
                ),
            });
        }
    }

    Ok(())
}

fn digest_replay_steps(steps: &[RpcReplayCaptureStep]) -> Result<String, serde_json::Error> {
    let snapshot = steps
        .iter()
        .map(|step| {
            let request = serde_json::to_value(&step.request)?;
            let response = serde_json::to_value(&step.response)?;
            Ok::<_, serde_json::Error>(json!({
                "request": normalize_digest_value(request),
                "response": normalize_digest_value(response),
            }))
        })
        .collect::<Result<Vec<_>, _>>()?;
    let payload = serde_json::to_vec(&snapshot)?;
    let mut hasher = Sha256::new();
    hasher.update(payload);
    Ok(hex::encode(hasher.finalize()))
}

fn normalize_digest_value(value: JsonValue) -> JsonValue {
    match value {
        JsonValue::Object(map) => {
            let mut normalized = serde_json::Map::new();
            for (key, inner) in map {
                if is_dynamic_digest_key(key.as_str()) {
                    continue;
                }
                normalized.insert(key, normalize_digest_value(inner));
            }
            JsonValue::Object(normalized)
        }
        JsonValue::Array(values) => {
            JsonValue::Array(values.into_iter().map(normalize_digest_value).collect())
        }
        other => other,
    }
}

fn is_dynamic_digest_key(key: &str) -> bool {
    matches!(
        key,
        "timestamp" | "created_ts_ms" | "updated_ts_ms" | "first_seen" | "last_seen" | "expires_at"
    )
}

fn json_contains(actual: &JsonValue, expected: &JsonValue) -> bool {
    match (actual, expected) {
        (JsonValue::Object(actual_map), JsonValue::Object(expected_map)) => {
            expected_map.iter().all(|(key, expected_value)| {
                actual_map
                    .get(key)
                    .is_some_and(|actual_value| json_contains(actual_value, expected_value))
            })
        }
        (JsonValue::Array(actual_items), JsonValue::Array(expected_items)) => {
            expected_items.iter().all(|expected_item| {
                actual_items.iter().any(|actual_item| json_contains(actual_item, expected_item))
            })
        }
        _ => actual == expected,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::path::{Path, PathBuf};

    fn fixture_path() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../..")
            .join("docs/fixtures/sdk-v2/rpc/replay_known_send_cancel.v1.json")
    }

    #[test]
    fn replay_fixture_trace_executes_successfully() {
        let trace = load_trace_file(fixture_path()).expect("fixture trace should load");
        let daemon = RpcDaemon::test_instance();
        let capture = execute_trace(&daemon, &trace).expect("fixture replay should succeed");
        assert_eq!(capture.trace_name, "known-send-cancel-trace");
        assert_eq!(capture.steps_executed, 6);
        assert!(!capture.response_digest_sha256.is_empty());
    }

    #[test]
    fn replay_detects_mismatched_result_subset() {
        let mut trace = load_trace_file(fixture_path()).expect("fixture trace should load");
        trace.steps[1].expect.result_subset = Some(json!({ "message_id": "different-id" }));
        let daemon = RpcDaemon::test_instance();
        let failure = execute_trace(&daemon, &trace).expect_err("mismatch should fail");
        assert_eq!(failure.step_index, 1);
        assert!(failure.reason.contains("result subset mismatch"));
    }

    #[test]
    fn json_contains_matches_subset_arrays() {
        let actual = json!({
            "transitions": [
                {"status": "queued", "timestamp": 1},
                {"status": "sending", "timestamp": 2},
                {"status": "sent: direct", "timestamp": 3}
            ]
        });
        let expected = json!({
            "transitions": [
                {"status": "queued"},
                {"status": "sent: direct"}
            ]
        });
        assert!(json_contains(&actual, &expected));
    }
}
