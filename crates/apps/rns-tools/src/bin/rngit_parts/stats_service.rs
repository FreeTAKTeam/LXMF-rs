impl ReticulumGitNode {
    pub fn repository_stats(
        &self,
        remote: &[u8; 16],
        group: &str,
        repository: &str,
        lookback_days: u64,
    ) -> Option<rmpv::Value> {
        if !self.resolve_permission(remote, group, repository, Self::PERM_STATS) {
            return None;
        }
        let lookback_days = lookback_days.clamp(1, 366);
        let repository_stats = self.stats.groups.get(group)?.get(repository)?;
        let keys = ["view", "fetch", "push", "download", "release_download"];
        let mut result = Vec::new();
        for key in keys {
            let mut daily = Vec::new();
            let mut total = 0_u64;
            for day in (0..lookback_days).rev() {
                let day_key = (unix_now() / 86_400).saturating_sub(day).to_string();
                let value = repository_stats
                    .get(&format!("{key}:{day_key}"))
                    .copied()
                    .unwrap_or(0);
                total += value;
                daily.push(rmpv::Value::from(value));
            }
            result.push((
                rmpv::Value::String(key.into()),
                rmpv::Value::Map(vec![
                    (rmpv::Value::String("daily".into()), rmpv::Value::Array(daily)),
                    (rmpv::Value::String("total".into()), rmpv::Value::from(total)),
                ]),
            ));
        }
        Some(rmpv::Value::Map(result))
    }
}
