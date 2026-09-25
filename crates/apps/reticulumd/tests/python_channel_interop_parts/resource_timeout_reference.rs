#[test]
#[ignore = "requires pinned Python Reticulum checkout"]
fn pinned_python_resource_cancels_after_missing_part_retry_budget() {
    const PINNED_RETICULUM: &str = "99de23c040d507e3fefca19e87b182302902725d";
    const PYTHON_TEST: &str = r#"
import importlib
import types

resource_module = importlib.import_module("RNS.Resource")
Resource = resource_module.Resource

# Patching is confined to this subprocess. The watchdog is invoked directly,
# so no Python thread or real clock/sleep is started.
clock_reads = []
sleep_calls = []
resource_module.time.time = lambda: clock_reads.append(1000.0) or 1000.0
resource_module.sleep = sleep_calls.append

resource = Resource.__new__(Resource)
resource.status = Resource.TRANSFERRING
resource.initiator = False
resource.parts = [b"received-part", None]
resource.sent_parts = 0
resource.max_retries = 1
resource.retries_left = 0
resource.outstanding_parts = 1
resource.sdu = 1
resource.waiting_for_hmu = False
resource.req_resp_rtt_rate = 0
resource.part_timeout_factor = 1.0
resource.last_activity = 0.0
resource.watchdog_lock = False
resource._Resource__watchdog_job_id = 0
resource._Resource__progress_callback = None
resource.callback = None
resource.next_segment = None
resource.eifr = 1000.0
resource.update_eifr = types.MethodType(lambda self: None, resource)

class InactiveLink:
    status = 0
    def __init__(self):
        self.incoming_resources = [resource]
    def cancel_incoming_resource(self, candidate):
        assert candidate is resource
        self.incoming_resources.remove(candidate)

link = InactiveLink()
resource.link = link
cancel_statuses = []
original_cancel = resource.cancel
def track_cancel():
    cancel_statuses.append(resource.status)
    original_cancel()
resource.cancel = track_cancel

resource._Resource__watchdog_job()

assert resource.parts == [b"received-part", None], "fixture must model one of two parts"
assert cancel_statuses == [Resource.TRANSFERRING], "watchdog must cancel exactly once from TRANSFERRING"
assert resource.status == Resource.FAILED, "exhausted retry budget must become FAILED"
assert link.incoming_resources == [], "cancel must remove the inbound Resource from its Link"
assert len(clock_reads) == 1, "use the fake clock exactly for the missing-part timeout"
assert sleep_calls == [0.001], "only the watchdog's post-transition poll is requested; fake sleep performs no delay"
"#;

    let python_bin = std::env::var("LXMF_PYTHON_BIN").unwrap_or_else(|_| "python3".to_string());
    let reticulum_repo = std::env::var("RETICULUM_PY_REPO")
        .expect("set RETICULUM_PY_REPO to the frozen Reticulum checkout");
    let revision = std::process::Command::new("git")
        .args(["-C", &reticulum_repo, "rev-parse", "HEAD"])
        .output()
        .expect("read pinned Reticulum revision");
    assert!(revision.status.success(), "could not inspect RETICULUM_PY_REPO revision");
    assert_eq!(
        String::from_utf8_lossy(&revision.stdout).trim(),
        PINNED_RETICULUM,
        "reference differential must use the frozen Resource implementation"
    );

    let output = std::process::Command::new(python_bin)
        .arg("-c")
        .arg(PYTHON_TEST)
        .env("PYTHONPATH", &reticulum_repo)
        .output()
        .expect("run pinned Python Resource watchdog transition");
    assert!(
        output.status.success(),
        "pinned Python timeout differential failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}
