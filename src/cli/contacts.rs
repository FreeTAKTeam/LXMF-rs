use crate::cli::profile::profile_paths;
use anyhow::{anyhow, Context, Result};
use serde::{Deserialize, Serialize};
use std::cmp::Ordering;
use std::fs;
use std::path::Path;

const MAX_ALIAS_CHARS: usize = 64;
const MAX_NOTES_CHARS: usize = 280;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ContactEntry {
    pub alias: String,
    pub hash: String,
    #[serde(default)]
    pub notes: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct ContactsFile {
    #[serde(default)]
    contacts: Vec<ContactEntry>,
}

pub fn load_contacts(profile_name: &str) -> Result<Vec<ContactEntry>> {
    let path = profile_paths(profile_name)?.contacts_toml;
    if !path.exists() {
        return Ok(Vec::new());
    }

    let contents = fs::read_to_string(&path)
        .with_context(|| format!("failed to read contacts {}", path.display()))?;
    let parsed: ContactsFile = toml::from_str(&contents)
        .with_context(|| format!("invalid contacts format in {}", path.display()))?;

    let mut contacts = parsed
        .contacts
        .into_iter()
        .map(validate_contact)
        .collect::<Result<Vec<_>>>()?;
    sort_contacts(&mut contacts);
    Ok(contacts)
}

pub fn save_contacts(profile_name: &str, contacts: &[ContactEntry]) -> Result<()> {
    let paths = profile_paths(profile_name)?;
    fs::create_dir_all(&paths.root)
        .with_context(|| format!("failed to create {}", paths.root.display()))?;

    let mut normalized = contacts
        .iter()
        .cloned()
        .map(validate_contact)
        .collect::<Result<Vec<_>>>()?;
    dedupe_contacts(&mut normalized);
    sort_contacts(&mut normalized);

    let encoded = toml::to_string_pretty(&ContactsFile {
        contacts: normalized,
    })
    .context("failed to encode contacts.toml")?;
    fs::write(&paths.contacts_toml, encoded)
        .with_context(|| format!("failed to write {}", paths.contacts_toml.display()))
}

pub fn import_contacts_json(profile_name: &str, src: &Path, replace: bool) -> Result<usize> {
    let raw = fs::read_to_string(src)
        .with_context(|| format!("failed to read contact import {}", src.display()))?;

    let imported = parse_contacts_json(&raw)
        .with_context(|| format!("invalid contacts JSON in {}", src.display()))?;
    let mut imported = imported
        .into_iter()
        .map(validate_contact)
        .collect::<Result<Vec<_>>>()?;

    if replace {
        dedupe_contacts(&mut imported);
        save_contacts(profile_name, &imported)?;
        return Ok(imported.len());
    }

    let mut merged = load_contacts(profile_name)?;
    for contact in imported {
        upsert_contact(&mut merged, contact);
    }
    save_contacts(profile_name, &merged)?;
    Ok(merged.len())
}

pub fn export_contacts_json(profile_name: &str, dst: &Path) -> Result<usize> {
    let contacts = load_contacts(profile_name)?;
    let encoded = serde_json::to_string_pretty(&contacts).context("failed to encode contacts")?;
    fs::write(dst, encoded).with_context(|| format!("failed to write {}", dst.display()))?;
    Ok(contacts.len())
}

pub fn upsert_contact(contacts: &mut Vec<ContactEntry>, entry: ContactEntry) {
    let mut updated = false;
    if let Some(existing) = contacts
        .iter_mut()
        .find(|contact| contact.alias.eq_ignore_ascii_case(&entry.alias))
    {
        *existing = entry.clone();
        updated = true;
    } else if let Some(existing) = contacts
        .iter_mut()
        .find(|contact| contact.hash.eq_ignore_ascii_case(&entry.hash))
    {
        *existing = entry.clone();
        updated = true;
    }

    if !updated {
        contacts.push(entry);
    }
    dedupe_contacts(contacts);
    sort_contacts(contacts);
}

pub fn remove_contact_by_alias(contacts: &mut Vec<ContactEntry>, alias: &str) -> bool {
    let len_before = contacts.len();
    contacts.retain(|entry| !entry.alias.eq_ignore_ascii_case(alias));
    len_before != contacts.len()
}

pub fn resolve_contact_hash(contacts: &[ContactEntry], selector: &str) -> Option<String> {
    let selector = selector.trim();
    if selector.is_empty() {
        return None;
    }
    let selector = selector.strip_prefix('@').unwrap_or(selector);

    contacts
        .iter()
        .find(|entry| {
            entry.alias.eq_ignore_ascii_case(selector) || entry.hash.eq_ignore_ascii_case(selector)
        })
        .map(|entry| entry.hash.clone())
}

pub fn find_contact_by_hash<'a>(
    contacts: &'a [ContactEntry],
    hash: &str,
) -> Option<&'a ContactEntry> {
    contacts
        .iter()
        .find(|entry| entry.hash.eq_ignore_ascii_case(hash))
}

pub fn select_contacts<'a>(
    contacts: &'a [ContactEntry],
    selector: &str,
    exact: bool,
) -> Vec<&'a ContactEntry> {
    let selector = selector.trim();
    if selector.is_empty() {
        return Vec::new();
    }
    let selector_lower = selector.to_ascii_lowercase();

    if exact {
        return contacts
            .iter()
            .filter(|entry| {
                entry.alias.eq_ignore_ascii_case(selector)
                    || entry.hash.eq_ignore_ascii_case(selector)
            })
            .collect();
    }

    let mut ranked = contacts
        .iter()
        .filter_map(|entry| rank_contact(entry, &selector_lower).map(|score| (score, entry)))
        .collect::<Vec<_>>();
    ranked.sort_by(|(score_a, entry_a), (score_b, entry_b)| {
        score_a
            .cmp(score_b)
            .then_with(|| compare_contacts(entry_a, entry_b))
    });
    ranked.into_iter().map(|(_, entry)| entry).collect()
}

pub fn filter_contacts(
    contacts: &[ContactEntry],
    query: Option<&str>,
    limit: Option<usize>,
) -> Vec<ContactEntry> {
    let mut result = if let Some(query) = query.and_then(trimmed_nonempty) {
        select_contacts(contacts, query, false)
            .into_iter()
            .cloned()
            .collect::<Vec<_>>()
    } else {
        contacts.to_vec()
    };
    sort_contacts(&mut result);
    if let Some(limit) = limit {
        result.truncate(limit);
    }
    result
}

pub fn validate_contact(entry: ContactEntry) -> Result<ContactEntry> {
    let alias = normalize_alias(&entry.alias)?;
    let hash = normalize_hash(&entry.hash)?;
    let notes = normalize_notes(entry.notes.as_deref())?;
    Ok(ContactEntry { alias, hash, notes })
}

fn normalize_alias(value: &str) -> Result<String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Err(anyhow!("contact alias cannot be empty"));
    }
    if trimmed.chars().any(char::is_control) {
        return Err(anyhow!("contact alias cannot contain control characters"));
    }
    Ok(trimmed.chars().take(MAX_ALIAS_CHARS).collect())
}

fn normalize_hash(value: &str) -> Result<String> {
    let trimmed = value.trim().trim_start_matches("0x").to_ascii_lowercase();
    if trimmed.len() != 32 {
        return Err(anyhow!(
            "contact hash must be a 32-character hex destination hash"
        ));
    }
    if !trimmed.chars().all(|c| c.is_ascii_hexdigit()) {
        return Err(anyhow!("contact hash must be valid hex"));
    }
    Ok(trimmed)
}

fn normalize_notes(value: Option<&str>) -> Result<Option<String>> {
    let Some(value) = value else {
        return Ok(None);
    };

    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Ok(None);
    }
    if trimmed.chars().any(char::is_control) {
        return Err(anyhow!("contact notes cannot contain control characters"));
    }
    Ok(Some(trimmed.chars().take(MAX_NOTES_CHARS).collect()))
}

fn parse_contacts_json(raw: &str) -> Result<Vec<ContactEntry>> {
    if raw.trim_start().starts_with('{') {
        #[derive(Deserialize)]
        struct Wrapped {
            contacts: Vec<ContactEntry>,
        }
        let wrapped: Wrapped =
            serde_json::from_str(raw).context("invalid wrapped contacts JSON")?;
        return Ok(wrapped.contacts);
    }

    let list: Vec<ContactEntry> =
        serde_json::from_str(raw).context("invalid contacts list JSON")?;
    Ok(list)
}

fn rank_contact(entry: &ContactEntry, selector: &str) -> Option<u8> {
    let alias_lower = entry.alias.to_ascii_lowercase();
    let hash_lower = entry.hash.to_ascii_lowercase();

    if hash_lower == selector {
        return Some(0);
    }
    if alias_lower == selector {
        return Some(1);
    }
    if hash_lower.starts_with(selector) {
        return Some(2);
    }
    if alias_lower.starts_with(selector) {
        return Some(3);
    }
    if alias_lower.contains(selector) {
        return Some(4);
    }
    None
}

fn dedupe_contacts(contacts: &mut Vec<ContactEntry>) {
    let mut unique: Vec<ContactEntry> = Vec::with_capacity(contacts.len());
    for contact in contacts.drain(..) {
        if let Some(index) = unique.iter().position(|entry| {
            entry.alias.eq_ignore_ascii_case(&contact.alias)
                || entry.hash.eq_ignore_ascii_case(&contact.hash)
        }) {
            unique[index] = contact;
        } else {
            unique.push(contact);
        }
    }
    *contacts = unique;
}

fn sort_contacts(contacts: &mut [ContactEntry]) {
    contacts.sort_by(compare_contacts);
}

fn compare_contacts(a: &ContactEntry, b: &ContactEntry) -> Ordering {
    a.alias
        .to_ascii_lowercase()
        .cmp(&b.alias.to_ascii_lowercase())
        .then_with(|| a.hash.cmp(&b.hash))
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
    use super::{resolve_contact_hash, select_contacts, ContactEntry};

    #[test]
    fn resolve_contact_hash_matches_alias_and_hash() {
        let contacts = vec![
            ContactEntry {
                alias: "Alice".into(),
                hash: "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".into(),
                notes: None,
            },
            ContactEntry {
                alias: "Bob".into(),
                hash: "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb".into(),
                notes: None,
            },
        ];

        assert_eq!(
            resolve_contact_hash(&contacts, "alice"),
            Some("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".into())
        );
        assert_eq!(
            resolve_contact_hash(&contacts, "@Bob"),
            Some("bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb".into())
        );
    }

    #[test]
    fn select_contacts_ranks_exact_alias_before_prefix() {
        let contacts = vec![
            ContactEntry {
                alias: "alice".into(),
                hash: "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".into(),
                notes: None,
            },
            ContactEntry {
                alias: "alice-remote".into(),
                hash: "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb".into(),
                notes: None,
            },
        ];
        let matches = select_contacts(&contacts, "alice", false);
        assert_eq!(matches.len(), 2);
        assert_eq!(matches[0].hash, "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa");
    }
}
