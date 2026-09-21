impl ReticulumGitClient {
    pub fn fetch_release_artifact(
        &mut self,
        remote: &str,
        tag: &str,
        artifact: &str,
    ) -> Result<Vec<u8>, String> {
        let parsed = self.ensure_repository_remote(remote)?;
        self.request_repository(
            RNGIT_PATH_RELEASE,
            &format!("{}/{}", parsed.group, parsed.repository),
            [
                (rmpv::Value::String("operation".into()), rmpv::Value::String("fetch".into())),
                (rmpv::Value::String("tag".into()), rmpv::Value::String(tag.into())),
                (rmpv::Value::String("artifact".into()), rmpv::Value::String(artifact.into())),
            ],
        )
    }

    pub fn create_release_init(
        &mut self,
        remote: &str,
        tag: &str,
        commit_hash: Option<&str>,
        notes: &str,
        notes_format: &str,
    ) -> Result<Vec<u8>, String> {
        let parsed = self.ensure_repository_remote(remote)?;
        let mut extra = vec![
            (rmpv::Value::String("operation".into()), rmpv::Value::String("create".into())),
            (rmpv::Value::String("step".into()), rmpv::Value::String("init".into())),
            (rmpv::Value::String("tag".into()), rmpv::Value::String(tag.into())),
            (rmpv::Value::String("notes".into()), rmpv::Value::String(notes.into())),
            (
                rmpv::Value::String("notes_format".into()),
                rmpv::Value::String(notes_format.into()),
            ),
        ];
        if let Some(commit_hash) = commit_hash {
            extra.push((
                rmpv::Value::String("hash".into()),
                rmpv::Value::String(commit_hash.into()),
            ));
        }
        self.request_repository(
            RNGIT_PATH_RELEASE,
            &format!("{}/{}", parsed.group, parsed.repository),
            extra,
        )
    }

    pub fn upload_release_artifact(
        &mut self,
        remote: &str,
        tag: &str,
        artifact_name: &str,
        artifact_data: &[u8],
    ) -> Result<Vec<u8>, String> {
        let parsed = self.ensure_repository_remote(remote)?;
        self.request_repository(
            RNGIT_PATH_RELEASE,
            &format!("{}/{}", parsed.group, parsed.repository),
            [
                (rmpv::Value::String("operation".into()), rmpv::Value::String("create".into())),
                (rmpv::Value::String("step".into()), rmpv::Value::String("artifact".into())),
                (rmpv::Value::String("tag".into()), rmpv::Value::String(tag.into())),
                (
                    rmpv::Value::String("artifact_name".into()),
                    rmpv::Value::String(artifact_name.into()),
                ),
                (
                    rmpv::Value::String("artifact_data".into()),
                    rmpv::Value::Binary(artifact_data.to_vec()),
                ),
            ],
        )
    }

    pub fn finalize_release(&mut self, remote: &str, tag: &str) -> Result<Vec<u8>, String> {
        let parsed = self.ensure_repository_remote(remote)?;
        self.request_repository(
            RNGIT_PATH_RELEASE,
            &format!("{}/{}", parsed.group, parsed.repository),
            [
                (rmpv::Value::String("operation".into()), rmpv::Value::String("create".into())),
                (rmpv::Value::String("step".into()), rmpv::Value::String("finalize".into())),
                (rmpv::Value::String("tag".into()), rmpv::Value::String(tag.into())),
            ],
        )
    }
}
