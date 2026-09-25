fn sort_path_rows(rows: &mut [Value]) {
    rows.sort_by(|left, right| {
        let left_interface = value_str(left, "interface")
            .or_else(|| value_str(left, "interface_hash"))
            .unwrap_or_default();
        let right_interface = value_str(right, "interface")
            .or_else(|| value_str(right, "interface_hash"))
            .unwrap_or_default();
        let left_hops = left.get("hops").and_then(Value::as_u64).unwrap_or_default();
        let right_hops = right.get("hops").and_then(Value::as_u64).unwrap_or_default();
        left_interface.cmp(right_interface).then_with(|| left_hops.cmp(&right_hops))
    });
}

fn filter_path_rows(rows: &mut Vec<Value>, destination_hash: Option<&str>, json_output: bool) -> bool {
    if json_output {
        return true;
    }
    let Some(destination_hash) = destination_hash else {
        return true;
    };
    rows.retain(|row| {
        value_str(row, "hash").is_some_and(|hash| hash.eq_ignore_ascii_case(destination_hash))
    });
    !rows.is_empty()
}

fn format_path_timestamp(timestamp_secs: f64) -> String {
    let Some(whole_seconds) = timestamp_secs
        .is_finite()
        .then_some(timestamp_secs.floor())
        .filter(|seconds| *seconds >= i64::MIN as f64 && *seconds < i64::MAX as f64)
    else {
        return timestamp_secs.to_string();
    };
    let Ok(utc_time) = time::OffsetDateTime::from_unix_timestamp(whole_seconds as i64) else {
        return timestamp_secs.to_string();
    };
    let offset = time::UtcOffset::local_offset_at(utc_time).unwrap_or(time::UtcOffset::UTC);
    format_path_timestamp_at_offset(timestamp_secs, offset)
        .unwrap_or_else(|| timestamp_secs.to_string())
}

fn format_path_timestamp_at_offset(
    timestamp_secs: f64,
    offset: time::UtcOffset,
) -> Option<String> {
    if !timestamp_secs.is_finite() {
        return None;
    }
    let whole_seconds = timestamp_secs.floor();
    if whole_seconds < i64::MIN as f64 || whole_seconds >= i64::MAX as f64 {
        return None;
    }
    let timestamp = time::OffsetDateTime::from_unix_timestamp(whole_seconds as i64)
        .ok()?
        .to_offset(offset);
    Some(format!(
        "{:04}-{:02}-{:02} {:02}:{:02}:{:02}",
        timestamp.year(),
        timestamp.month() as u8,
        timestamp.day(),
        timestamp.hour(),
        timestamp.minute(),
        timestamp.second()
    ))
}
