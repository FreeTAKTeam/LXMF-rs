impl ReticulumGitNode {
    fn release_path(record: &RepositoryRecord, tag: &str) -> Option<PathBuf> {
        if tag.is_empty() || tag.contains('/') || tag.contains('\\') || tag == "." || tag == ".." {
            None
        } else {
            Some(companion_path(&record.path, "releases").join(tag))
        }
    }

    pub fn releases_list_data(&self, repository_path: &Path) -> Vec<u8> {
        let releases_path = companion_path(repository_path, "releases");
        let mut releases = Vec::new();
        let mut latest = None;
        if let Ok(entries) = fs::read_dir(&releases_path) {
            for entry in entries.flatten() {
                let directory = entry.path();
                if !directory.is_dir() || !directory.join("META").is_file() {
                    continue;
                }
                let metadata = fs::read_to_string(directory.join("META")).unwrap_or_default();
                let mut values = BTreeMap::new();
                for line in metadata.lines() {
                    if let Some((key, value)) = line.split_once('=') {
                        values.insert(key.trim().to_string(), value.trim().to_string());
                    }
                }
                let tag = values
                    .get("tag")
                    .cloned()
                    .or_else(|| directory.file_name().and_then(|name| name.to_str()).map(ToOwned::to_owned))
                    .unwrap_or_default();
                let status = values.get("status").cloned().unwrap_or_else(|| "unknown".to_string());
                let created = values.get("created").and_then(|value| value.parse::<u64>().ok()).unwrap_or(0);
                let artifacts = fs::read_dir(directory.join("artifacts"))
                    .map(|entries| entries.flatten().filter(|entry| entry.path().is_file()).count() as u64)
                    .unwrap_or(0);
                releases.push(rmpv::Value::Map(vec![
                    (rmpv::Value::String("tag".into()), rmpv::Value::String(tag.clone().into())),
                    (rmpv::Value::String("status".into()), rmpv::Value::String(status.clone().into())),
                    (rmpv::Value::String("created".into()), rmpv::Value::from(created)),
                    (rmpv::Value::String("artifacts".into()), rmpv::Value::from(artifacts)),
                ]));
                if status == "published" && fs::read_to_string(releases_path.join("latest")).ok().as_deref() == Some(tag.as_str()) {
                    latest = Some(tag);
                }
            }
        }
        let payload = rmpv::Value::Map(vec![
            (rmpv::Value::String("releases".into()), rmpv::Value::Array(releases)),
            (
                rmpv::Value::String("latest".into()),
                latest.map_or(rmpv::Value::Nil, |value| rmpv::Value::String(value.into())),
            ),
        ]);
        response(Self::RES_OK, "", Some(&payload))
    }

    pub fn release_data(&self, release_dir: &Path, tag: &str) -> Option<rmpv::Value> {
        let metadata = fs::read_to_string(release_dir.join("META")).ok()?;
        let mut values = BTreeMap::new();
        for line in metadata.lines() {
            if let Some((key, value)) = line.split_once('=') {
                values.insert(key.trim().to_string(), value.trim().to_string());
            }
        }
        let release_tag = values.get("tag").cloned().unwrap_or_else(|| tag.to_string());
        let created = values.get("created").and_then(|value| value.parse::<u64>().ok()).unwrap_or(0);
        let status = values.get("status").cloned().unwrap_or_else(|| "unknown".to_string());
        let notes = ["RELEASE.md", "RELEASE.mu", "RELEASE.txt"]
            .into_iter()
            .find_map(|name| fs::read_to_string(release_dir.join(name)).ok())
            .unwrap_or_default();
        let artifacts = fs::read_dir(release_dir.join("artifacts"))
            .map(|entries| {
                entries
                    .flatten()
                    .filter_map(|entry| {
                        let path = entry.path();
                        path.is_file().then(|| {
                            rmpv::Value::Map(vec![
                                (rmpv::Value::String("name".into()), rmpv::Value::String(entry.file_name().to_string_lossy().into())),
                                (rmpv::Value::String("size".into()), rmpv::Value::from(fs::metadata(path).map(|value| value.len()).unwrap_or(0))),
                            ])
                        })
                    })
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        Some(rmpv::Value::Map(vec![
            (rmpv::Value::String("tag".into()), rmpv::Value::String(release_tag.into())),
            (rmpv::Value::String("created".into()), rmpv::Value::from(created)),
            (rmpv::Value::String("status".into()), rmpv::Value::String(status.into())),
            (rmpv::Value::String("notes".into()), rmpv::Value::String(notes.into())),
            (rmpv::Value::String("artifacts".into()), rmpv::Value::Array(artifacts)),
        ]))
    }

    pub fn handle_release_request(
        &mut self,
        request: &[(rmpv::Value, rmpv::Value)],
        remote: [u8; 16],
    ) -> Vec<u8> {
        let (group, repository, record) = match self.repository_for_request(request) {
            Ok(value) => value,
            Err(error) => return error,
        };
        if !self.resolve_permission(&remote, &group, &repository, Self::PERM_RELEASE) {
            return response(Self::RES_DISALLOWED, "Not allowed", None);
        }
        let operation = map_string(request, &rmpv::Value::String("operation".into())).unwrap_or_default();
        let tag = map_string(request, &rmpv::Value::String("target".into())).unwrap_or_default();
        match operation.as_str() {
            "list" => self.releases_list_data(&record.path),
            "view" => {
                let Some(path) = Self::release_path(record, &tag) else {
                    return response(Self::RES_INVALID_REQ, "Invalid release tag", None);
                };
                self.release_data(&path, &tag).map_or_else(
                    || response(Self::RES_NOT_FOUND, "Release not found", None),
                    |value| response(Self::RES_OK, "", Some(&value)),
                )
            }
            "latest" => {
                let latest = fs::read_to_string(companion_path(&record.path, "releases").join("latest")).ok();
                latest.map_or_else(
                    || response(Self::RES_NOT_FOUND, "No published release", None),
                    |value| response(Self::RES_OK, "", Some(&rmpv::Value::String(value.trim().into()))),
                )
            }
            "create" => {
                let Some(path) = Self::release_path(record, &tag) else {
                    return response(Self::RES_INVALID_REQ, "Invalid release tag", None);
                };
                if let Err(error) = fs::create_dir_all(&path) {
                    return response(Self::RES_REMOTE_FAIL, error.to_string(), None);
                }
                let metadata = format!("tag={tag}\nstatus=published\ncreated={}\ncreated_by={}\n", unix_now(), hex::encode(remote));
                if let Err(error) = fs::write(path.join("META"), metadata) {
                    return response(Self::RES_REMOTE_FAIL, error.to_string(), None);
                }
                let releases = companion_path(&record.path, "releases");
                if let Err(error) = fs::write(releases.join("latest"), tag) {
                    return response(Self::RES_REMOTE_FAIL, error.to_string(), None);
                }
                vec![Self::RES_OK]
            }
            "delete" => {
                let Some(path) = Self::release_path(record, &tag) else {
                    return response(Self::RES_INVALID_REQ, "Invalid release tag", None);
                };
                match fs::remove_dir_all(path) {
                    Ok(()) => vec![Self::RES_OK],
                    Err(error) if error.kind() == io::ErrorKind::NotFound => response(Self::RES_NOT_FOUND, "Release not found", None),
                    Err(error) => response(Self::RES_REMOTE_FAIL, error.to_string(), None),
                }
            }
            "fetch" => {
                let Some(path) = Self::release_path(record, &tag) else {
                    return response(Self::RES_INVALID_REQ, "Invalid release tag", None);
                };
                self.release_data(&path, &tag).map_or_else(
                    || response(Self::RES_NOT_FOUND, "Release not found", None),
                    |value| response(Self::RES_OK, "", Some(&value)),
                )
            }
            _ => response(Self::RES_INVALID_REQ, "Invalid request", None),
        }
    }
}
