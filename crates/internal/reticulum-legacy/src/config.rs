use crate::error::RnsError;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Config {
    pub interfaces: Vec<String>,
}

impl Config {
    pub fn from_ini(ini: &str) -> Result<Self, RnsError> {
        let mut interfaces = Vec::new();
        let mut in_interfaces = false;

        for raw_line in ini.lines() {
            let line = raw_line.trim();

            if line.is_empty() || line.starts_with('#') || line.starts_with(';') {
                continue;
            }

            if line.starts_with('[') && line.ends_with(']') {
                let section = &line[1..line.len() - 1];
                in_interfaces = section.trim().eq_ignore_ascii_case("interfaces");
                continue;
            }

            if in_interfaces {
                if let Some((key, value)) = line.split_once('=') {
                    let key = key.trim();
                    if key.starts_with("interface") {
                        let value = value.trim();
                        if !value.is_empty() {
                            interfaces.push(value.to_string());
                        }
                    }
                }
            }
        }

        Ok(Self { interfaces })
    }
}
