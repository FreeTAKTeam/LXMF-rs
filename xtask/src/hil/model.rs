use clap::ValueEnum;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fmt;
use std::path::PathBuf;

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize, ValueEnum)]
#[serde(rename_all = "lowercase")]
pub enum ExecutionLevel {
    Pr,
    Nightly,
    Release,
}

impl fmt::Display for ExecutionLevel {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Pr => "pr",
            Self::Nightly => "nightly",
            Self::Release => "release",
        })
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ResultClass {
    Pass,
    FailProtocol,
    FailAssertion,
    FailDevice,
    FailLab,
    Blocked,
}

impl ResultClass {
    pub fn is_pass(self) -> bool {
        self == Self::Pass
    }
}

impl fmt::Display for ResultClass {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Pass => "PASS",
            Self::FailProtocol => "FAIL_PROTOCOL",
            Self::FailAssertion => "FAIL_ASSERTION",
            Self::FailDevice => "FAIL_DEVICE",
            Self::FailLab => "FAIL_LAB",
            Self::Blocked => "BLOCKED",
        })
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Profile {
    pub id: String,
    pub label: String,
    pub adapter: String,
    pub host: String,
    #[serde(default)]
    pub physical: bool,
    #[serde(default)]
    pub suites: Vec<String>,
    #[serde(default)]
    pub capabilities: Vec<String>,
    pub identity_env: Option<String>,
    pub endpoint_env: Option<String>,
    pub firmware_env: Option<String>,
    pub firmware_hash_env: Option<String>,
    pub power_hub_env: Option<String>,
    pub power_port_env: Option<String>,
    pub reset_command_env: Option<String>,
    pub executor_env: Option<String>,
    #[serde(default)]
    pub rf_required: bool,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct CommandSpec {
    pub program: String,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub env: BTreeMap<String, String>,
    pub cwd: Option<PathBuf>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct CaseDefinition {
    pub id: String,
    pub suite: String,
    pub description: String,
    #[serde(default)]
    pub levels: Vec<ExecutionLevel>,
    #[serde(default)]
    pub profiles: Vec<String>,
    #[serde(default)]
    pub requires_env: Vec<String>,
    pub timeout_secs: Option<u64>,
    pub failure_class: Option<ResultClass>,
    pub command: Option<CommandSpec>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct LabConfig {
    pub version: u32,
    pub rack_id: String,
    #[serde(default = "default_lock_env")]
    pub lock_env: String,
    #[serde(default = "default_lock_ttl_secs")]
    pub lock_ttl_secs: u64,
    #[serde(default = "default_reset_timeout_secs")]
    pub reset_timeout_secs: u64,
    #[serde(default = "default_rf_confirmation_env")]
    pub rf_confirmation_env: String,
    #[serde(default)]
    pub profiles: Vec<Profile>,
}

fn default_lock_env() -> String {
    "HIL_LOCK_PATH".to_string()
}

fn default_rf_confirmation_env() -> String {
    "HIL_RF_ENCLOSURE_CONFIRMED".to_string()
}

fn default_lock_ttl_secs() -> u64 {
    14_400
}

fn default_reset_timeout_secs() -> u64 {
    120
}

#[derive(Clone, Debug, Serialize)]
pub struct CaseResult {
    pub case_id: String,
    pub profile_id: String,
    pub result: ResultClass,
    pub reason: String,
    pub duration_ms: u128,
    pub attempts: u8,
    pub seed: u64,
    pub command: Option<String>,
    pub artifacts: Vec<String>,
}

#[derive(Clone, Debug, Serialize)]
pub struct RunReport {
    pub schema: &'static str,
    pub run_id: String,
    pub commit_sha: String,
    pub rack_id: String,
    pub level: ExecutionLevel,
    pub started_at_unix_secs: u64,
    pub duration_ms: u128,
    pub result: ResultClass,
    pub environment: BTreeMap<String, String>,
    pub cases: Vec<CaseResult>,
}

impl RunReport {
    pub fn new(
        run_id: String,
        commit_sha: String,
        rack_id: String,
        level: ExecutionLevel,
        started_at_unix_secs: u64,
    ) -> Self {
        Self {
            schema: "lxmf-rs-hil-result-v1",
            run_id,
            commit_sha,
            rack_id,
            level,
            started_at_unix_secs,
            duration_ms: 0,
            result: ResultClass::Pass,
            environment: BTreeMap::new(),
            cases: Vec::new(),
        }
    }

    pub fn finalize(&mut self, duration_ms: u128) {
        self.duration_ms = duration_ms;
        self.result = if self.cases.iter().all(|case| case.result.is_pass()) {
            ResultClass::Pass
        } else if self.cases.iter().any(|case| case.result == ResultClass::FailProtocol) {
            ResultClass::FailProtocol
        } else if self.cases.iter().any(|case| case.result == ResultClass::FailAssertion) {
            ResultClass::FailAssertion
        } else if self.cases.iter().any(|case| case.result == ResultClass::FailDevice) {
            ResultClass::FailDevice
        } else if self.cases.iter().any(|case| case.result == ResultClass::FailLab) {
            ResultClass::FailLab
        } else {
            ResultClass::Blocked
        };
    }
}

#[cfg(test)]
mod tests {
    use super::{ExecutionLevel, ResultClass, RunReport};

    #[test]
    fn result_class_precedence_preserves_protocol_failures() {
        let mut report = RunReport::new(
            "run".to_string(),
            "sha".to_string(),
            "rack".to_string(),
            ExecutionLevel::Pr,
            1,
        );
        report.cases.push(super::CaseResult {
            case_id: "blocked".to_string(),
            profile_id: "device".to_string(),
            result: ResultClass::Blocked,
            reason: "missing device".to_string(),
            duration_ms: 0,
            attempts: 1,
            seed: 1,
            command: None,
            artifacts: Vec::new(),
        });
        report.cases.push(super::CaseResult {
            case_id: "protocol".to_string(),
            profile_id: "virtual".to_string(),
            result: ResultClass::FailProtocol,
            reason: "bad frame".to_string(),
            duration_ms: 0,
            attempts: 1,
            seed: 1,
            command: None,
            artifacts: Vec::new(),
        });
        report.finalize(1);
        assert_eq!(report.result, ResultClass::FailProtocol);
    }
}
