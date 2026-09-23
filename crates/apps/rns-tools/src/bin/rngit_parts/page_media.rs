const MEDIA_BLOB_LIMIT: usize = 32 * 1024 * 1024;

fn hex_nibble(value: u8) -> Option<u8> {
    match value {
        b'0'..=b'9' => Some(value - b'0'),
        b'a'..=b'f' => Some(value - b'a' + 10),
        b'A'..=b'F' => Some(value - b'A' + 10),
        _ => None,
    }
}

fn percent_decode_plus(value: &str) -> Option<String> {
    let bytes = value.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        match bytes[index] {
            b'+' => {
                decoded.push(b' ');
                index += 1;
            }
            b'%' if index + 2 < bytes.len() => {
                let high = hex_nibble(bytes[index + 1])?;
                let low = hex_nibble(bytes[index + 2])?;
                decoded.push((high << 4) | low);
                index += 3;
            }
            b'%' => return None,
            value => {
                decoded.push(value);
                index += 1;
            }
        }
    }
    match String::from_utf8(decoded) {
        Ok(value) => Some(value),
        Err(error) => {
            eprintln!("rngit: percent-decoded request value is not UTF-8: {error}");
            None
        }
    }
}

// Match urllib.parse.unquote_plus for /media only: malformed escapes remain
// literal and invalid UTF-8 is replaced, without relaxing path validation.
fn media_unquote_plus(value: &str) -> String {
    let bytes = value.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        match bytes[index] {
            b'+' => {
                decoded.push(b' ');
                index += 1;
            }
            b'%' if index + 2 < bytes.len() => {
                if let (Some(high), Some(low)) = (hex_nibble(bytes[index + 1]), hex_nibble(bytes[index + 2])) {
                    decoded.push((high << 4) | low);
                    index += 3;
                } else {
                    decoded.push(bytes[index]);
                    index += 1;
                }
            }
            byte => {
                decoded.push(byte);
                index += 1;
            }
        }
    }
    String::from_utf8_lossy(&decoded).into_owned()
}

fn percent_encode_plus(value: &str) -> String {
    let mut encoded = String::with_capacity(value.len());
    for byte in value.as_bytes() {
        match *byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                encoded.push(*byte as char)
            }
            b' ' => encoded.push('+'),
            byte => {
                encoded.push('%');
                encoded.push(char::from(b"0123456789ABCDEF"[(byte >> 4) as usize]));
                encoded.push(char::from(b"0123456789ABCDEF"[(byte & 0x0f) as usize]));
            }
        }
    }
    encoded
}

fn resource_metadata(name: &str) -> Option<Vec<u8>> {
    pack_value(&rmpv::Value::Map(vec![(
        rmpv::Value::String("name".into()),
        rmpv::Value::Binary(name.as_bytes().to_vec()),
    )]))
    .ok()
}

fn filename(value: &str) -> Option<String> {
    let name = Path::new(value).file_name()?.to_str()?;
    (!name.is_empty() && name != "." && name != ".." && !name.contains('/')).then(|| name.to_string())
}

impl ReticulumGitNode {
    fn media_request_path(path: &str) -> Option<(String, String, String, String)> {
        let remainder = path.strip_prefix(PAGE_MEDIA)?.trim_start_matches('/');
        let mut components = remainder.splitn(4, '/');
        // The pinned Python handler treats these path components literally;
        // only the file-path tail is URL-decoded with unquote_plus.
        let group = components.next()?.to_string();
        let repository = components.next()?.to_string();
        let reference = components.next()?.to_string();
        let file_path = media_unquote_plus(components.next()?);
        if group.is_empty() || repository.is_empty() || reference.is_empty() {
            return None;
        }
        Some((group, repository, reference, file_path))
    }

    fn file_response(data: Vec<u8>, name: &str) -> Option<PageResponse> {
        Some(page_response(data, resource_metadata(name)))
    }

    fn serve_media(
        &mut self,
        map: &[(rmpv::Value, rmpv::Value)],
        remote: [u8; 16],
        link_id: [u8; 16],
    ) -> Option<PageResponse> {
        if map_value(map, &rmpv::Value::String("key".into())).is_none() {
            return Some(page_denial_response());
        }
        let Some(request_path) = map_string_value(map, "path") else {
            return Some(page_denial_response());
        };
        let Some((group, repository, reference, file_path)) = Self::media_request_path(&request_path) else {
            return Some(page_denial_response());
        };
        let Some(record) = self.accessible_repository(&remote, &group, &repository) else {
            return Some(page_denial_response());
        };
        let repository_path = record.path.clone();
        let Some(resolved) = Self::resolve_page_ref(&repository_path, &reference) else {
            return Some(page_denial_response());
        };
        if file_path.is_empty() {
            return Some(page_denial_response());
        }
        if !Self::valid_page_path(&file_path) {
            return Some(page_denial_response());
        }
        let blob_spec = format!("{resolved}:{file_path}");
        if Self::page_git_output(
            &repository_path,
            &["cat-file".into(), "-s".into(), blob_spec.clone()],
            64,
        )
        .is_none()
        {
            return Some(page_denial_response());
        }
        // A missing object is the pinned handler's False response above. Once
        // it is known to exist, a failed content read remains no-response.
        let blob = Self::page_blob(&repository_path, &resolved, &file_path, MEDIA_BLOB_LIMIT)?;
        let Some(original_name) = filename(&file_path) else {
            return Some(page_denial_response());
        };
        let extension = Path::new(&file_path)
            .extension()
            .and_then(|value| value.to_str())
            .map(|value| format!(".{}", value.to_ascii_lowercase()));

        if self.media_conversion
            && extension
                .as_deref()
                .is_some_and(|value| matches!(value, ".png" | ".jpg" | ".jpeg" | ".gif" | ".tiff" | ".tif" | ".bmp"))
            && self.active_page_links.contains_key(&link_id)
        {
            if let Ok(directory) = self.next_media_directory(link_id) {
                let stem = Path::new(&original_name)
                    .file_stem()
                    .and_then(|value| value.to_str())
                    .unwrap_or("image");
                let output_path = directory.join(format!("{stem}.webp"));
                let input = vec![
                    "git".to_string(),
                    "cat-file".to_string(),
                    "blob".to_string(),
                    format!("{resolved}:{file_path}"),
                ];
                let converted = convert_to_webp(
                    &input,
                    &output_path,
                    Some(&repository_path),
                    Some(Duration::from_secs(8)),
                    Some(self.media_quality),
                    self.media_max_dimension,
                );
                if converted {
                    if let Ok(converted_data) = fs::read(&output_path) {
                        let response_name = format!("{stem}.webp");
                        self.download_succeeded(&group, &repository, false);
                        return Self::file_response(converted_data, &response_name);
                    }
                }
                if let Err(error) = Self::remove_tracked_page_media_directory(
                    &mut self.active_page_links,
                    link_id,
                    &directory,
                    |path| fs::remove_dir_all(path),
                ) {
                    log_page_media_cleanup_failure(
                        "WebP conversion fallback",
                        link_id,
                        &directory,
                        &error,
                    );
                }
            }
        }

        self.download_succeeded(&group, &repository, false);
        Self::file_response(blob, &original_name)
    }

    fn decoded_request_field(map: &[(rmpv::Value, rmpv::Value)], key: &str) -> Option<String> {
        percent_decode_plus(&map_string_value(map, key)?)
    }

    fn serve_download(
        &mut self,
        map: &[(rmpv::Value, rmpv::Value)],
        remote: [u8; 16],
    ) -> Option<PageResponse> {
        let group = Self::decoded_request_field(map, "var_g")?;
        let repository = Self::decoded_request_field(map, "var_r")?;
        let reference = Self::decoded_request_field(map, "var_ref").unwrap_or_else(|| "HEAD".to_string());
        let file_path = Self::decoded_request_field(map, "var_path")?;
        let record = self.accessible_repository(&remote, &group, &repository)?;
        let resolved = Self::resolve_page_ref(&record.path, &reference)?;
        let file_name = filename(&file_path)?;
        let blob = Self::page_blob(&record.path, &resolved, &file_path, MEDIA_BLOB_LIMIT)?;
        self.download_succeeded(&group, &repository, false);
        Self::file_response(blob, &file_name)
    }

    fn serve_artifact(
        &mut self,
        map: &[(rmpv::Value, rmpv::Value)],
        remote: [u8; 16],
    ) -> Option<PageResponse> {
        let group = Self::decoded_request_field(map, "var_g")?;
        let repository = Self::decoded_request_field(map, "var_r")?;
        let mut tag = Self::decoded_request_field(map, "var_t")?;
        let artifact = Self::decoded_request_field(map, "var_a")?;
        if artifact.is_empty() || artifact.contains('/') || artifact.contains('\\') {
            return None;
        }
        let record = self.accessible_repository(&remote, &group, &repository)?;
        if tag == "latest" {
            tag = fs::read_to_string(companion_path(&record.path, "releases").join("latest"))
                .ok()?
                .trim()
                .to_string();
        }
        let release_dir = Self::release_path(record, &tag)?;
        let metadata = fs::read_to_string(release_dir.join("META")).ok()?;
        if !metadata.lines().any(|line| line.trim() == "status=published") {
            return None;
        }
        let artifact_path = release_dir.join("artifacts").join(&artifact);
        if !artifact_path.is_file() {
            return None;
        }
        let data = fs::read(artifact_path).ok()?;
        self.release_download_succeeded(&group, &repository, false);
        Self::file_response(data, &artifact)
    }

    fn serve_workdoc(
        &mut self,
        map: &[(rmpv::Value, rmpv::Value)],
        remote: [u8; 16],
    ) -> Option<PageResponse> {
        let group = Self::decoded_request_field(map, "var_g")?;
        let repository = Self::decoded_request_field(map, "var_r")?;
        let record = self.accessible_repository(&remote, &group, &repository)?;
        let id = map_value(map, &rmpv::Value::String("var_id".into()))
            .and_then(|value| value.as_u64().or_else(|| value.as_str()?.parse().ok()))?;
        let requested_scope = Self::decoded_request_field(map, "var_scope").unwrap_or_else(|| "all".to_string());
        let root = companion_path(&record.path, "work");
        let mut request = map.to_vec();
        request.push((
            rmpv::Value::String("doc_id".into()),
            rmpv::Value::from(id),
        ));
        request.push((
            rmpv::Value::String("scope".into()),
            rmpv::Value::String(requested_scope.into()),
        ));
        let (scope, _id, _directory, document) = self.work_request_document(&root, &request)?;
        if !self.resolve_doc_permission(&remote, &group, &repository, id, Self::PERM_READ) {
            return None;
        }
        let format = Self::work_meta_string(&document, "format");
        let title = Self::work_meta_string(&document, "title");
        let content = document
            .as_map()
            .and_then(|value| map_value(value, &rmpv::Value::String("content".into())))
            .and_then(rmpv::Value::as_str)?
            .trim();
        if content.is_empty() {
            return None;
        }
        let mut safe_title = title
            .chars()
            .map(|character| if matches!(character, '/' | '\\' | '\0') { '_' } else { character })
            .collect::<String>();
        if safe_title.is_empty() {
            safe_title = format!("work-{id}");
        }
        let extension = if format == "micron" { "mu" } else { "md" };
        let name = format!("{safe_title}.{extension}");
        let _ = scope;
        self.download_succeeded(&group, &repository, false);
        Self::file_response(content.as_bytes().to_vec(), &name)
    }
}
