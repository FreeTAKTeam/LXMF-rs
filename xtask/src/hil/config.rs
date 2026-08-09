use super::model::{CaseDefinition, ExecutionLevel, LabConfig, Profile};
use anyhow::{bail, Context, Result};
use std::collections::BTreeSet;
use std::ffi::OsStr;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Clone, Debug)]
pub struct HilConfig {
    pub root: PathBuf,
    pub lab: LabConfig,
    pub cases: Vec<CaseDefinition>,
}

impl HilConfig {
    pub fn load(root: &Path, lab_path: Option<&Path>) -> Result<Self> {
        let root = root
            .canonicalize()
            .with_context(|| format!("canonicalize repository root {}", root.display()))?;
        let path = lab_path.map(PathBuf::from).unwrap_or_else(|| root.join("tests/hil/lab.toml"));
        let lab_text = fs::read_to_string(&path)
            .with_context(|| format!("read HIL lab config {}", path.display()))?;
        let lab: LabConfig = toml::from_str(&lab_text)
            .with_context(|| format!("parse HIL lab config {}", path.display()))?;
        let cases = load_cases(&root.join("tests/hil/cases"))?;
        validate(&lab, &cases)?;
        Ok(Self { root, lab, cases })
    }

    pub fn profile(&self, id: &str) -> Option<&Profile> {
        self.lab.profiles.iter().find(|profile| {
            profile.id == id || profile.id.replace('_', "-") == id.replace('_', "-")
        })
    }

    pub fn profiles_for(
        &self,
        profile: Option<&str>,
        level: ExecutionLevel,
    ) -> Result<Vec<&Profile>> {
        if let Some(id) = profile {
            let profile =
                self.profile(id).with_context(|| format!("unknown HIL profile '{id}'"))?;
            return Ok(vec![profile]);
        }

        Ok(self
            .lab
            .profiles
            .iter()
            .filter(|profile| match level {
                ExecutionLevel::Pr => !profile.physical,
                ExecutionLevel::Nightly | ExecutionLevel::Release => true,
            })
            .collect())
    }
}

fn load_cases(dir: &Path) -> Result<Vec<CaseDefinition>> {
    let mut paths = fs::read_dir(dir)
        .with_context(|| format!("read HIL cases directory {}", dir.display()))?
        .collect::<std::result::Result<Vec<_>, _>>()?
        .into_iter()
        .map(|entry| entry.path())
        .filter(|path| path.extension().and_then(|value| value.to_str()) == Some("toml"))
        .collect::<Vec<_>>();
    paths.sort();

    let mut cases = Vec::new();
    for path in paths {
        let text = fs::read_to_string(&path)
            .with_context(|| format!("read HIL case file {}", path.display()))?;
        let file: CaseFile = toml::from_str(&text)
            .with_context(|| format!("parse HIL case file {}", path.display()))?;
        cases.extend(file.cases);
    }
    Ok(cases)
}

#[derive(serde::Deserialize)]
struct CaseFile {
    #[serde(default)]
    cases: Vec<CaseDefinition>,
}

fn validate(lab: &LabConfig, cases: &[CaseDefinition]) -> Result<()> {
    if lab.version != 1 {
        bail!("unsupported HIL lab config version {}; expected 1", lab.version);
    }
    if lab.rack_id.trim().is_empty() {
        bail!("HIL rack_id must not be empty");
    }
    if lab.lock_ttl_secs == 0 {
        bail!("HIL lock_ttl_secs must be greater than zero");
    }
    if lab.reset_timeout_secs == 0 {
        bail!("HIL reset_timeout_secs must be greater than zero");
    }
    if lab.profiles.is_empty() {
        bail!("HIL lab must define at least one profile");
    }

    let mut profile_ids = BTreeSet::new();
    for profile in &lab.profiles {
        if profile.id.trim().is_empty() || !profile_ids.insert(profile.id.clone()) {
            bail!("HIL profile IDs must be non-empty and unique: {}", profile.id);
        }
        if profile.suites.is_empty() {
            bail!("HIL profile '{}' must define at least one suite", profile.id);
        }
        for env_name in [
            profile.identity_env.as_deref(),
            profile.endpoint_env.as_deref(),
            profile.firmware_env.as_deref(),
            profile.firmware_hash_env.as_deref(),
            profile.power_hub_env.as_deref(),
            profile.power_port_env.as_deref(),
            profile.reset_command_env.as_deref(),
            profile.executor_env.as_deref(),
        ]
        .into_iter()
        .flatten()
        {
            validate_env_name(env_name)?;
        }
    }

    let known_profiles = profile_ids;
    let mut case_ids = BTreeSet::new();
    for case in cases {
        if case.id.trim().is_empty() || !case_ids.insert(case.id.clone()) {
            bail!("HIL case IDs must be non-empty and unique: {}", case.id);
        }
        if case.suite.trim().is_empty() || case.levels.is_empty() {
            bail!("HIL case '{}' must define suite and levels", case.id);
        }
        for profile in &case.profiles {
            if !known_profiles.contains(profile) {
                bail!("HIL case '{}' references unknown profile '{}'", case.id, profile);
            }
        }
        for env_name in &case.requires_env {
            validate_env_name(env_name)?;
        }
    }
    Ok(())
}

fn validate_env_name(name: &str) -> Result<()> {
    if name.is_empty()
        || !name.chars().all(|character| {
            character.is_ascii_uppercase() || character.is_ascii_digit() || character == '_'
        })
    {
        bail!("invalid HIL environment variable name '{name}'");
    }
    Ok(())
}

pub fn missing_profile_environment(profile: &Profile, lab: &LabConfig) -> Vec<String> {
    let mut missing = Vec::new();
    for (label, name) in [
        ("identity", profile.identity_env.as_deref()),
        ("endpoint", profile.endpoint_env.as_deref()),
        ("firmware", profile.firmware_env.as_deref()),
        ("firmware-hash", profile.firmware_hash_env.as_deref()),
        ("executor", profile.executor_env.as_deref()),
    ]
    .into_iter()
    .filter_map(|(label, name)| name.map(|value| (label, value)))
    {
        if !has_nonempty_env(name) {
            missing.push(format!("{label}:{name}"));
        }
    }
    if profile.rf_required
        && std::env::var(&lab.rf_confirmation_env).ok().as_deref() != Some("true")
    {
        missing.push(format!("rf-confirmation:{}=true", lab.rf_confirmation_env));
    }
    let reset_ready = reset_hook_configured(profile)
        || profile
            .power_hub_env
            .as_deref()
            .zip(profile.power_port_env.as_deref())
            .is_some_and(|(hub, port)| has_nonempty_env(hub) && has_nonempty_env(port));
    if !reset_ready {
        missing.push("reset:command-or-uhubctl-mapping".to_string());
    }
    missing
}

pub fn has_nonempty_env(name: &str) -> bool {
    std::env::var_os(name).is_some_and(|value| has_nonempty_value(&value))
}

pub fn reset_hook_configured(profile: &Profile) -> bool {
    profile.reset_command_env.as_deref().is_some_and(has_nonempty_env)
}

fn has_nonempty_value(value: &OsStr) -> bool {
    !value.to_string_lossy().trim().is_empty()
}

pub fn case_applies(
    case: &CaseDefinition,
    profile: &Profile,
    level: ExecutionLevel,
    suite: Option<&str>,
) -> bool {
    case.levels.contains(&level)
        && suite.is_none_or(|value| value == case.suite)
        && (case.profiles.is_empty() || case.profiles.iter().any(|id| id == &profile.id))
        && profile.suites.iter().any(|value| value == &case.suite)
}

#[cfg(test)]
mod tests {
    use super::case_applies;
    use crate::hil::model::{CaseDefinition, ExecutionLevel, Profile};
    use std::ffi::OsStr;

    fn profile() -> Profile {
        Profile {
            id: "virtual".to_string(),
            label: "Virtual".to_string(),
            adapter: "virtual".to_string(),
            host: "linux".to_string(),
            physical: false,
            suites: vec!["core".to_string()],
            capabilities: Vec::new(),
            identity_env: None,
            endpoint_env: None,
            firmware_env: None,
            firmware_hash_env: None,
            power_hub_env: None,
            power_port_env: None,
            reset_command_env: None,
            executor_env: None,
            rf_required: false,
        }
    }

    #[test]
    fn case_filter_requires_level_suite_and_profile_support() {
        let case = CaseDefinition {
            id: "case".to_string(),
            suite: "core".to_string(),
            description: "case".to_string(),
            levels: vec![ExecutionLevel::Pr],
            profiles: Vec::new(),
            requires_env: Vec::new(),
            timeout_secs: None,
            failure_class: None,
            command: None,
        };
        assert!(case_applies(&case, &profile(), ExecutionLevel::Pr, Some("core")));
        assert!(!case_applies(&case, &profile(), ExecutionLevel::Nightly, Some("core")));
    }

    #[test]
    fn whitespace_environment_values_are_not_configured() {
        assert!(!super::has_nonempty_value(OsStr::new(" \t")));
        assert!(super::has_nonempty_value(OsStr::new("device-1")));
    }
}
