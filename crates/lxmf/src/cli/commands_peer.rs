use crate::cli::app::{PeerAction, PeerCommand, RuntimeContext};
use anyhow::{anyhow, Result};
use serde_json::{json, Value};
use std::cmp::Ordering;

pub fn run(ctx: &RuntimeContext, command: &PeerCommand) -> Result<()> {
    match &command.action {
        PeerAction::List { query, limit } => {
            let peers = list_peers(ctx, query.as_deref(), *limit)?;
            ctx.output.emit_status(&json!({"peers": peers}))
        }
        PeerAction::Show { selector, exact } => {
            let peers = list_peers(ctx, None, None)?;
            let matches = select_peers(&peers, selector, *exact);
            match matches.len() {
                0 => Err(anyhow!("no peer matched selector '{}'", selector)),
                1 => {
                    let entry = matches[0].clone();
                    if ctx.cli.json {
                        ctx.output.emit_status(&entry)
                    } else {
                        let summary = summarize_peer(&entry);
                        ctx.output.emit_status(&summary)
                    }
                }
                _ => {
                    let preview = matches
                        .iter()
                        .take(5)
                        .map(format_peer_candidate)
                        .collect::<Vec<_>>()
                        .join(", ");
                    Err(anyhow!(
                        "selector '{}' is ambiguous ({} matches): {}{}",
                        selector,
                        matches.len(),
                        preview,
                        if matches.len() > 5 { ", ..." } else { "" }
                    ))
                }
            }
        }
        PeerAction::Watch { interval_secs } => watch_peers(ctx, *interval_secs),
        PeerAction::Sync { peer } => {
            let result = ctx.rpc.call("peer_sync", Some(json!({ "peer": peer })))?;
            ctx.output.emit_status(&result)
        }
        PeerAction::Unpeer { peer } => {
            let result = ctx.rpc.call("peer_unpeer", Some(json!({ "peer": peer })))?;
            ctx.output.emit_status(&result)
        }
        PeerAction::Clear => {
            let result = ctx.rpc.call("clear_peers", None)?;
            ctx.output.emit_status(&result)
        }
    }
}

fn watch_peers(ctx: &RuntimeContext, interval_secs: u64) -> Result<()> {
    loop {
        let peers = list_peers(ctx, None, None)?;
        ctx.output.emit_status(&json!({ "peers": peers }))?;
        std::thread::sleep(std::time::Duration::from_secs(interval_secs.max(1)));
    }
}

fn list_peers(
    ctx: &RuntimeContext,
    query: Option<&str>,
    limit: Option<usize>,
) -> Result<Vec<Value>> {
    let response = ctx.rpc.call("list_peers", None)?;
    let peers = extract_peers(response);
    let mut peers = if let Some(query) = query.and_then(trimmed_nonempty) {
        query_peers(&peers, query)
    } else {
        peers
    };

    sort_peers(&mut peers);
    if let Some(limit) = limit {
        peers.truncate(limit);
    }

    Ok(peers)
}

pub(crate) fn extract_peers(value: Value) -> Vec<Value> {
    if let Some(items) = value.get("peers").and_then(Value::as_array) {
        return items.clone();
    }
    value.as_array().cloned().unwrap_or_default()
}

fn query_peers(peers: &[Value], query: &str) -> Vec<Value> {
    let mut ranked = peers
        .iter()
        .filter_map(|peer| rank_peer(peer, query).map(|score| (score, peer.clone())))
        .collect::<Vec<_>>();

    ranked.sort_by(|(score_a, peer_a), (score_b, peer_b)| {
        score_a.cmp(score_b).then_with(|| compare_peers(peer_a, peer_b))
    });
    ranked.into_iter().map(|(_, peer)| peer).collect()
}

fn select_peers(peers: &[Value], selector: &str, exact: bool) -> Vec<Value> {
    let selector = selector.trim();
    if selector.is_empty() {
        return Vec::new();
    }

    if exact {
        return peers.iter().filter(|peer| exact_match(peer, selector)).cloned().collect();
    }

    let mut ranked = peers
        .iter()
        .filter_map(|peer| rank_peer(peer, selector).map(|score| (score, peer.clone())))
        .collect::<Vec<_>>();
    ranked.sort_by(|(score_a, peer_a), (score_b, peer_b)| {
        score_a.cmp(score_b).then_with(|| compare_peers(peer_a, peer_b))
    });
    ranked.into_iter().map(|(_, peer)| peer).collect()
}

fn rank_peer(peer: &Value, selector: &str) -> Option<u8> {
    let query = selector.to_ascii_lowercase();
    let peer_hash = peer_hash(peer)?;
    let hash_lower = peer_hash.to_ascii_lowercase();
    let name_lower = peer_name(peer).map(str::to_ascii_lowercase);

    if hash_lower == query {
        return Some(0);
    }
    if name_lower.as_deref() == Some(query.as_str()) {
        return Some(1);
    }
    if hash_lower.starts_with(&query) {
        return Some(2);
    }
    if name_lower.as_deref().is_some_and(|name| name.starts_with(&query)) {
        return Some(3);
    }
    if name_lower.as_deref().is_some_and(|name| name.contains(&query)) {
        return Some(4);
    }
    None
}

fn exact_match(peer: &Value, selector: &str) -> bool {
    let query = selector.to_ascii_lowercase();
    peer_hash(peer).map(str::to_ascii_lowercase).is_some_and(|hash| hash == query)
        || peer_name(peer).map(str::to_ascii_lowercase).is_some_and(|name| name == query)
}

fn summarize_peer(peer: &Value) -> Value {
    let now = now_secs();
    let hash = peer_hash(peer).unwrap_or("<unknown>").to_string();
    let name = peer_name(peer).map(ToOwned::to_owned);
    let name_source = peer.get("name_source").and_then(Value::as_str).map(ToOwned::to_owned);
    let first_seen = peer.get("first_seen").and_then(Value::as_i64);
    let last_seen = peer.get("last_seen").and_then(Value::as_i64);
    let seen_count = peer.get("seen_count").and_then(Value::as_u64).unwrap_or(0);
    let age_seconds = last_seen.and_then(|ts| now.checked_sub(ts));

    json!({
        "hash": hash,
        "display_name": name,
        "name_source": name_source,
        "first_seen": first_seen,
        "last_seen": last_seen,
        "seen_count": seen_count,
        "age_seconds": age_seconds,
    })
}

fn format_peer_candidate(peer: &Value) -> String {
    let hash = peer_hash(peer).unwrap_or("<unknown>");
    match peer_name(peer) {
        Some(name) => format!("{name} ({hash})"),
        None => hash.to_string(),
    }
}

fn peer_hash(peer: &Value) -> Option<&str> {
    peer.get("peer").and_then(Value::as_str)
}

fn peer_name(peer: &Value) -> Option<&str> {
    for key in ["name", "display_name", "alias"] {
        if let Some(name) =
            peer.get(key).and_then(Value::as_str).map(str::trim).filter(|value| !value.is_empty())
        {
            return Some(name);
        }
    }
    None
}

fn peer_last_seen(peer: &Value) -> i64 {
    peer.get("last_seen").and_then(Value::as_i64).unwrap_or(0)
}

fn compare_peers(a: &Value, b: &Value) -> Ordering {
    peer_last_seen(b)
        .cmp(&peer_last_seen(a))
        .then_with(|| peer_hash(a).unwrap_or("").cmp(peer_hash(b).unwrap_or("")))
}

fn sort_peers(peers: &mut [Value]) {
    peers.sort_by(compare_peers);
}

fn now_secs() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_secs()
        as i64
}

fn trimmed_nonempty(value: &str) -> Option<&str> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed)
    }
}

#[cfg(test)]
mod tests {
    use super::{extract_peers, select_peers};
    use serde_json::json;

    #[test]
    fn extract_peers_supports_wrapped_response() {
        let peers = extract_peers(json!({
            "peers": [
                {"peer": "aaaa", "name": "Alice"},
                {"peer": "bbbb", "name": "Bob"}
            ]
        }));
        assert_eq!(peers.len(), 2);
    }

    #[test]
    fn select_peers_prefers_hash_prefix_before_name_prefix() {
        let peers = vec![
            json!({"peer": "abc111", "name": "zeta"}),
            json!({"peer": "ffff00", "name": "abc-match"}),
        ];
        let ranked = select_peers(&peers, "abc", false);
        assert_eq!(ranked[0]["peer"], "abc111");
    }

    #[test]
    fn select_peers_exact_matches_name() {
        let peers = vec![
            json!({"peer": "abc111", "name": "Alice"}),
            json!({"peer": "abc222", "name": "Alice B"}),
        ];
        let exact = select_peers(&peers, "alice", true);
        assert_eq!(exact.len(), 1);
        assert_eq!(exact[0]["peer"], "abc111");
    }

    #[test]
    fn select_peers_uses_display_name_fallback() {
        let peers = vec![
            json!({"peer": "abc111", "display_name": "Relay One"}),
            json!({"peer": "abc222", "name": "Relay Two"}),
        ];
        let exact = select_peers(&peers, "relay one", true);
        assert_eq!(exact.len(), 1);
        assert_eq!(exact[0]["peer"], "abc111");
    }
}
