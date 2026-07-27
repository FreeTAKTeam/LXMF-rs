//! Private filesystem persistence helpers.

use rand_core::{OsRng, RngCore};
use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};

#[cfg(unix)]
const PRIVATE_DIRECTORY_MODE: u32 = 0o700;
#[cfg(unix)]
const PRIVATE_FILE_MODE: u32 = 0o600;
const TEMP_CREATE_ATTEMPTS: usize = 128;

/// Creates a directory and restricts it to its owner on Unix.
pub fn ensure_private_directory(path: &Path) -> io::Result<()> {
    fs::create_dir_all(path)?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(PRIVATE_DIRECTORY_MODE))?;
    }

    Ok(())
}

/// Atomically replaces a private file using a randomized, exclusively-created sibling.
pub fn atomic_write_private(path: &Path, contents: &[u8]) -> io::Result<()> {
    if let Some(parent) = path.parent().filter(|parent| !parent.as_os_str().is_empty()) {
        ensure_private_directory(parent)?;
    }

    let (tmp_path, mut tmp_file) = create_private_temp(path)?;
    let mut cleanup = TempCleanup::new(tmp_path);
    tmp_file.write_all(contents)?;
    tmp_file.sync_all()?;
    drop(tmp_file);

    replace_private_temp(cleanup.path(), path)?;
    cleanup.disarm();

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(PRIVATE_FILE_MODE))?;
        if let Some(parent) = path.parent().filter(|parent| !parent.as_os_str().is_empty()) {
            File::open(parent)?.sync_all()?;
        }
    }

    Ok(())
}

#[cfg(not(windows))]
fn replace_private_temp(source: &Path, destination: &Path) -> io::Result<()> {
    fs::rename(source, destination)
}

#[cfg(windows)]
fn replace_private_temp(source: &Path, destination: &Path) -> io::Result<()> {
    atomicwrites::replace_atomic(source, destination)
}

fn create_private_temp(path: &Path) -> io::Result<(PathBuf, File)> {
    let file_name = path.file_name().ok_or_else(|| {
        io::Error::new(io::ErrorKind::InvalidInput, "private file path has no file name")
    })?;

    for _ in 0..TEMP_CREATE_ATTEMPTS {
        let mut random = [0u8; 16];
        OsRng
            .try_fill_bytes(&mut random)
            .map_err(|_| io::Error::other("OS randomness unavailable"))?;
        let tmp_name = format!(".{}.tmp-{}", file_name.to_string_lossy(), hex::encode(random));
        let tmp_path = path.with_file_name(tmp_name);
        match open_private_new(&tmp_path) {
            Ok(file) => return Ok((tmp_path, file)),
            Err(err) if err.kind() == io::ErrorKind::AlreadyExists => {}
            Err(err) => return Err(err),
        }
    }

    Err(io::Error::new(
        io::ErrorKind::AlreadyExists,
        "could not create a unique private temporary file",
    ))
}

fn open_private_new(path: &Path) -> io::Result<File> {
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);

    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(PRIVATE_FILE_MODE);
    }

    options.open(path)
}

struct TempCleanup {
    path: PathBuf,
    armed: bool,
}

impl TempCleanup {
    fn new(path: PathBuf) -> Self {
        Self { path, armed: true }
    }

    fn path(&self) -> &Path {
        &self.path
    }

    fn disarm(&mut self) {
        self.armed = false;
    }
}

impl Drop for TempCleanup {
    fn drop(&mut self) {
        if self.armed {
            let _ = fs::remove_file(&self.path);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::atomic_write_private;
    #[cfg(unix)]
    use super::ensure_private_directory;

    #[test]
    fn private_file_roundtrip_and_atomically_replace_existing() {
        let temp = tempfile::tempdir().expect("tempdir");
        let path = temp.path().join("private").join("secret");

        atomic_write_private(&path, b"first").expect("first write");
        atomic_write_private(&path, b"second").expect("replacement write");

        assert_eq!(std::fs::read(path).expect("read private file"), b"second");
    }

    #[cfg(windows)]
    #[test]
    fn failed_windows_replacement_preserves_existing_secret() {
        use std::fs::OpenOptions;
        use std::os::windows::fs::OpenOptionsExt;

        let temp = tempfile::tempdir().expect("tempdir");
        let path = temp.path().join("secret");
        atomic_write_private(&path, b"existing").expect("initial write");

        let locked_file = OpenOptions::new()
            .read(true)
            .share_mode(0)
            .open(&path)
            .expect("lock destination against replacement");
        let result = atomic_write_private(&path, b"replacement");
        drop(locked_file);

        assert!(result.is_err(), "replacement should fail while locked");
        assert_eq!(std::fs::read(path).expect("read preserved secret"), b"existing");
    }

    #[cfg(unix)]
    #[test]
    fn private_storage_enforces_unix_modes() {
        use std::os::unix::fs::PermissionsExt;

        let temp = tempfile::tempdir().expect("tempdir");
        let directory = temp.path().join("private");
        std::fs::create_dir(&directory).expect("create permissive directory");
        std::fs::set_permissions(&directory, std::fs::Permissions::from_mode(0o755))
            .expect("set permissive directory mode");
        ensure_private_directory(&directory).expect("secure directory");
        let path = directory.join("secret");
        atomic_write_private(&path, b"secret").expect("write private file");

        let directory_mode =
            std::fs::metadata(&directory).expect("directory metadata").permissions().mode() & 0o777;
        let file_mode =
            std::fs::metadata(&path).expect("file metadata").permissions().mode() & 0o777;
        assert_eq!(directory_mode, 0o700);
        assert_eq!(file_mode, 0o600);
    }

    #[cfg(unix)]
    #[test]
    fn private_write_ignores_preexisting_legacy_temp_symlink() {
        use std::os::unix::fs::symlink;

        let temp = tempfile::tempdir().expect("tempdir");
        let path = temp.path().join("secret.key");
        let legacy_tmp = path.with_extension("tmp");
        let victim = temp.path().join("victim");
        std::fs::write(&victim, b"unchanged").expect("write victim");
        symlink(&victim, &legacy_tmp).expect("create legacy temp symlink");

        atomic_write_private(&path, b"private").expect("write private file");

        assert_eq!(std::fs::read(&victim).expect("read victim"), b"unchanged");
        assert!(legacy_tmp.is_symlink());
        assert_eq!(std::fs::read(path).expect("read private file"), b"private");
    }
}
