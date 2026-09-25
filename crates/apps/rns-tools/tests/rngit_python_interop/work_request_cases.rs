pub(super) const PYTHON: &str = r#"
# Pin the bounded malformed/missing/denied request response cases for work.
malformed_list = request(
    "/mgmt/work",
    {0: "group/repo", "operation": "list", "scope": "unknown"},
)
missing_view_id = request(
    "/mgmt/work",
    {0: "group/repo", "operation": "view", "scope": "active"},
)
malformed_view_id = request(
    "/mgmt/work",
    {0: "group/repo", "operation": "view", "doc_id": "not-an-id"},
)
missing_document = request(
    "/mgmt/work",
    {0: "group/repo", "operation": "view", "doc_id": 999999, "scope": "active"},
)
denied_read = request(
    "/mgmt/work",
    {0: "private/repo", "operation": "list", "scope": "active"},
)
if malformed_list[0] != 0:
    raise RuntimeError(f"unknown work-list scope was not accepted: {malformed_list!r}")
empty_listing = mp.unpackb(malformed_list[1:])
if empty_listing != {"active": [], "completed": [], "proposed": []}:
    raise RuntimeError(f"unknown work-list scope did not return empty scopes: {empty_listing!r}")
expected_errors = [
    ("missing view ID", missing_view_id, 2, b"No document ID specified"),
    ("malformed view ID", malformed_view_id, 2, b"Invalid request"),
    ("missing document", missing_document, 3, b"Not found"),
    ("denied read", denied_read, 3, b"Not found"),
]
for label, response, status, message in expected_errors:
    if response[0] != status or response[1:] != message:
        raise RuntimeError(
            f"work {label} response mismatch: status={response[0]} body={response[1:]!r}"
        )
"#;
