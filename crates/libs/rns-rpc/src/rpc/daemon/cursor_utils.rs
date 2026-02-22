#[derive(Debug)]
struct SdkCursorError {
    code: String,
    message: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct StreamGapMeta {
    gap_seq_no: u64,
    expected_seq_no: u64,
    observed_seq_no: u64,
    dropped_count: u64,
}

fn cursor_is_expired(cursor_seq: Option<u64>, oldest_seq: Option<u64>) -> bool {
    matches!(
        (cursor_seq, oldest_seq),
        (Some(cursor), Some(oldest)) if cursor.saturating_add(1) < oldest
    )
}

fn compute_stream_gap(dropped_count: u64, oldest_seq: Option<u64>) -> Option<StreamGapMeta> {
    if dropped_count == 0 {
        return None;
    }
    let observed_seq_no = oldest_seq?;
    let expected_seq_no = observed_seq_no.saturating_sub(dropped_count);
    let gap_seq_no = observed_seq_no.saturating_sub(1);
    Some(StreamGapMeta { gap_seq_no, expected_seq_no, observed_seq_no, dropped_count })
}

fn parse_announce_cursor(cursor: Option<&str>) -> Option<(Option<i64>, Option<String>)> {
    let raw = cursor?.trim();
    if raw.is_empty() {
        return None;
    }
    if let Some((timestamp_raw, id)) = raw.split_once(':') {
        let timestamp = timestamp_raw.parse::<i64>().ok()?;
        let before_id = if id.is_empty() { None } else { Some(id.to_string()) };
        return Some((Some(timestamp), before_id));
    }
    raw.parse::<i64>().ok().map(|timestamp| (Some(timestamp), None))
}

fn delivery_reason_code(status: &str) -> Option<&'static str> {
    let normalized = status.trim().to_ascii_lowercase();
    if normalized.is_empty() {
        return None;
    }
    if normalized.contains("receipt timeout") {
        return Some("receipt_timeout");
    }
    if normalized.contains("timeout") {
        return Some("timeout");
    }
    if normalized.contains("no route")
        || normalized.contains("no path")
        || normalized.contains("no known path")
    {
        return Some("no_path");
    }
    if normalized.contains("no propagation relay selected") {
        return Some("relay_unset");
    }
    if normalized.contains("retry budget exhausted") {
        return Some("retry_budget_exhausted");
    }
    None
}

fn merge_json_patch(target: &mut JsonValue, patch: &JsonValue) {
    let JsonValue::Object(patch_map) = patch else {
        *target = patch.clone();
        return;
    };

    if !target.is_object() {
        *target = JsonValue::Object(JsonMap::new());
    }
    let target_map = target.as_object_mut().expect("target must be object after initialization");
    for (key, value) in patch_map {
        if value.is_null() {
            target_map.remove(key);
            continue;
        }
        match target_map.get_mut(key) {
            Some(existing) if existing.is_object() && value.is_object() => {
                merge_json_patch(existing, value);
            }
            _ => {
                target_map.insert(key.clone(), value.clone());
            }
        }
    }
}

#[cfg(test)]
mod cursor_utils_tests {
    use super::{compute_stream_gap, cursor_is_expired};

    #[test]
    fn cursor_expiry_threshold_respects_retained_window_boundary() {
        for oldest in [1_u64, 2, 8, 32, 1024] {
            let not_expired_cursor = oldest.saturating_sub(1);
            assert!(
                !cursor_is_expired(Some(not_expired_cursor), Some(oldest)),
                "cursor at oldest-1 must remain valid (oldest={oldest})"
            );
            if oldest > 1 {
                let expired_cursor = oldest.saturating_sub(2);
                assert!(
                    cursor_is_expired(Some(expired_cursor), Some(oldest)),
                    "cursor older than retained window must expire (cursor={expired_cursor}, oldest={oldest})"
                );
            }
        }
    }

    #[test]
    fn stream_gap_meta_preserves_expected_observed_invariant() {
        for dropped in [1_u64, 2, 5, 64, 512] {
            for oldest in [dropped, dropped + 1, dropped + 32, dropped + 1000] {
                let gap = compute_stream_gap(dropped, Some(oldest)).expect("gap meta");
                assert_eq!(
                    gap.expected_seq_no.saturating_add(gap.dropped_count),
                    gap.observed_seq_no,
                    "expected + dropped must equal observed"
                );
                assert_eq!(
                    gap.gap_seq_no,
                    gap.observed_seq_no.saturating_sub(1),
                    "gap sequence should be one before observed sequence"
                );
            }
        }
        assert!(compute_stream_gap(0, Some(10)).is_none(), "no drops must not produce gap meta");
        assert!(
            compute_stream_gap(3, None).is_none(),
            "missing observed sequence must not produce gap meta"
        );
    }
}
