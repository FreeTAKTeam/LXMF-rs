use crate::cli::app::{Cli, ProfileAction, ProfileCommand};
use crate::cli::output::Output;
use crate::cli::profile::{
    export_identity, import_identity, init_profile, list_profiles, load_profile_settings,
    normalize_display_name, profile_exists, profile_paths, remove_profile, save_profile_settings,
    select_profile, selected_profile_name,
};
use anyhow::{anyhow, Context, Result};
use serde_json::json;
use std::path::Path;

pub fn run(cli: &Cli, command: &ProfileCommand, output: &Output) -> Result<()> {
    match &command.action {
        ProfileAction::Init { name, managed, rpc } => {
            let profile = init_profile(name, *managed, rpc.clone())?;
            select_profile(name)?;
            output.emit_status(&json!({
                "created": true,
                "selected": name,
                "managed": profile.managed,
                "rpc": profile.rpc,
                "path": profile_paths(name)?.root.display().to_string(),
            }))
        }
        ProfileAction::List => {
            let selected = selected_profile_name()?;
            let profiles = list_profiles()?;
            if cli.json {
                output.emit_status(&json!({
                    "profiles": profiles,
                    "selected": selected,
                }))
            } else {
                let lines = profiles
                    .iter()
                    .map(|name| {
                        if selected.as_deref() == Some(name.as_str()) {
                            format!("* {name} (selected)")
                        } else {
                            format!("  {name}")
                        }
                    })
                    .collect::<Vec<_>>();
                output.emit_lines(&lines);
                Ok(())
            }
        }
        ProfileAction::Show { name } => {
            let name = resolve_profile_name(name.as_deref(), &cli.profile)?;
            let profile = load_profile_settings(&name)?;
            let paths = profile_paths(&name)?;
            output.emit_status(&json!({
                "name": name,
                "settings": profile,
                "paths": {
                    "root": paths.root,
                    "profile_toml": paths.profile_toml,
                    "reticulum_toml": paths.reticulum_toml,
                    "daemon_pid": paths.daemon_pid,
                    "daemon_log": paths.daemon_log,
                    "identity": paths.identity_file,
                    "db": paths.daemon_db,
                },
            }))
        }
        ProfileAction::Select { name } => {
            if !profile_exists(name)? {
                return Err(anyhow!("profile '{}' does not exist", name));
            }
            select_profile(name)?;
            output.emit_status(&json!({"selected": name}))
        }
        ProfileAction::Set {
            display_name,
            clear_display_name,
            name,
        } => {
            if display_name.is_some() && *clear_display_name {
                return Err(anyhow!(
                    "cannot set and clear display name at the same time"
                ));
            }
            if display_name.is_none() && !clear_display_name {
                return Err(anyhow!(
                    "no changes requested; use --display-name or --clear-display-name"
                ));
            }

            let name = resolve_profile_name(name.as_deref(), &cli.profile)?;
            let mut profile = load_profile_settings(&name)?;

            if *clear_display_name {
                profile.display_name = None;
            } else if let Some(display_name) = display_name {
                profile.display_name = Some(normalize_display_name(display_name)?);
            }

            save_profile_settings(&profile)?;
            output.emit_status(&json!({
                "profile": name,
                "display_name": profile.display_name,
            }))
        }
        ProfileAction::ImportIdentity { path, name } => {
            let name = resolve_profile_name(name.as_deref(), &cli.profile)?;
            let imported = import_identity(Path::new(path), &name)
                .with_context(|| format!("failed to import identity from {}", path))?;
            output.emit_status(&json!({
                "profile": name,
                "identity": imported,
            }))
        }
        ProfileAction::ExportIdentity { path, name } => {
            let name = resolve_profile_name(name.as_deref(), &cli.profile)?;
            let exported = export_identity(Path::new(path), &name)
                .with_context(|| format!("failed to export identity to {}", path))?;
            output.emit_status(&json!({
                "profile": name,
                "identity": exported,
            }))
        }
        ProfileAction::Delete { name, force } => {
            if !force {
                return Err(anyhow!(
                    "deleting profile '{}' requires --force to avoid accidental removal",
                    name
                ));
            }
            remove_profile(name)?;
            let selected = selected_profile_name()?;
            if selected.as_deref() == Some(name) {
                let fallback = list_profiles()?.into_iter().next();
                if let Some(fallback) = fallback {
                    select_profile(&fallback)?;
                }
            }
            output.emit_status(&json!({"deleted": name}))
        }
    }
}

fn resolve_profile_name(requested: Option<&str>, cli_profile: &str) -> Result<String> {
    if let Some(name) = requested {
        return Ok(name.to_string());
    }

    if profile_exists(cli_profile)? {
        return Ok(cli_profile.to_string());
    }

    if let Some(selected) = selected_profile_name()? {
        return Ok(selected);
    }

    let default_name = "default";
    if !profile_exists(default_name)? {
        let mut profile = init_profile(default_name, false, None)?;
        profile.name = default_name.to_string();
        save_profile_settings(&profile)?;
    }
    Ok(default_name.to_string())
}
