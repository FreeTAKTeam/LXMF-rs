use rns_transport::hash::{address_hash, AddressHash};
use rns_transport::resource::ResourceComplete;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

const FETCH_NOT_ALLOWED: i64 = 0xF0;

pub(crate) fn parse_address(value: &str) -> io::Result<AddressHash> {
    AddressHash::new_from_hex_string(value).map_err(|_| {
        io::Error::new(io::ErrorKind::InvalidInput, "destination must be a 32-character hex hash")
    })
}

pub(crate) fn encode_metadata(filename: &Path) -> io::Result<Vec<u8>> {
    let name = filename
        .file_name()
        .and_then(|value| value.to_str())
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "file name is not UTF-8"))?;
    let value = rmpv::Value::Map(vec![(
        rmpv::Value::String("name".into()),
        rmpv::Value::Binary(name.as_bytes().to_vec()),
    )]);
    let mut encoded = Vec::new();
    rmpv::encode::write_value(&mut encoded, &value).map_err(io::Error::other)?;
    Ok(encoded)
}

pub(crate) fn decode_filename(metadata: &[u8]) -> io::Result<String> {
    let mut cursor = std::io::Cursor::new(metadata);
    let value = rmpv::decode::read_value(&mut cursor)
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "invalid Resource metadata"))?;
    if cursor.position() != metadata.len() as u64 {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "trailing Resource metadata"));
    }
    let map = value.as_map().ok_or_else(|| {
        io::Error::new(io::ErrorKind::InvalidData, "Resource metadata is not a map")
    })?;
    let name = map
        .iter()
        .find(|(key, _)| key.as_str() == Some("name"))
        .and_then(|(_, value)| value.as_slice())
        .ok_or_else(|| {
            io::Error::new(io::ErrorKind::InvalidData, "Resource metadata has no name")
        })?;
    let name = String::from_utf8(name.to_vec()).map_err(|_| {
        io::Error::new(io::ErrorKind::InvalidData, "Resource filename is not UTF-8")
    })?;
    let basename = Path::new(&name)
        .file_name()
        .and_then(|value| value.to_str())
        .filter(|value| *value == name)
        .ok_or_else(|| {
            io::Error::new(io::ErrorKind::InvalidData, "Resource filename is not a basename")
        })?;
    Ok(basename.to_owned())
}

pub(crate) fn response_frame(request_id: [u8; 16], response: rmpv::Value) -> io::Result<Vec<u8>> {
    let value = rmpv::Value::Array(vec![rmpv::Value::Binary(request_id.to_vec()), response]);
    let mut encoded = Vec::new();
    rmpv::encode::write_value(&mut encoded, &value).map_err(io::Error::other)?;
    Ok(encoded)
}

pub(crate) fn request_path_and_file(data: &[u8]) -> io::Result<Option<String>> {
    let mut cursor = std::io::Cursor::new(data);
    let value = rmpv::decode::read_value(&mut cursor)
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "invalid Link request"))?;
    let Some(values) = value.as_array() else { return Ok(None) };
    if values.len() != 3 {
        return Ok(None);
    }
    let Some(path_hash) = values[1].as_slice() else { return Ok(None) };
    if path_hash != address_hash(b"fetch_file") {
        return Ok(None);
    }
    let Some(file) = values[2].as_str() else { return Ok(None) };
    Ok(Some(file.to_owned()))
}

pub(crate) fn response_status(value: &rmpv::Value) -> io::Result<ResponseStatus> {
    match value {
        rmpv::Value::Boolean(true) => Ok(ResponseStatus::Found),
        rmpv::Value::Boolean(false) => Ok(ResponseStatus::NotFound),
        rmpv::Value::Nil => Ok(ResponseStatus::RemoteError),
        rmpv::Value::Integer(number) if number.as_i64() == Some(FETCH_NOT_ALLOWED) => {
            Ok(ResponseStatus::NotAllowed)
        }
        _ => Err(io::Error::new(io::ErrorKind::InvalidData, "unknown rncp response")),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ResponseStatus {
    Found,
    NotFound,
    NotAllowed,
    RemoteError,
}

pub(crate) fn resolve_fetch_path(requested: &str, jail: Option<&Path>) -> io::Result<PathBuf> {
    let requested = Path::new(requested);
    let candidate = if let Some(jail) = jail {
        let jail = fs::canonicalize(jail)?;
        if !jail.is_dir() {
            return Err(io::Error::new(io::ErrorKind::NotFound, "fetch jail is not a directory"));
        }
        jail.join(requested)
    } else {
        requested.to_path_buf()
    };
    let candidate = fs::canonicalize(candidate)?;
    if let Some(jail) = jail {
        let jail = fs::canonicalize(jail)?;
        if !candidate.starts_with(&jail) || candidate == jail {
            return Err(io::Error::new(io::ErrorKind::PermissionDenied, "fetch path escapes jail"));
        }
    }
    if !candidate.is_file() {
        return Err(io::Error::new(io::ErrorKind::NotFound, "requested file was not found"));
    }
    Ok(candidate)
}

pub(crate) fn save_received(
    complete: &ResourceComplete,
    save_dir: Option<&Path>,
    overwrite: bool,
) -> io::Result<PathBuf> {
    let metadata = complete.metadata.as_deref().ok_or_else(|| {
        io::Error::new(io::ErrorKind::InvalidData, "received Resource has no metadata")
    })?;
    let filename = decode_filename(metadata)?;
    let base = save_dir.unwrap_or_else(|| Path::new("."));
    if !base.is_dir() {
        return Err(io::Error::new(io::ErrorKind::NotFound, "save directory is not a directory"));
    }
    let candidate = base.join(filename);
    if overwrite {
        fs::write(&candidate, &complete.data)?;
        return Ok(candidate);
    }
    let mut index = 0u32;
    loop {
        let candidate = if index == 0 {
            candidate.clone()
        } else {
            let stem = candidate.file_name().and_then(|value| value.to_str()).ok_or_else(|| {
                io::Error::new(io::ErrorKind::InvalidData, "invalid save filename")
            })?;
            candidate.with_file_name(format!("{stem}.{index}"))
        };
        if !candidate.exists() {
            fs::write(&candidate, &complete.data)?;
            return Ok(candidate);
        }
        index = index.saturating_add(1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resource_metadata_roundtrips_a_binary_safe_basename() {
        let encoded = encode_metadata(Path::new("payload.bin")).expect("metadata");
        assert_eq!(decode_filename(&encoded).expect("filename"), "payload.bin");
    }

    #[test]
    fn fetch_jail_rejects_parent_traversal() {
        let temp = tempfile::tempdir().expect("tempdir");
        let jail = temp.path().join("jail");
        fs::create_dir(&jail).expect("jail");
        fs::write(temp.path().join("outside.bin"), b"outside").expect("outside");
        let error = resolve_fetch_path("../outside.bin", Some(&jail)).expect_err("escape");
        assert_eq!(error.kind(), io::ErrorKind::PermissionDenied);
    }

    #[test]
    fn response_status_preserves_reference_failure_categories() {
        assert_eq!(
            response_status(&rmpv::Value::Boolean(false)).expect("not found"),
            ResponseStatus::NotFound
        );
        assert_eq!(
            response_status(&rmpv::Value::Integer(0xF0.into())).expect("not allowed"),
            ResponseStatus::NotAllowed
        );
        assert_eq!(
            response_status(&rmpv::Value::Nil).expect("remote error"),
            ResponseStatus::RemoteError
        );
    }
}
