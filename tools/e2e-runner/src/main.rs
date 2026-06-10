use anyhow::{bail, Context, Result};
use clap::{Parser, ValueEnum};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use testcontainers::compose::DockerCompose;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

const CONFIG_PATH: &str = "tools/e2e-bench/e2e.toml";
const DEFAULT_OUTPUT_ROOT: &str = "target/e2e-bench";
const CONTROL_PORT: u16 = 5678;

#[derive(Parser, Debug)]
#[command(name = "lxmf-e2e-runner")]
struct Cli {
    #[arg(long, value_enum, default_value_t = RunMode::All)]
    mode: RunMode,
    #[arg(long, value_enum, default_value_t = Profile::Smoke)]
    profile: Profile,
    #[arg(long)]
    scenario: Vec<String>,
    #[arg(long, value_enum)]
    implementation: Vec<Implementation>,
    #[arg(long)]
    keep: bool,
    #[arg(long)]
    output: Option<PathBuf>,
    #[arg(long)]
    dry_run: bool,
}

#[derive(Copy, Clone, Debug, Deserialize, Eq, PartialEq, Serialize, ValueEnum)]
#[serde(rename_all = "snake_case")]
enum RunMode {
    Correctness,
    Benchmark,
    All,
}

#[derive(Copy, Clone, Debug, Deserialize, Eq, PartialEq, Serialize, ValueEnum)]
#[serde(rename_all = "snake_case")]
enum Profile {
    Smoke,
    Report,
}

impl Profile {
    fn as_str(self) -> &'static str {
        match self {
            Self::Smoke => "smoke",
            Self::Report => "report",
        }
    }
}

#[derive(Copy, Clone, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize, ValueEnum)]
#[serde(rename_all = "snake_case")]
enum Implementation {
    Rust,
    Python,
    Tcp,
}

#[derive(Debug, Deserialize)]
struct Config {
    scenarios: Vec<Scenario>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct Scenario {
    id: String,
    mode: RunMode,
    profile: Profile,
    implementation: Implementation,
    topology: String,
    compose_file: PathBuf,
    expected_services: Vec<String>,
}

#[derive(Debug, Serialize)]
struct DryRunReport {
    mode: RunMode,
    profile: Profile,
    keep: bool,
    output: PathBuf,
    scenarios: Vec<Scenario>,
}

#[derive(Debug, Serialize)]
struct RunReport {
    status: &'static str,
    mode: RunMode,
    profile: Profile,
    started_unix_ms: u128,
    completed_unix_ms: u128,
    scenarios: Vec<ScenarioResult>,
}

#[derive(Debug, Serialize)]
struct ScenarioResult {
    id: String,
    topology: String,
    implementation: Implementation,
    status: &'static str,
    observed_services: Vec<String>,
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    let repo_root = repo_root()?;
    let config = load_config(&repo_root)?;
    let scenarios = select_scenarios(&config, &cli)?;
    let output = cli.output.clone().unwrap_or_else(default_output_dir);

    if cli.dry_run {
        let report = DryRunReport {
            mode: cli.mode,
            profile: cli.profile,
            keep: cli.keep,
            output,
            scenarios,
        };
        println!("{}", serde_json::to_string_pretty(&report)?);
        return Ok(());
    }

    fs::create_dir_all(&output)
        .with_context(|| format!("failed to create output directory {}", output.display()))?;
    let started_unix_ms = unix_millis();
    let mut results = Vec::with_capacity(scenarios.len());
    for scenario in scenarios {
        results.push(run_scenario(&repo_root, &scenario, cli.keep).await?);
    }
    let report = RunReport {
        status: "pass",
        mode: cli.mode,
        profile: cli.profile,
        started_unix_ms,
        completed_unix_ms: unix_millis(),
        scenarios: results,
    };
    let report_path = output.join("run.json");
    fs::write(&report_path, serde_json::to_vec_pretty(&report)?)
        .with_context(|| format!("failed to write {}", report_path.display()))?;
    println!("E2E runner pass: {}", report_path.display());
    Ok(())
}

fn repo_root() -> Result<PathBuf> {
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    manifest_dir
        .ancestors()
        .nth(2)
        .map(Path::to_path_buf)
        .context("runner manifest is not under <repo>/tools/e2e-runner")
}

fn load_config(repo_root: &Path) -> Result<Config> {
    let path = repo_root.join(CONFIG_PATH);
    let raw =
        fs::read_to_string(&path).with_context(|| format!("failed to read {}", path.display()))?;
    toml::from_str(&raw).with_context(|| format!("failed to parse {}", path.display()))
}

fn select_scenarios(config: &Config, cli: &Cli) -> Result<Vec<Scenario>> {
    let scenario_filter = cli.scenario.iter().cloned().collect::<BTreeSet<_>>();
    let implementation_filter = cli.implementation.iter().copied().collect::<BTreeSet<_>>();
    let selected = config
        .scenarios
        .iter()
        .filter(|scenario| cli.mode == RunMode::All || scenario.mode == cli.mode)
        .filter(|scenario| scenario.profile == cli.profile)
        .filter(|scenario| scenario_filter.is_empty() || scenario_filter.contains(&scenario.id))
        .filter(|scenario| {
            implementation_filter.is_empty()
                || implementation_filter.contains(&scenario.implementation)
        })
        .cloned()
        .collect::<Vec<_>>();

    if selected.is_empty() {
        bail!("no scenarios matched profile '{}' and the requested filters", cli.profile.as_str());
    }
    Ok(selected)
}

async fn run_scenario(repo_root: &Path, scenario: &Scenario, keep: bool) -> Result<ScenarioResult> {
    let compose_path = repo_root.join(&scenario.compose_file);
    if !compose_path.is_file() {
        bail!("scenario '{}' compose file does not exist: {}", scenario.id, compose_path.display());
    }
    if keep {
        std::env::set_var("TESTCONTAINERS_COMMAND", "keep");
    }

    let mut compose = DockerCompose::with_auto_client(&[compose_path.as_path()])
        .await
        .context("failed to initialize Docker Compose client")?
        .with_wait(false);
    compose.up().await.with_context(|| format!("failed to start scenario '{}'", scenario.id))?;

    let observed_services =
        compose.services().into_iter().map(str::to_owned).collect::<BTreeSet<_>>();
    for expected in &scenario.expected_services {
        if !observed_services.contains(expected) {
            bail!("scenario '{}' did not discover expected service '{}'", scenario.id, expected);
        }
    }

    if scenario.topology == "c1_tcp_control" {
        verify_tcp_control(&compose).await?;
    }

    let observed_services = observed_services.into_iter().collect::<Vec<_>>();
    if !keep {
        compose
            .down()
            .await
            .with_context(|| format!("failed to stop scenario '{}'", scenario.id))?;
    }
    Ok(ScenarioResult {
        id: scenario.id.clone(),
        topology: scenario.topology.clone(),
        implementation: scenario.implementation,
        status: "pass",
        observed_services,
    })
}

async fn verify_tcp_control(compose: &DockerCompose) -> Result<()> {
    let service =
        compose.service("tcp_control").context("tcp_control service was not discovered")?;
    let port = service
        .get_host_port_ipv4(CONTROL_PORT)
        .await
        .context("tcp_control port was not published")?;
    let host = service.get_host().await.context("tcp_control host was not discovered")?.to_string();
    let deadline = tokio::time::Instant::now() + Duration::from_secs(15);

    loop {
        match request_control(&host, port).await {
            Ok(response) if response.contains("lxmf-e2e-ok") => return Ok(()),
            Ok(_) => {}
            Err(error)
                if matches!(
                    error.kind(),
                    ErrorKind::ConnectionRefused
                        | ErrorKind::ConnectionReset
                        | ErrorKind::TimedOut
                        | ErrorKind::WouldBlock
                ) => {}
            Err(error) => return Err(error).context("tcp_control request failed"),
        }
        if tokio::time::Instant::now() >= deadline {
            bail!("tcp_control did not return the expected response within 15 seconds");
        }
        tokio::time::sleep(Duration::from_millis(250)).await;
    }
}

async fn request_control(host: &str, port: u16) -> std::io::Result<String> {
    let mut stream = TcpStream::connect((host, port)).await?;
    stream.write_all(b"GET / HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n").await?;
    let mut response = Vec::new();
    stream.read_to_end(&mut response).await?;
    Ok(String::from_utf8_lossy(&response).into_owned())
}

fn default_output_dir() -> PathBuf {
    Path::new(DEFAULT_OUTPUT_ROOT).join(unix_millis().to_string())
}

fn unix_millis() -> u128 {
    SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_millis()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config() -> Config {
        Config {
            scenarios: vec![
                Scenario {
                    id: "smoke-tcp".to_string(),
                    mode: RunMode::Correctness,
                    profile: Profile::Smoke,
                    implementation: Implementation::Tcp,
                    topology: "c1_tcp_control".to_string(),
                    compose_file: "compose.yml".into(),
                    expected_services: vec!["tcp_control".to_string()],
                },
                Scenario {
                    id: "report-tcp".to_string(),
                    mode: RunMode::Benchmark,
                    profile: Profile::Report,
                    implementation: Implementation::Tcp,
                    topology: "c1_tcp_control".to_string(),
                    compose_file: "compose.yml".into(),
                    expected_services: vec!["tcp_control".to_string()],
                },
            ],
        }
    }

    fn cli() -> Cli {
        Cli {
            mode: RunMode::All,
            profile: Profile::Smoke,
            scenario: Vec::new(),
            implementation: Vec::new(),
            keep: false,
            output: None,
            dry_run: true,
        }
    }

    #[test]
    fn selection_respects_profile() {
        let selected = select_scenarios(&config(), &cli()).expect("selection");
        assert_eq!(selected.len(), 1);
        assert_eq!(selected[0].id, "smoke-tcp");
    }

    #[test]
    fn selection_rejects_empty_filters() {
        let mut cli = cli();
        cli.scenario.push("missing".to_string());
        let error = select_scenarios(&config(), &cli).expect_err("empty selection");
        assert!(error.to_string().contains("no scenarios matched"));
    }

    #[test]
    fn selection_respects_mode() {
        let mut cli = cli();
        cli.mode = RunMode::Benchmark;
        let error = select_scenarios(&config(), &cli).expect_err("empty selection");
        assert!(error.to_string().contains("no scenarios matched"));
    }
}
