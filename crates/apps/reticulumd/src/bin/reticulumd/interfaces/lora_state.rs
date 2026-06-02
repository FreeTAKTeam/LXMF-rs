use serde::{Deserialize, Serialize};
use std::fs;
use std::io::Write;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

pub(crate) const LORA_STATE_VERSION: u8 = 1;
const CLOCK_UNCERTAINTY_THRESHOLD_MS: u64 = 5 * 60 * 1000;
const STALE_STATE_THRESHOLD_MS: u64 = 30 * 24 * 60 * 60 * 1000;
const MAX_DUTY_CYCLE_DEBT_MS: u64 = 24 * 60 * 60 * 1000;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct LoraState {
    pub(crate) version: u8,
    pub(crate) duty_cycle_debt_ms: u64,
    pub(crate) last_updated_unix_ms: u64,
    #[serde(default)]
    pub(crate) debt_elapsed_ms: u64,
    #[serde(default)]
    pub(crate) uncertain: bool,
    #[serde(default)]
    pub(crate) uncertainty_reason: Option<String>,
}

pub(crate) fn ensure_state_file(path: &str) -> Result<LoraState, String> {
    let state_path = Path::new(path);
    if let Some(parent) = state_path.parent() {
        fs::create_dir_all(parent).map_err(|err| {
            format!("failed to create lora state dir {}: {err}", parent.display())
        })?;
    }

    if state_path.exists() {
        let bytes = fs::read(state_path)
            .map_err(|err| format!("failed to read lora state {}: {err}", state_path.display()))?;
        let mut state: LoraState = serde_json::from_slice(&bytes).map_err(|err| {
            format!(
                "invalid lora state {}: {err}; fail-closed until operator resets the state file",
                state_path.display()
            )
        })?;

        if state.version != LORA_STATE_VERSION {
            return Err(format!(
                "invalid lora state {}: unsupported version {}; fail-closed until operator resets the state file",
                state_path.display(),
                state.version
            ));
        }

        if state.uncertain {
            return Err(format!(
                "lora state {} is marked uncertain (reason: {}); fail-closed until operator resets the state file",
                state_path.display(),
                state.uncertainty_reason.as_deref().unwrap_or("unknown")
            ));
        }

        let now_ms = now_unix_ms();
        if state.last_updated_unix_ms == 0 {
            return mark_uncertain_and_fail(
                state_path,
                state,
                "last_updated_unix_ms is zero (invalid persisted state timestamp)",
            );
        }

        if now_ms.saturating_add(CLOCK_UNCERTAINTY_THRESHOLD_MS) < state.last_updated_unix_ms {
            let last_updated_unix_ms = state.last_updated_unix_ms;
            return mark_uncertain_and_fail(
                state_path,
                state,
                format!(
                    "clock rollback detected: now_ms={} last_updated_unix_ms={}",
                    now_ms, last_updated_unix_ms
                ),
            );
        }

        let elapsed_ms = now_ms.saturating_sub(state.last_updated_unix_ms);
        if elapsed_ms > STALE_STATE_THRESHOLD_MS {
            return mark_uncertain_and_fail(
                state_path,
                state,
                format!(
                    "state timestamp too old: elapsed_ms={} exceeds stale threshold_ms={}",
                    elapsed_ms, STALE_STATE_THRESHOLD_MS
                ),
            );
        }

        if state.duty_cycle_debt_ms > MAX_DUTY_CYCLE_DEBT_MS {
            let duty_cycle_debt_ms = state.duty_cycle_debt_ms;
            return mark_uncertain_and_fail(
                state_path,
                state,
                format!(
                    "duty_cycle_debt_ms={} exceeds max_debt_ms={}",
                    duty_cycle_debt_ms, MAX_DUTY_CYCLE_DEBT_MS
                ),
            );
        }

        state.debt_elapsed_ms = elapsed_ms;
        state.duty_cycle_debt_ms = state.duty_cycle_debt_ms.saturating_sub(elapsed_ms);
        state.last_updated_unix_ms = now_ms;
        persist_state_atomically(state_path, &state)?;
        return Ok(state);
    }

    let state = LoraState {
        version: LORA_STATE_VERSION,
        duty_cycle_debt_ms: 0,
        last_updated_unix_ms: now_unix_ms(),
        debt_elapsed_ms: 0,
        uncertain: false,
        uncertainty_reason: None,
    };
    persist_state_atomically(state_path, &state)?;
    Ok(state)
}

fn mark_uncertain_and_fail(
    state_path: &Path,
    mut state: LoraState,
    reason: impl Into<String>,
) -> Result<LoraState, String> {
    let reason = reason.into();
    state.uncertain = true;
    state.uncertainty_reason = Some(reason.clone());
    state.debt_elapsed_ms = 0;

    if let Err(err) = persist_state_atomically(state_path, &state) {
        return Err(format!(
            "lora state {} marked uncertain but persistence failed: {}; reason={}; fail-closed until operator resets the state file",
            state_path.display(),
            err,
            reason
        ));
    }

    Err(format!(
        "lora state {} marked uncertain (reason: {}); fail-closed until operator resets the state file",
        state_path.display(),
        reason
    ))
}

fn persist_state_atomically(path: &Path, state: &LoraState) -> Result<(), String> {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    let parent = if parent.as_os_str().is_empty() { Path::new(".") } else { parent };

    let tmp_path = path.with_extension("tmp");
    let payload = serde_json::to_vec_pretty(state)
        .map_err(|err| format!("failed to serialize lora state {}: {err}", path.display()))?;

    let mut file = fs::File::create(&tmp_path)
        .map_err(|err| format!("failed to create lora tmp state {}: {err}", tmp_path.display()))?;
    file.write_all(&payload)
        .map_err(|err| format!("failed to write lora tmp state {}: {err}", tmp_path.display()))?;
    file.sync_all()
        .map_err(|err| format!("failed to fsync lora tmp state {}: {err}", tmp_path.display()))?;
    drop(file);

    fs::rename(&tmp_path, path).map_err(|err| {
        format!(
            "failed to replace lora state {} from {}: {err}",
            path.display(),
            tmp_path.display()
        )
    })?;

    match fs::File::open(parent) {
        Ok(dir) => {
            if let Err(err) = dir.sync_all() {
                if should_skip_dir_sync(&err) {
                    log::warn!(
                        "[daemon] lora state dir fsync skipped path={} err={}",
                        parent.display(),
                        err
                    );
                    return Ok(());
                }
                return Err(format!("failed to fsync lora state dir {}: {err}", parent.display()));
            }
            Ok(())
        }
        Err(err) => {
            if should_skip_dir_sync(&err) {
                log::warn!(
                    "[daemon] lora state dir sync open skipped path={} err={}",
                    parent.display(),
                    err
                );
                return Ok(());
            }
            Err(format!("failed to open lora state dir {}: {err}", parent.display()))
        }
    }
}

fn now_unix_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis() as u64)
        .unwrap_or(0)
}

fn should_skip_dir_sync(err: &std::io::Error) -> bool {
    matches!(err.kind(), std::io::ErrorKind::Unsupported | std::io::ErrorKind::PermissionDenied)
}

#[cfg(test)]
fn write_state_for_test(path: &Path, state: &LoraState) -> Result<(), String> {
    persist_state_atomically(path, state)
}

#[cfg(test)]
fn read_state_for_test(path: &Path) -> Result<LoraState, String> {
    let bytes = fs::read(path)
        .map_err(|err| format!("failed to read lora state {}: {err}", path.display()))?;
    serde_json::from_slice(&bytes)
        .map_err(|err| format!("failed to parse lora state {}: {err}", path.display()))
}

#[cfg(test)]
fn uncertainty_threshold_ms_for_test() -> u64 {
    CLOCK_UNCERTAINTY_THRESHOLD_MS
}

#[cfg(test)]
fn stale_threshold_ms_for_test() -> u64 {
    STALE_STATE_THRESHOLD_MS
}

#[cfg(test)]
fn max_debt_ms_for_test() -> u64 {
    MAX_DUTY_CYCLE_DEBT_MS
}

#[cfg(test)]
fn now_unix_ms_for_test() -> u64 {
    now_unix_ms()
}

#[cfg(test)]
mod tests {
    use super::{
        ensure_state_file, max_debt_ms_for_test, now_unix_ms_for_test, read_state_for_test,
        stale_threshold_ms_for_test, uncertainty_threshold_ms_for_test, write_state_for_test,
        LoraState, LORA_STATE_VERSION,
    };
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn ensure_state_file_initializes_default_state() {
        let temp = TempDir::new().expect("temp dir");
        let state_path = temp.path().join("lora-state.json");
        let state = ensure_state_file(state_path.to_str().expect("utf8 path"))
            .expect("ensure state should initialize");
        assert_eq!(state.version, LORA_STATE_VERSION);
        assert_eq!(state.duty_cycle_debt_ms, 0);
        assert_eq!(state.debt_elapsed_ms, 0);
        assert!(!state.uncertain);
    }

    #[test]
    fn ensure_state_file_supports_parentless_relative_path() {
        let file_name =
            format!("lora-state-{}-{}.json", std::process::id(), now_unix_ms_for_test());
        let _ = fs::remove_file(&file_name);

        let state = ensure_state_file(&file_name).expect("state should initialize");
        assert_eq!(state.version, LORA_STATE_VERSION);

        fs::remove_file(&file_name).expect("cleanup parentless state file");
    }

    #[test]
    fn ensure_state_file_rejects_corrupt_payload() {
        let temp = TempDir::new().expect("temp dir");
        let state_path = temp.path().join("lora-state.json");
        fs::write(&state_path, b"{not-json").expect("write corrupt state");

        let err =
            ensure_state_file(state_path.to_str().expect("utf8 path")).expect_err("should fail");
        assert!(err.contains("fail-closed"), "unexpected error: {err}");
    }

    #[test]
    fn ensure_state_file_marks_uncertain_on_clock_rollback() {
        let temp = TempDir::new().expect("temp dir");
        let state_path = temp.path().join("lora-state.json");
        let future_state = LoraState {
            version: LORA_STATE_VERSION,
            duty_cycle_debt_ms: 100,
            last_updated_unix_ms: now_unix_ms_for_test()
                .saturating_add(uncertainty_threshold_ms_for_test())
                .saturating_add(60_000),
            debt_elapsed_ms: 0,
            uncertain: false,
            uncertainty_reason: None,
        };
        write_state_for_test(&state_path, &future_state).expect("write state");

        let err =
            ensure_state_file(state_path.to_str().expect("utf8 path")).expect_err("must fail");
        assert!(err.contains("clock rollback"), "expected rollback fail-closed error, got: {err}");

        let persisted = read_state_for_test(&state_path).expect("read persisted state");
        assert!(persisted.uncertain, "state must remain marked uncertain");
    }

    #[test]
    fn ensure_state_file_marks_uncertain_on_stale_state() {
        let temp = TempDir::new().expect("temp dir");
        let state_path = temp.path().join("lora-state.json");
        let stale_state = LoraState {
            version: LORA_STATE_VERSION,
            duty_cycle_debt_ms: 100,
            last_updated_unix_ms: now_unix_ms_for_test()
                .saturating_sub(stale_threshold_ms_for_test())
                .saturating_sub(1),
            debt_elapsed_ms: 0,
            uncertain: false,
            uncertainty_reason: None,
        };
        write_state_for_test(&state_path, &stale_state).expect("write state");

        let err =
            ensure_state_file(state_path.to_str().expect("utf8 path")).expect_err("must fail");
        assert!(err.contains("timestamp too old"), "expected stale fail-closed error, got: {err}");

        let persisted = read_state_for_test(&state_path).expect("read persisted state");
        assert!(persisted.uncertain, "state must remain marked uncertain");
    }

    #[test]
    fn ensure_state_file_marks_uncertain_on_zero_timestamp() {
        let temp = TempDir::new().expect("temp dir");
        let state_path = temp.path().join("lora-state.json");
        let invalid_state = LoraState {
            version: LORA_STATE_VERSION,
            duty_cycle_debt_ms: 100,
            last_updated_unix_ms: 0,
            debt_elapsed_ms: 0,
            uncertain: false,
            uncertainty_reason: None,
        };
        write_state_for_test(&state_path, &invalid_state).expect("write state");

        let err =
            ensure_state_file(state_path.to_str().expect("utf8 path")).expect_err("must fail");
        assert!(
            err.contains("last_updated_unix_ms is zero"),
            "expected invalid timestamp fail-closed error, got: {err}"
        );

        let persisted = read_state_for_test(&state_path).expect("read persisted state");
        assert!(persisted.uncertain, "state must remain marked uncertain");
    }

    #[test]
    fn ensure_state_file_marks_uncertain_when_debt_exceeds_max() {
        let temp = TempDir::new().expect("temp dir");
        let state_path = temp.path().join("lora-state.json");
        let overflow_state = LoraState {
            version: LORA_STATE_VERSION,
            duty_cycle_debt_ms: max_debt_ms_for_test().saturating_add(1),
            last_updated_unix_ms: now_unix_ms_for_test(),
            debt_elapsed_ms: 0,
            uncertain: false,
            uncertainty_reason: None,
        };
        write_state_for_test(&state_path, &overflow_state).expect("write state");

        let err =
            ensure_state_file(state_path.to_str().expect("utf8 path")).expect_err("must fail");
        assert!(
            err.contains("exceeds max_debt"),
            "expected debt overflow fail-closed error, got: {err}"
        );

        let persisted = read_state_for_test(&state_path).expect("read persisted state");
        assert!(persisted.uncertain, "state must remain marked uncertain");
    }

    #[test]
    fn ensure_state_file_reduces_duty_cycle_debt_by_elapsed_time() {
        let temp = TempDir::new().expect("temp dir");
        let state_path = temp.path().join("lora-state.json");
        let initial_debt_ms = 8_000;
        let elapsed_target_ms = 3_000;
        let persisted = LoraState {
            version: LORA_STATE_VERSION,
            duty_cycle_debt_ms: initial_debt_ms,
            last_updated_unix_ms: now_unix_ms_for_test().saturating_sub(elapsed_target_ms),
            debt_elapsed_ms: 0,
            uncertain: false,
            uncertainty_reason: None,
        };
        write_state_for_test(&state_path, &persisted).expect("write state");

        let state = ensure_state_file(state_path.to_str().expect("utf8 path"))
            .expect("state should normalize debt");
        assert!(state.duty_cycle_debt_ms < initial_debt_ms);
        assert!(state.debt_elapsed_ms > 0);
        assert!(!state.uncertain);

        let persisted = read_state_for_test(&state_path).expect("read persisted state");
        assert_eq!(persisted.duty_cycle_debt_ms, state.duty_cycle_debt_ms);
        assert_eq!(persisted.debt_elapsed_ms, state.debt_elapsed_ms);
    }
}
