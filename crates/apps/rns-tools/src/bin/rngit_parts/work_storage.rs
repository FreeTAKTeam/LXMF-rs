impl ReticulumGitNode {
    const WORK_DOC_LIMIT: usize = 256 * 1024;

    fn work_root(record: &RepositoryRecord) -> PathBuf {
        companion_path(&record.path, "work")
    }

    pub fn work_get_next_id(&self, work_root: &Path) -> u64 {
        ["active", "completed", "proposed"]
            .into_iter()
            .flat_map(|scope| fs::read_dir(work_root.join(scope)).into_iter().flatten())
            .filter_map(Result::ok)
            .filter_map(|entry| {
                entry
                    .file_name()
                    .to_str()
                    .and_then(|value| value.parse::<u64>().ok())
            })
            .max()
            .unwrap_or(0)
            .saturating_add(1)
    }

    pub fn work_get_next_comment_id(&self, document_dir: &Path) -> u64 {
        fs::read_dir(document_dir)
            .into_iter()
            .flatten()
            .filter_map(Result::ok)
            .filter_map(|entry| {
                entry
                    .file_name()
                    .to_str()
                    .and_then(|value| value.parse::<u64>().ok())
            })
            .max()
            .unwrap_or(0)
            .saturating_add(1)
    }

    pub fn work_load_document(&self, path: &Path) -> Option<rmpv::Value> {
        let bytes = fs::read(path).ok()?;
        if bytes.len() > Self::WORK_DOC_LIMIT {
            return None;
        }
        rmpv::decode::read_value(&mut std::io::Cursor::new(bytes)).ok()
    }

    pub fn work_save_document(&self, path: &Path, document: &rmpv::Value) -> Result<(), String> {
        let bytes = pack_value(document)?;
        if bytes.len() > Self::WORK_DOC_LIMIT {
            return Err("work document exceeds size limit".to_string());
        }
        let Some(parent) = path.parent() else {
            return Err("work document path has no parent".to_string());
        };
        fs::create_dir_all(parent).map_err(|error| error.to_string())?;
        let temporary =
            tempfile::NamedTempFile::new_in(parent).map_err(|error| error.to_string())?;
        fs::write(temporary.path(), bytes).map_err(|error| error.to_string())?;
        temporary
            .as_file()
            .sync_all()
            .map_err(|error| error.to_string())?;
        fs::rename(temporary.path(), path).map_err(|error| error.to_string())
    }

    fn work_document_path(&self, root: &Path, scope: &str, id: u64) -> PathBuf {
        root.join(scope).join(id.to_string()).join("root")
    }

    fn work_meta(document: &rmpv::Value) -> Option<&[(rmpv::Value, rmpv::Value)]> {
        document
            .as_map()
            .and_then(|map| map_value(map, &rmpv::Value::String("meta".into())))
            .and_then(rmpv::Value::as_map)
            .map(Vec::as_slice)
    }

    fn work_meta_string(document: &rmpv::Value, key: &str) -> String {
        let Some(value) = Self::work_meta(document)
            .and_then(|map| map_value(map, &rmpv::Value::String(key.into())))
        else {
            return String::new();
        };
        value
            .as_str()
            .map(ToOwned::to_owned)
            .or_else(|| value.as_slice().map(hex::encode))
            .unwrap_or_default()
    }

    fn work_meta_value(document: &rmpv::Value, key: &str) -> Option<rmpv::Value> {
        Self::work_meta(document)
            .and_then(|map| map_value(map, &rmpv::Value::String(key.into())))
            .cloned()
    }

    fn work_author_matches(document: &rmpv::Value, remote: &[u8; 16]) -> bool {
        let Some(author) = Self::work_meta_value(document, "author") else {
            return false;
        };
        match author {
            rmpv::Value::Binary(value) => value.as_slice() == remote,
            rmpv::Value::String(value) => value
                .as_str()
                .is_some_and(|value| value.eq_ignore_ascii_case(&hex::encode(remote))),
            _ => false,
        }
    }

    fn work_request_document(
        &self,
        root: &Path,
        request: &[(rmpv::Value, rmpv::Value)],
    ) -> Option<(String, u64, PathBuf, rmpv::Value)> {
        let id = map_value(request, &rmpv::Value::String("doc_id".into()))
            .and_then(|value| value.as_u64().or_else(|| value.as_str()?.parse::<u64>().ok()))?;
        let requested_scope = map_string(request, &rmpv::Value::String("scope".into()));
        let scopes: Vec<&str> = match requested_scope.as_deref() {
            None | Some("all") => vec!["active", "completed", "proposed"],
            Some(scope @ ("active" | "completed" | "proposed")) => vec![scope],
            Some(_) => return None,
        };
        scopes.into_iter().find_map(|scope| {
            let directory = root.join(scope).join(id.to_string());
            let document = self.work_load_document(&directory.join("root"))?;
            Some((scope.to_string(), id, directory, document))
        })
    }

    fn work_comments(&self, document_dir: &Path) -> Vec<rmpv::Value> {
        let mut comments = fs::read_dir(document_dir)
            .into_iter()
            .flatten()
            .filter_map(Result::ok)
            .filter_map(|entry| {
                let id = entry.file_name().to_str()?.parse::<u64>().ok()?;
                let path = entry.path();
                let document = self.work_load_document(&path)?;
                let content = document
                    .as_map()
                    .and_then(|map| map_string(map, &rmpv::Value::String("content".into())))
                    .unwrap_or_default();
                let created = Self::work_meta_value(&document, "created")
                    .unwrap_or_else(|| rmpv::Value::from(0_u64));
                let edited = Self::work_meta_value(&document, "edited")
                    .unwrap_or_else(|| created.clone());
                Some(rmpv::Value::Map(vec![
                    (rmpv::Value::String("id".into()), rmpv::Value::from(id)),
                    (
                        rmpv::Value::String("content".into()),
                        rmpv::Value::String(content.into()),
                    ),
                    (rmpv::Value::String("created".into()), created),
                    (rmpv::Value::String("edited".into()), edited),
                    (
                        rmpv::Value::String("author".into()),
                        rmpv::Value::String(Self::work_meta_string(&document, "author").into()),
                    ),
                    (
                        rmpv::Value::String("format".into()),
                        rmpv::Value::String(Self::work_meta_string(&document, "format").into()),
                    ),
                ]))
            })
            .collect::<Vec<_>>();
        comments.sort_by_key(|comment| {
            comment
                .as_map()
                .and_then(|map| map_value(map, &rmpv::Value::String("id".into())))
                .and_then(rmpv::Value::as_u64)
                .unwrap_or(0)
        });
        comments
    }

    fn work_view_payload(
        &self,
        scope: &str,
        id: u64,
        document_dir: &Path,
        document: &rmpv::Value,
    ) -> rmpv::Value {
        let content = document
            .as_map()
            .and_then(|map| map_string(map, &rmpv::Value::String("content".into())))
            .unwrap_or_default();
        let created = Self::work_meta_value(document, "created")
            .unwrap_or_else(|| rmpv::Value::from(0_u64));
        let edited = Self::work_meta_value(document, "edited")
            .unwrap_or_else(|| created.clone());
        let meta = rmpv::Value::Map(vec![
            (
                rmpv::Value::String("title".into()),
                rmpv::Value::String(Self::work_meta_string(document, "title").into()),
            ),
            (rmpv::Value::String("created".into()), created),
            (rmpv::Value::String("edited".into()), edited),
            (
                rmpv::Value::String("author".into()),
                rmpv::Value::String(Self::work_meta_string(document, "author").into()),
            ),
            (
                rmpv::Value::String("identity".into()),
                Self::work_meta_value(document, "identity").unwrap_or(rmpv::Value::Nil),
            ),
            (
                rmpv::Value::String("signature".into()),
                Self::work_meta_value(document, "signature").unwrap_or(rmpv::Value::Nil),
            ),
            (
                rmpv::Value::String("format".into()),
                rmpv::Value::String(Self::work_meta_string(document, "format").into()),
            ),
        ]);
        rmpv::Value::Map(vec![
            (rmpv::Value::String("id".into()), rmpv::Value::from(id)),
            (
                rmpv::Value::String("scope".into()),
                rmpv::Value::String(scope.into()),
            ),
            (
                rmpv::Value::String("content".into()),
                rmpv::Value::String(content.into()),
            ),
            (
                rmpv::Value::String("comments".into()),
                rmpv::Value::Array(self.work_comments(document_dir)),
            ),
            (rmpv::Value::String("meta".into()), meta),
        ])
    }
}
