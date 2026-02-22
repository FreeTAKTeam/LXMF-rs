use crate::cli::app::{ContactAction, ContactCommand, ContactUpsertArgs, RuntimeContext};
use crate::cli::contacts::{
    export_contacts_json, filter_contacts, import_contacts_json, load_contacts,
    remove_contact_by_alias, save_contacts, select_contacts, upsert_contact, validate_contact,
    ContactEntry,
};
use anyhow::{anyhow, Result};
use serde_json::json;
use std::path::Path;

pub fn run(ctx: &RuntimeContext, command: &ContactCommand) -> Result<()> {
    match &command.action {
        ContactAction::List { query, limit } => {
            let contacts = load_contacts(&ctx.profile_name)?;
            let contacts = filter_contacts(&contacts, query.as_deref(), *limit);
            ctx.output.emit_status(&json!({ "contacts": contacts }))
        }
        ContactAction::Add(args) => add_contact(ctx, args),
        ContactAction::Show { selector, exact } => {
            let contacts = load_contacts(&ctx.profile_name)?;
            let matches = select_contacts(&contacts, selector, *exact);
            match matches.len() {
                0 => Err(anyhow!("no contact matched selector '{}'", selector)),
                1 => ctx.output.emit_status(matches[0]),
                _ => Err(anyhow!(
                    "selector '{}' is ambiguous ({} matches): {}",
                    selector,
                    matches.len(),
                    matches
                        .iter()
                        .take(5)
                        .map(|entry| format!("{}({})", entry.alias, entry.hash))
                        .collect::<Vec<_>>()
                        .join(", ")
                )),
            }
        }
        ContactAction::Remove { selector, exact } => {
            let mut contacts = load_contacts(&ctx.profile_name)?;
            let matches = select_contacts(&contacts, selector, *exact);
            match matches.len() {
                0 => Err(anyhow!("no contact matched selector '{}'", selector)),
                1 => {
                    let selected_alias = matches[0].alias.clone();
                    let selected_hash = matches[0].hash.clone();
                    if !remove_contact_by_alias(&mut contacts, &selected_alias) {
                        return Err(anyhow!("failed to remove contact '{}'", selected_alias));
                    }
                    save_contacts(&ctx.profile_name, &contacts)?;
                    ctx.output.emit_status(&json!({
                        "removed": {
                            "alias": selected_alias,
                            "hash": selected_hash,
                        },
                        "total_contacts": contacts.len(),
                    }))
                }
                _ => {
                    Err(anyhow!("selector '{}' is ambiguous ({} matches)", selector, matches.len()))
                }
            }
        }
        ContactAction::Import { path, replace } => {
            let total = import_contacts_json(&ctx.profile_name, Path::new(path), *replace)?;
            ctx.output.emit_status(&json!({
                "imported_path": path,
                "replace": replace,
                "total_contacts": total,
            }))
        }
        ContactAction::Export { path } => {
            let total = export_contacts_json(&ctx.profile_name, Path::new(path))?;
            ctx.output.emit_status(&json!({
                "exported_path": path,
                "total_contacts": total,
            }))
        }
    }
}

fn add_contact(ctx: &RuntimeContext, args: &ContactUpsertArgs) -> Result<()> {
    let mut contacts = load_contacts(&ctx.profile_name)?;
    let contact = validate_contact(ContactEntry {
        alias: args.alias.clone(),
        hash: args.hash.clone(),
        notes: args.notes.clone(),
    })?;
    let alias = contact.alias.clone();
    let hash = contact.hash.clone();
    upsert_contact(&mut contacts, contact);
    save_contacts(&ctx.profile_name, &contacts)?;

    ctx.output.emit_status(&json!({
        "contact": {
            "alias": alias,
            "hash": hash,
        },
        "total_contacts": contacts.len(),
    }))
}
