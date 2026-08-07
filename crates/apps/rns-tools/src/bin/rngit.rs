use clap::{Parser, Subcommand};
use std::collections::{BTreeMap, BTreeSet};
use std::io;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitStatus};
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Debug, Parser)]
#[command(name = "rngit", about = "Run local Git workflows prepared for Reticulum file transport")]
struct Cli {
    #[arg(long)]
    root: PathBuf,
    #[command(subcommand)]
    command: GitCommand,
}

#[derive(Debug, Subcommand)]
enum GitCommand {
    Init {
        path: PathBuf,
    },
    Status {
        path: PathBuf,
    },
    Bundle {
        path: PathBuf,
        output: PathBuf,
        #[arg(default_value = "--all")]
        revision: String,
    },
    Unbundle {
        path: PathBuf,
        bundle: PathBuf,
    },
}

pub fn main() -> std::process::ExitCode {
    match run(&Cli::parse()) {
        Ok(status) => std::process::ExitCode::from(status.code().unwrap_or(1) as u8),
        Err(error) => {
            eprintln!("rngit: {error}");
            std::process::ExitCode::FAILURE
        }
    }
}

fn run(cli: &Cli) -> io::Result<ExitStatus> {
    let root = cli.root.canonicalize()?;
    match &cli.command {
        GitCommand::Init { path } => git(&root, path, &["init"]),
        GitCommand::Status { path } => git(&root, path, &["status", "--short"]),
        GitCommand::Bundle { path, output, revision } => {
            let output = scoped(&root, output)?;
            git(&root, path, &["bundle", "create", output.to_string_lossy().as_ref(), revision])
        }
        GitCommand::Unbundle { path, bundle } => {
            let bundle = scoped(&root, bundle)?;
            git(&root, path, &["bundle", "unbundle", bundle.to_string_lossy().as_ref()])
        }
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
        escape_for_stdout, san_ref, san_refs, san_sha, PermissionTarget, RemoteGroup,
        RemoteRepository, RepositoryGroup, RepositoryRecord, ReticulumGitClient, ReticulumGitNode,
    };
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
        let node = ReticulumGitNode::default();
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

    #[test]
    fn local_git_bundle_fetch_and_push_use_the_registered_request_paths() {
        let temp = tempfile::tempdir().expect("tempdir");
        let source = temp.path().join("source");
        let group_path = temp.path().join("group");
        fs::create_dir_all(&source).expect("source");
        fs::create_dir_all(&group_path).expect("group");
        for args in [
            vec!["init", "-q"],
            vec!["config", "user.email", "rngit@example.invalid"],
            vec!["config", "user.name", "rngit-test"],
        ] {
            assert!(Command::new("git")
                .args(&args)
                .current_dir(&source)
                .status()
                .expect("git")
                .success());
        }
        fs::write(source.join("README"), "round trip").expect("file");
        for args in [vec!["add", "README"], vec!["commit", "-qm", "initial"]] {
            assert!(Command::new("git")
                .args(&args)
                .current_dir(&source)
                .status()
                .expect("git")
                .success());
        }

        let mut node = ReticulumGitNode::default();
        node.load_repository_group("group", &group_path).expect("load group");
        let group = node.groups.get_mut("group").expect("group state");
        group.permissions.read.add(PermissionTarget::All);
        group.permissions.write.add(PermissionTarget::All);
        group.permissions.create.add(PermissionTarget::All);
        let mut client = ReticulumGitClient::default();
        client.attach_local_node(node);
        let remote = "rns://00000000000000000000000000000000/group/repo";
        client.create_repository(remote).expect("create repository");

        let bundle = temp.path().join("source.bundle");
        assert!(Command::new("git")
            .args(["bundle", "create", bundle.to_string_lossy().as_ref(), "--all"])
            .current_dir(&source)
            .status()
            .expect("bundle")
            .success());
        let bundle = fs::read(bundle).expect("bundle bytes");
        let pushed = client
            .process_push_queue(remote, "refs/heads/master", "refs/heads/main", &bundle, false)
            .expect("push");
        assert_eq!(pushed.first().copied(), Some(ReticulumGitNode::RES_OK));
        let fetched =
            client.process_fetch_queue(remote, &["refs/heads/main".to_string()]).expect("fetch");
        assert_eq!(fetched.first().copied(), Some(ReticulumGitNode::RES_OK));
        assert!(fetched.len() > 1, "fetch should return the Git bundle payload");
    }
}
