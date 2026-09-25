use clap::{Parser, Subcommand};
use std::collections::{BTreeMap, BTreeSet};
use std::io::{self, Read};
use std::path::{Path, PathBuf};
use std::process::{Command, ExitStatus, Stdio};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

#[path = "rngit_parts/page_git_output.rs"]
mod page_git_output;

#[path = "rngit_parts/media_config.rs"]
mod media_config;

mod rngit_network {
    include!("rngit_parts/network.rs");
}

mod rngit_remote_helper {
    include!("rngit_parts/remote_helper.rs");
}

include!("rngit_parts/cli.rs");

pub fn main() -> std::process::ExitCode {
    if std::env::current_exe()
        .ok()
        .and_then(|path| path.file_stem().map(|name| name == "git-remote-rns"))
        .unwrap_or(false)
    {
        return match rngit_remote_helper::run() {
            Ok(()) => std::process::ExitCode::SUCCESS,
            Err(error) => {
                eprintln!("git-remote-rns: {error}");
                std::process::ExitCode::FAILURE
            }
        };
    }
    let cli = Cli::parse();
    if cli.network_mode() {
        return match rngit_network::run(&cli) {
            Ok(()) => std::process::ExitCode::SUCCESS,
            Err(error) => {
                eprintln!("rngit: {error}");
                std::process::ExitCode::FAILURE
            }
        };
    }
    match run(&cli) {
        Ok(status) => std::process::ExitCode::from(status.code().unwrap_or(1) as u8),
        Err(error) => {
            eprintln!("rngit: {error}");
            std::process::ExitCode::FAILURE
        }
    }
}

fn run(cli: &Cli) -> io::Result<ExitStatus> {
    let root = cli.root.canonicalize()?;
    let Some(command) = &cli.command else {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "missing Git subcommand or network interface",
        ));
    };
    match command {
        GitCommand::Init { path } => git(&root, path, &["init"]),
        GitCommand::Status { path } => git(&root, path, &["status", "--short"]),
        GitCommand::Bundle { path, output, revision } => {
            let output = scoped(&root, output)?;
            git(&root, path, &["bundle", "create", output.to_string_lossy().as_ref(), revision])
        }
        GitCommand::Fetch { .. } => {
            Err(io::Error::new(io::ErrorKind::InvalidInput, "network Git fetch requires --connect"))
        }
        GitCommand::Push { .. } => {
            Err(io::Error::new(io::ErrorKind::InvalidInput, "network Git push requires --connect"))
        }
        GitCommand::Unbundle { path, bundle } => {
            let bundle = scoped(&root, bundle)?;
            git(&root, path, &["bundle", "unbundle", bundle.to_string_lossy().as_ref()])
        }
    }
}

impl Cli {
    fn network_mode(&self) -> bool {
        !self.listen.is_empty()
            || !self.connect.is_empty()
            || self.print_identity
            || self.identity_seed.is_some()
            || self.identity.is_some()
            || matches!(self.command.as_ref(), Some(GitCommand::Fetch { .. }))
            || matches!(self.command.as_ref(), Some(GitCommand::Push { .. }))
    }
}

fn git(root: &Path, repository: &Path, args: &[&str]) -> io::Result<ExitStatus> {
    let repository = scoped(root, repository)?;
    Command::new("git").arg("-C").arg(repository).args(args).status()
}

fn scoped(root: &Path, path: &Path) -> io::Result<PathBuf> {
    if path.components().any(|component| component == std::path::Component::ParentDir) {
        return Err(io::Error::new(io::ErrorKind::PermissionDenied, "parent traversal is denied"));
    }
    let candidate = if path.is_absolute() { path.to_path_buf() } else { root.join(path) };
    if !candidate.starts_with(root) {
        return Err(io::Error::new(io::ErrorKind::PermissionDenied, "path escapes workflow root"));
    }
    Ok(candidate)
}

/// Validate a Git reference using the same conservative contract as Python's
/// `RNS.Utilities.rngit.util.san_ref`.
pub fn san_ref(reference: &str) -> Option<&str> {
    if reference.starts_with('-')
        || reference.starts_with('/')
        || reference.ends_with('/')
        || reference.ends_with('.')
        || reference.contains(' ')
        || !reference.contains('/')
        || reference.contains("..")
        || reference.contains("/.")
        || reference.contains("//")
        || reference.contains('\\')
        || reference.contains('\u{7f}')
        || reference.contains('~')
        || reference.contains('^')
        || reference.contains(':')
        || reference.contains('?')
        || reference.contains('*')
        || reference.contains('[')
        || reference.contains("@{")
        || reference == "@"
        || !reference.chars().all(|character| character as u32 >= 40)
        || reference.split('/').any(|component| component.ends_with(".lock"))
    {
        None
    } else {
        Some(reference)
    }
}

/// Validate a list of Git references without changing its ownership.
pub fn san_refs(references: &[String]) -> Option<&[String]> {
    references.iter().all(|reference| san_ref(reference).is_some()).then_some(references)
}

/// Validate a hexadecimal Git object ID using Python's minimum 40-character
/// contract. Longer IDs are accepted when they are valid hexadecimal.
pub fn san_sha(sha: &str) -> Option<&str> {
    (sha.len() >= 40 && hex::decode(sha).is_ok()).then_some(sha)
}

/// Quote a value for Git's line-oriented stdout protocol.
pub fn escape_for_stdout(value: &[u8]) -> String {
    let value = String::from_utf8_lossy(value);
    let mut escaped = String::from('"');
    for character in value.chars() {
        match character {
            '\\' => escaped.push_str("\\\\"),
            '"' => escaped.push_str("\\\""),
            '\n' => escaped.push_str("\\n"),
            '\t' => escaped.push_str("\\t"),
            '\r' => escaped.push_str("\\r"),
            character if (character as u32) < 32 || (character as u32) > 126 => {
                escaped.push_str(&format!("\\x{:02x}", character as u32));
            }
            character => escaped.push(character),
        }
    }
    escaped.push('"');
    escaped
}

pub fn program_setup(mut node: ReticulumGitNode) -> ReticulumGitNode {
    node.start();
    node
}

include!("rngit_parts/compat.rs");

#[cfg(test)]
mod tests {
    use super::{
        escape_for_stdout, map_value, san_ref, san_refs, san_sha, PermissionTarget, RemoteGroup,
        RemoteRepository, RepositoryGroup, RepositoryRecord, ReticulumGitClient, ReticulumGitNode,
    };
    use rns_transport::destination::link::LinkStatus;
    use std::collections::BTreeMap;
    use std::fs;
    use std::process::Command;

    #[test]
    fn san_ref_matches_python_git_reference_guards() {
        assert_eq!(san_ref("refs/heads/main"), Some("refs/heads/main"));
        for invalid in
            ["main", "refs//main", "refs/heads/a..b", "refs/heads/a.lock", "refs/heads/a~1"]
        {
            assert_eq!(san_ref(invalid), None, "{invalid} should be rejected");
        }
    }

    #[test]
    fn san_refs_and_san_sha_preserve_valid_input() {
        let references = vec!["refs/heads/main".to_owned(), "refs/tags/v1".to_owned()];
        assert!(san_refs(&references).is_some());
        assert!(san_refs(&["main".to_owned()]).is_none());
        assert!(san_sha(&"ab".repeat(20)).is_some());
        assert!(san_sha("not-a-sha").is_none());
    }

    #[test]
    fn escape_for_stdout_matches_python_quoting() {
        assert_eq!(escape_for_stdout(b"a\\\"\n"), "\"a\\\\\\\"\\n\"");
        assert_eq!(escape_for_stdout("caf\u{e9}".as_bytes()), "\"caf\\xe9\"");
    }

    #[test]
    fn remote_url_parsing_matches_python_rngit_shapes_and_aliases() {
        let mut client = ReticulumGitClient::default();
        client
            .destination_aliases
            .insert("node".to_string(), "00112233445566778899aabbccddeeff".to_string());
        let repository = client.parse_remote_url("RNS://node/group/repo").expect("repository URL");
        assert_eq!(
            repository,
            RemoteRepository {
                destination: [
                    0x00, 0x11, 0x22, 0x33, 0x44, 0x55, 0x66, 0x77, 0x88, 0x99, 0xaa, 0xbb, 0xcc,
                    0xdd, 0xee, 0xff
                ],
                group: "group".to_string(),
                repository: "repo".to_string(),
            }
        );
        assert_eq!(
            client.parse_remote_group_url("rns://node/group").expect("group URL"),
            RemoteGroup { destination: repository.destination, group: "group".to_string() }
        );
        assert_eq!(
            client.parse_remote_destination_url("rns://node").expect("destination"),
            repository.destination
        );
        assert!(client.parse_remote_url("http://node/group/repo").is_err());
        assert!(client.parse_remote_url("rns://node/group").is_err());
    }

    #[test]
    fn permission_and_path_parsing_match_pinned_rngit_server() {
        let mut node = ReticulumGitNode::default();
        node.set_identity_alias("owner", [0xabu8; 16]);
        assert_eq!(
            node.parse_permission("rw:all"),
            Some((ReticulumGitNode::PERM_READWRITE, PermissionTarget::All))
        );
        assert_eq!(
            node.parse_permission("admin:none"),
            Some((ReticulumGitNode::PERM_ADMIN, PermissionTarget::None))
        );
        assert_eq!(
            node.parse_permission("read:00112233445566778899aabbccddeeff"),
            Some((
                ReticulumGitNode::PERM_READ,
                PermissionTarget::Identity([
                    0x00, 0x11, 0x22, 0x33, 0x44, 0x55, 0x66, 0x77, 0x88, 0x99, 0xaa, 0xbb, 0xcc,
                    0xdd, 0xee, 0xff
                ])
            ))
        );
        assert!(node.parse_permission("read:bad").is_none());
        assert_eq!(
            node.parse_permission("read:owner"),
            Some((ReticulumGitNode::PERM_READ, PermissionTarget::Identity([0xabu8; 16])))
        );
        assert_eq!(
            node.parse_request_repository_path("group/repo"),
            Some(("group".to_string(), "repo".to_string()))
        );
        assert_eq!(node.parse_request_group_path("group"), Some("group".to_string()));
        assert!(node.parse_request_repository_path("group/repo/extra").is_none());
        assert!(node.parse_request_group_path("group/repo").is_none());
    }

    #[test]
    fn permission_resolution_obeys_repository_group_and_admin_fallbacks() {
        let identity = [7_u8; 16];
        let mut node = ReticulumGitNode::default();
        let mut group_permissions = node.permissions_from_allowed_input(Some("read:all"));
        group_permissions.admin.add(PermissionTarget::Identity(identity));
        let mut repository_permissions = node.permissions_from_allowed_input(Some("write:none"));
        repository_permissions.admin.add(PermissionTarget::Identity(identity));
        node.groups.insert(
            "group".to_string(),
            RepositoryGroup {
                name: "group".to_string(),
                path: "group".into(),
                permissions: group_permissions,
                repositories: BTreeMap::from([(
                    "repo".to_string(),
                    RepositoryRecord {
                        name: "repo".to_string(),
                        path: "group/repo".into(),
                        fork: None,
                        mirror: None,
                        permissions: repository_permissions,
                    },
                )]),
            },
        );
        assert!(node.resolve_permission(&identity, "group", "repo", ReticulumGitNode::PERM_READ));
        assert!(!node.resolve_permission(&identity, "group", "repo", ReticulumGitNode::PERM_WRITE));
        assert!(node.resolve_group_permission(&identity, "group", ReticulumGitNode::PERM_READ));
        node.blocked_identities.insert(identity);
        assert!(!node.resolve_permission(&identity, "group", "repo", ReticulumGitNode::PERM_READ));
        assert!(!node.resolve_group_permission(&identity, "group", ReticulumGitNode::PERM_READ));
    }

    #[test]
    fn permission_updates_refresh_in_memory_group_and_repository_state() {
        let temp = tempfile::tempdir().expect("tempdir");
        let group_path = temp.path().join("group");
        let repository_path = group_path.join("repo");
        fs::create_dir_all(&group_path).expect("group");
        let identity = [7_u8; 16];
        let mut node = ReticulumGitNode::default();
        node.groups.insert(
            "group".to_string(),
            RepositoryGroup {
                name: "group".to_string(),
                path: group_path.clone(),
                permissions: node.permissions_from_allowed_input(Some("read:all\nadmin:all")),
                repositories: BTreeMap::from([(
                    "repo".to_string(),
                    RepositoryRecord {
                        name: "repo".to_string(),
                        path: repository_path,
                        fork: None,
                        mirror: None,
                        permissions: node
                            .permissions_from_allowed_input(Some("write:all\nadmin:all")),
                    },
                )]),
            },
        );

        assert!(node.resolve_group_permission(&identity, "group", ReticulumGitNode::PERM_READ));
        assert!(node.resolve_permission(&identity, "group", "repo", ReticulumGitNode::PERM_WRITE));

        let group_request = vec![
            (rmpv::Value::String("operation".into()), rmpv::Value::String("gperms".into())),
            (rmpv::Value::from(2_u64), rmpv::Value::String("group".into())),
            (rmpv::Value::String("step".into()), rmpv::Value::String("set".into())),
            (
                rmpv::Value::String("content".into()),
                rmpv::Value::String("read:none\nadmin:all".into()),
            ),
        ];
        assert_eq!(
            node.handle_permission_request(&group_request, identity)[0],
            ReticulumGitNode::RES_OK
        );
        assert!(!node.resolve_group_permission(&identity, "group", ReticulumGitNode::PERM_READ));

        let repository_request = vec![
            (rmpv::Value::String("operation".into()), rmpv::Value::String("rperms".into())),
            (rmpv::Value::from(0_u64), rmpv::Value::String("group/repo".into())),
            (rmpv::Value::String("step".into()), rmpv::Value::String("set".into())),
            (
                rmpv::Value::String("content".into()),
                rmpv::Value::String("write:none\nadmin:all".into()),
            ),
        ];
        assert_eq!(
            node.handle_permission_request(&repository_request, identity)[0],
            ReticulumGitNode::RES_OK
        );
        assert!(!node.resolve_permission(&identity, "group", "repo", ReticulumGitNode::PERM_WRITE));
    }

    include!("rngit_parts/issue_612_tests.rs");
    include!("rngit_parts/issue_612_executable_python_differential_tests.rs");
    include!("rngit_parts/issue_612_companion_collision_tests.rs");
    include!("rngit_parts/issue_612_permission_failure_tests.rs");
    include!("rngit_parts/issue_612_work_storage_failure_tests.rs");
    include!("rngit_parts/issue_612_concurrency_tests.rs");
    include!("rngit_parts/issue_613_tests.rs");
    include!("rngit_parts/issue_613_pagination_tests.rs");
    include!("rngit_parts/page_git_output_tests.rs");

    #[test]
    fn statistics_hooks_record_python_rngit_event_buckets() {
        let mut node = ReticulumGitNode::default();
        node.record_page_view("front");
        node.record_group_view("group");
        node.record_repository_view("group", "repo");
        node.record_fetch("group", "repo");
        node.record_push("group", "repo");
        node.record_download("group", "repo");
        node.record_release_download("group", "repo");
        assert!(node.stats.pages["front"].values().any(|value| *value == 1));
        let repo = &node.stats.groups["group"]["repo"];
        assert!(repo.keys().any(|key| key.starts_with("view:")));
        assert!(repo.keys().any(|key| key.starts_with("release_download:")));
    }

    #[test]
    fn rns_1_5_rngit_adaptive_timeout_is_a_lower_bound() {
        let mut client = ReticulumGitClient::default();
        client.set_medium_path_timeout(42.1);
        client
            .connect_remote("rns://00112233445566778899aabbccddeeff/group/repo")
            .expect("remote connection setup");
        assert_eq!(client.path_timeout_secs, 43);
        assert_eq!(client.link_timeout_secs, 43);
        client.apply_medium_path_timeout(2.0);
        assert_eq!(client.path_timeout_secs, 43);
        assert_eq!(client.link_timeout_secs, 43);
    }

    #[test]
    fn local_client_and_node_roundtrip_repository_creation_and_listing() {
        let temp = tempfile::tempdir().expect("tempdir");
        let group_path = temp.path().join("group");
        fs::create_dir_all(&group_path).expect("group");
        let mut node = ReticulumGitNode::default();
        node.load_repository_group("group", &group_path).expect("load group");
        let group = node.groups.get_mut("group").expect("group state");
        group.permissions.read.add(PermissionTarget::All);
        group.permissions.write.add(PermissionTarget::All);
        group.permissions.create.add(PermissionTarget::All);
        group.permissions.release.add(PermissionTarget::All);
        group.permissions.admin.add(PermissionTarget::All);

        let mut client = ReticulumGitClient::default();
        client.attach_local_node(node);
        let remote = "rns://00000000000000000000000000000000/group/repo";
        assert_eq!(client.create_repository(remote).expect("create")[0], ReticulumGitNode::RES_OK);
        let listing =
            client.request_repository(super::RNGIT_PATH_LIST, "group/repo", []).expect("list");
        assert_eq!(listing.first().copied(), Some(ReticulumGitNode::RES_OK));
        assert!(String::from_utf8_lossy(&listing[1..]).contains("HEAD"));
    }

    #[test]
    fn local_work_release_and_permission_requests_are_service_backed() {
        let temp = tempfile::tempdir().expect("tempdir");
        let group_path = temp.path().join("group");
        fs::create_dir_all(&group_path).expect("group");
        let mut node = ReticulumGitNode::default();
        node.load_repository_group("group", &group_path).expect("load group");
        let group = node.groups.get_mut("group").expect("group state");
        for permissions in [
            &mut group.permissions.read,
            &mut group.permissions.write,
            &mut group.permissions.create,
            &mut group.permissions.release,
            &mut group.permissions.admin,
        ] {
            permissions.add(PermissionTarget::All);
        }

        let mut client = ReticulumGitClient::default();
        client.attach_local_node(node);
        let remote = "rns://00000000000000000000000000000000/group/repo";
        client.create_repository(remote).expect("create repository");
        let created = client.work_create(remote, "Title", "Body").expect("create work document");
        assert_eq!(created.first().copied(), Some(ReticulumGitNode::RES_OK));
        let listed = client.work_list(remote, "active").expect("list work documents");
        assert_eq!(listed.first().copied(), Some(ReticulumGitNode::RES_OK));
        let released = client.create_release(remote, "v1").expect("create release");
        assert_eq!(released.first().copied(), Some(ReticulumGitNode::RES_OK));
        let releases = client.list_releases(remote).expect("list releases");
        assert_eq!(releases.first().copied(), Some(ReticulumGitNode::RES_OK));
        let permissions = client.repository_permissions(remote).expect("get permissions");
        assert_eq!(permissions.first().copied(), Some(ReticulumGitNode::RES_OK));
    }

    include!("rngit_parts/document_permissions_tests.rs");
    include!("rngit_parts/rns_1_5_4_tests.rs");
    include!("rngit_parts/git_bundle_tests.rs");
    include!("rngit_parts/native_client_tests.rs");
}
