use super::{
    create_repository_fixture, free_port, python_bin, python_repo, rust_destination, wait_for_port,
    write_python_config,
};
use std::fs;
use std::io;
use std::os::unix::fs::PermissionsExt;
use std::process::{Command, Stdio};

#[test]
#[ignore = "requires local pinned Python Reticulum checkout and Linux cc"]
fn rngit_media_output_open_failure_returns_raw_resource() -> io::Result<()> {
    let _test_guard =
        super::PYTHON_INTEROP_TEST_LOCK.lock().expect("Python interop test lock poisoned");
    let temp = tempfile::tempdir()?;
    let root = create_repository_fixture(temp.path())?;
    let python_repo = python_repo();
    if !python_repo.join("RNS/Utilities/rngit/pages.py").is_file() {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            format!("pinned Python Reticulum checkout not found: {}", python_repo.display()),
        ));
    }
    let reference_media = fs::read_to_string(python_repo.join("RNS/Utilities/rngit/media.py"))?;
    assert!(
        reference_media.contains("except Exception as e:")
            && reference_media.contains("Error during media conversion")
            && reference_media.contains("return False"),
        "pinned media.py must treat output-open errors as conversion failure"
    );
    let reference_pages = fs::read_to_string(python_repo.join("RNS/Utilities/rngit/pages.py"))?;
    assert!(
        reference_pages.contains(
            "if not stream: stream = self.get_blob_stream(repo_path, resolved_ref, file_path)"
        ),
        "pinned pages.py must fall back to the original blob stream after conversion failure"
    );

    let shim_source = temp.path().join("deny_media_output_open.c");
    let shim_library = temp.path().join("deny_media_output_open.so");
    let interception_marker = temp.path().join("output-open-denied");
    fs::write(&shim_source, PRELOAD_SHIM)?;
    let compile = Command::new("cc")
        .args(["-shared", "-fPIC", "-O2", "-Wall", "-Wextra", "-Werror"])
        .arg(&shim_source)
        .args(["-o"])
        .arg(&shim_library)
        .output()?;
    if !compile.status.success() {
        return Err(io::Error::other(format!(
            "could not compile output-open fault shim: {}",
            String::from_utf8_lossy(&compile.stderr)
        )));
    }

    let wrapper_dir = temp.path().join("bin");
    fs::create_dir(&wrapper_dir)?;
    let encoder_marker = temp.path().join("encoder-invoked");
    let encoder = wrapper_dir.join("ffmpeg");
    fs::write(
        &encoder,
        format!(
            "#!/bin/sh\ntouch '{}'\ncat >/dev/null\nprintf 'RIFF\\036\\000\\000\\000WEBPVP8X\\000\\000\\000\\000\\000\\000\\000\\000\\000\\000\\000\\000\\000\\000\\000\\000'\n",
            encoder_marker.display()
        ),
    )?;
    let mut permissions = fs::metadata(&encoder)?.permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(&encoder, permissions)?;

    let port = free_port()?;
    let identity_seed = "rngit-python-media-output-open-failure";
    let mut server = Command::new(env!("CARGO_BIN_EXE_rngit"))
        .args([
            "--root",
            root.to_string_lossy().as_ref(),
            "--listen",
            &format!("127.0.0.1:{port}"),
            "--identity-seed",
            identity_seed,
            "--silent",
        ])
        .env("RNGIT_MEDIA_BACKEND", "ffmpeg")
        .env("PATH", format!("{}:/usr/bin:/bin", wrapper_dir.display()))
        .env("LD_PRELOAD", &shim_library)
        .env("RNGIT_OPEN_FAILURE_MARKER", &interception_marker)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()?;

    let result = (|| {
        wait_for_port(port, &mut server)?;
        let destination = rust_destination(&root, identity_seed)?;
        let config_dir = temp.path().join("python-client");
        fs::create_dir_all(&config_dir)?;
        write_python_config(&config_dir, port)?;
        let output = Command::new(python_bin())
            .arg("-c")
            .arg(PYTHON_CLIENT)
            .arg(&config_dir)
            .arg(&destination)
            .env("PYTHONPATH", &python_repo)
            .output()?;
        if !output.status.success() {
            return Err(io::Error::other(format!(
                "Python media output-open-failure client failed: {}\nstdout:\n{}\nstderr:\n{}",
                output.status,
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            )));
        }

        let response: serde_json::Value = serde_json::from_slice(&output.stdout).map_err(|error| {
            io::Error::other(format!(
                "Python media output-open-failure client returned invalid JSON: {error}\nstdout:\n{}",
                String::from_utf8_lossy(&output.stdout)
            ))
        })?;
        assert_eq!(response["name"], "image.png", "response: {response}");
        assert_eq!(response["size"], 8192, "response: {response}");
        assert_eq!(
            response["sha256"], "f8e920545e99cdc9bbc2650eb8282344e8971a7ff0c397c91355d0fcaf6c61fa",
            "response: {response}"
        );
        assert_eq!(fs::read(&interception_marker)?, b"denied", "output-file open was not denied");
        assert!(!encoder_marker.exists(), "converter ran despite output-file open failure");
        assert!(server.try_wait()?.is_none(), "output-open failure terminated rngit");
        Ok(())
    })();

    let _ = server.kill();
    let _ = server.wait();
    result
}

const PRELOAD_SHIM: &str = r#"
#define _GNU_SOURCE
#include <dlfcn.h>
#include <errno.h>
#include <fcntl.h>
#include <stdarg.h>
#include <stdbool.h>
#include <stdlib.h>
#include <string.h>
#include <sys/syscall.h>
#include <unistd.h>

static bool deny_output_open(const char *path, int flags) {
    size_t length = strlen(path);
    if ((flags & (O_CREAT | O_TRUNC)) != (O_CREAT | O_TRUNC) || (flags & O_EXCL) != 0 ||
        strstr(path, "rngit-media-") == NULL || length < 5 || strcmp(path + length - 5, ".webp") != 0) {
        return false;
    }
    const char *marker = getenv("RNGIT_OPEN_FAILURE_MARKER");
    if (marker != NULL) {
        long fd = syscall(SYS_openat, AT_FDCWD, marker, O_WRONLY | O_CREAT | O_TRUNC, 0600);
        if (fd >= 0) {
            (void)syscall(SYS_write, fd, "denied", 6);
            (void)syscall(SYS_close, fd);
        }
    }
    errno = EACCES;
    return true;
}

int openat(int dirfd, const char *path, int flags, ...) {
    mode_t mode = 0;
    if ((flags & O_CREAT) != 0) {
        va_list args;
        va_start(args, flags);
        mode = va_arg(args, mode_t);
        va_end(args);
    }
    if (deny_output_open(path, flags)) return -1;
    static int (*next_openat)(int, const char *, int, ...) = NULL;
    if (next_openat == NULL) next_openat = dlsym(RTLD_NEXT, "openat");
    return (flags & O_CREAT) != 0 ? next_openat(dirfd, path, flags, mode) : next_openat(dirfd, path, flags);
}

int openat64(int dirfd, const char *path, int flags, ...) {
    mode_t mode = 0;
    if ((flags & O_CREAT) != 0) {
        va_list args;
        va_start(args, flags);
        mode = va_arg(args, mode_t);
        va_end(args);
    }
    if (deny_output_open(path, flags)) return -1;
    static int (*next_openat64)(int, const char *, int, ...) = NULL;
    if (next_openat64 == NULL) next_openat64 = dlsym(RTLD_NEXT, "openat64");
    return (flags & O_CREAT) != 0 ? next_openat64(dirfd, path, flags, mode) : next_openat64(dirfd, path, flags);
}

int open(const char *path, int flags, ...) {
    mode_t mode = 0;
    if ((flags & O_CREAT) != 0) {
        va_list args;
        va_start(args, flags);
        mode = va_arg(args, mode_t);
        va_end(args);
    }
    if (deny_output_open(path, flags)) return -1;
    static int (*next_open)(const char *, int, ...) = NULL;
    if (next_open == NULL) next_open = dlsym(RTLD_NEXT, "open");
    return (flags & O_CREAT) != 0 ? next_open(path, flags, mode) : next_open(path, flags);
}

int open64(const char *path, int flags, ...) {
    mode_t mode = 0;
    if ((flags & O_CREAT) != 0) {
        va_list args;
        va_start(args, flags);
        mode = va_arg(args, mode_t);
        va_end(args);
    }
    if (deny_output_open(path, flags)) return -1;
    static int (*next_open64)(const char *, int, ...) = NULL;
    if (next_open64 == NULL) next_open64 = dlsym(RTLD_NEXT, "open64");
    return (flags & O_CREAT) != 0 ? next_open64(path, flags, mode) : next_open64(path, flags);
}
"#;

const PYTHON_CLIENT: &str = r#"
import hashlib
import json
import sys
import threading
import RNS

config_dir, destination_hex = sys.argv[1:3]
RNS.Reticulum(configdir=config_dir, loglevel=0)
destination_hash = bytes.fromhex(destination_hex)
if not RNS.Transport.await_path(destination_hash, timeout=30):
    raise RuntimeError("could not resolve rngit destination")
identity = RNS.Identity.recall(destination_hash)
if identity is None:
    raise RuntimeError("could not recall rngit identity")
destination = RNS.Destination(
    identity, RNS.Destination.OUT, RNS.Destination.SINGLE, "nomadnetwork", "node"
)
ready = threading.Event()
link = RNS.Link(destination)
link.set_link_established_callback(lambda _: ready.set())
link.set_link_closed_callback(lambda _: ready.set())
if not ready.wait(30) or link.status != RNS.Link.ACTIVE:
    raise RuntimeError("media output-open-failure Link did not establish")

finished = threading.Event()
result = {}
def response(receipt):
    payload = receipt.response.read()
    metadata = receipt.metadata or {}
    name = metadata.get("name", b"")
    result.update({
        "name": name.decode("utf-8") if isinstance(name, bytes) else str(name),
        "size": len(payload),
        "sha256": hashlib.sha256(payload).hexdigest(),
    })
    finished.set()
def failed(receipt):
    result["failed"] = str(receipt.status)
    finished.set()
link.request(
    "/media", {"key": b"present", "path": "/media/group/repo/HEAD/image.png"},
    response_callback=response, failed_callback=failed, timeout=30
)
if not finished.wait(35):
    raise RuntimeError("media output-open-failure response timed out")
link.teardown()
print(json.dumps(result, sort_keys=True))
"#;
