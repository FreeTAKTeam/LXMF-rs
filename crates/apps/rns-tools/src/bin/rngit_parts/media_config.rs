use std::fs;
use std::io;
use std::path::Path;

/// Read the optional `[pages] media_conversion` setting from rngit's config
/// directory, matching the pinned `ReticulumGitNode` default and override.
pub(crate) fn media_conversion_enabled(config_dir: Option<&Path>) -> io::Result<bool> {
    let Some(config_dir) = config_dir else { return Ok(true) };
    let contents = match fs::read_to_string(config_dir.join("config")) {
        Ok(contents) => contents,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(true),
        Err(error) => return Err(error),
    };

    let mut in_pages = false;
    for line in contents.lines() {
        let line = line.split(['#', ';']).next().unwrap_or_default().trim();
        if line.is_empty() {
            continue;
        }
        if line.starts_with('[') && line.ends_with(']') {
            in_pages = line[1..line.len() - 1].trim() == "pages";
            continue;
        }
        if !in_pages {
            continue;
        }
        let Some((key, value)) = line.split_once('=') else { continue };
        if key.trim() != "media_conversion" {
            continue;
        }
        let enabled = match value.trim().to_ascii_lowercase().as_str() {
            "yes" | "true" | "on" | "1" => true,
            "no" | "false" | "off" | "0" => false,
            value => {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("invalid [pages] media_conversion value {value:?}"),
                ));
            }
        };
        return Ok(enabled);
    }
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::media_conversion_enabled;

    #[test]
    fn media_conversion_defaults_on_and_reads_pinned_config_boolean_forms() {
        assert!(media_conversion_enabled(None).expect("default setting"));
        let temporary = tempfile::tempdir().expect("temporary config directory");
        std::fs::write(
            temporary.path().join("config"),
            "[rngit]\nmedia_conversion = no\n[pages]\nmedia_conversion = off ; disabled\n",
        )
        .expect("write rngit config");
        assert!(!media_conversion_enabled(Some(temporary.path())).expect("configured setting"));

        std::fs::write(temporary.path().join("config"), "[pages]\nmedia_conversion = yes\n")
            .expect("rewrite rngit config");
        assert!(media_conversion_enabled(Some(temporary.path())).expect("enabled setting"));
    }

    #[test]
    fn malformed_media_conversion_setting_is_reported() {
        let temporary = tempfile::tempdir().expect("temporary config directory");
        std::fs::write(temporary.path().join("config"), "[pages]\nmedia_conversion = sometimes\n")
            .expect("write malformed rngit config");
        let error = media_conversion_enabled(Some(temporary.path())).expect_err("invalid boolean");
        assert_eq!(error.kind(), std::io::ErrorKind::InvalidData);
        assert!(error.to_string().contains("media_conversion"));
    }
}
