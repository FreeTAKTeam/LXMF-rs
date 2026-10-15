use crate::ApiError;
use rand_core::RngCore;
use serde_json::{Map, Value};
use std::{fs, io::Write, path::PathBuf, str::FromStr, sync::Mutex};
use toml_edit::{ArrayOfTables, DocumentMut, Item, Table};

pub(crate) struct ConfigStore {
    path: PathBuf,
    lock: Mutex<()>,
}

impl ConfigStore {
    pub(crate) fn new(path: PathBuf) -> Self {
        Self { path, lock: Mutex::new(()) }
    }

    pub(crate) fn path(&self) -> &std::path::Path {
        &self.path
    }

    pub(super) fn read<T>(
        &self,
        operation: impl FnOnce(&DocumentMut) -> Result<T, ApiError>,
    ) -> Result<T, ApiError> {
        let _guard = self.lock.lock().map_err(|_| ApiError::unavailable("config lock poisoned"))?;
        let document = self.load()?;
        operation(&document)
    }

    pub(super) fn edit<T>(
        &self,
        operation: impl FnOnce(&mut DocumentMut) -> Result<T, ApiError>,
    ) -> Result<T, ApiError> {
        let _guard = self.lock.lock().map_err(|_| ApiError::unavailable("config lock poisoned"))?;
        let mut document = self.load()?;
        let result = operation(&mut document)?;
        self.save(&document)?;
        Ok(result)
    }

    fn load(&self) -> Result<DocumentMut, ApiError> {
        let source = fs::read_to_string(&self.path).map_err(|error| {
            ApiError::unavailable(format!(
                "Cannot read daemon config {}: {error}",
                self.path.display()
            ))
        })?;
        DocumentMut::from_str(&source)
            .map_err(|error| ApiError::unavailable(format!("Cannot parse daemon config: {error}")))
    }

    fn save(&self, document: &DocumentMut) -> Result<(), ApiError> {
        let parent =
            self.path.parent().ok_or_else(|| ApiError::unavailable("Config path has no parent"))?;
        let original = fs::metadata(&self.path).map_err(|error| {
            ApiError::unavailable(format!("Cannot stat daemon config: {error}"))
        })?;
        let temp = parent.join(format!(
            ".meshchat-config-{}-{}.tmp",
            std::process::id(),
            rand_core::OsRng.next_u64()
        ));
        let result = (|| -> std::io::Result<()> {
            let mut file = fs::OpenOptions::new().write(true).create_new(true).open(&temp)?;
            file.set_permissions(original.permissions())?;
            file.write_all(document.to_string().as_bytes())?;
            file.sync_all()?;
            fs::rename(&temp, &self.path)?;
            Ok(())
        })();
        if result.is_err() {
            let _ = fs::remove_file(&temp);
        }
        result.map_err(|error| ApiError::unavailable(format!("Cannot save daemon config: {error}")))
    }
}

pub(super) fn interface_names(doc: &DocumentMut) -> Result<Vec<String>, ApiError> {
    let Some(item) = doc.get("interfaces") else {
        return Ok(Vec::new());
    };
    if let Some(table) = item.as_table() {
        return Ok(table.iter().map(|(name, _)| name.to_string()).collect());
    }
    if let Some(array) = item.as_array_of_tables() {
        return array
            .iter()
            .map(|table| {
                table
                    .get("name")
                    .and_then(Item::as_str)
                    .map(str::to_string)
                    .ok_or_else(|| ApiError::unavailable("An interface record has no name"))
            })
            .collect();
    }
    Err(ApiError::unavailable("Daemon interfaces must be a table or array of tables"))
}

pub(super) fn interface_mut<'a>(doc: &'a mut DocumentMut, name: &str) -> Option<&'a mut Table> {
    let item = doc.get_mut("interfaces")?;
    if item.is_table() {
        return item.as_table_mut()?.get_mut(name)?.as_table_mut();
    }
    item.as_array_of_tables_mut()?
        .iter_mut()
        .find(|table| table.get("name").and_then(Item::as_str) == Some(name))
}

pub(super) fn insert_interface(
    doc: &mut DocumentMut,
    name: &str,
    mut record: Table,
) -> Result<(), ApiError> {
    if doc.get("interfaces").is_none() {
        doc["interfaces"] = Item::ArrayOfTables(ArrayOfTables::new());
    }
    let item = doc.get_mut("interfaces").expect("inserted interfaces");
    if let Some(table) = item.as_table_mut() {
        record.remove("name");
        table.insert(name, Item::Table(record));
        return Ok(());
    }
    if let Some(array) = item.as_array_of_tables_mut() {
        record.insert("name", toml_edit::value(name));
        array.push(record);
        return Ok(());
    }
    Err(ApiError::unavailable("Daemon interfaces must be a table or array of tables"))
}

pub(super) fn remove_interface(doc: &mut DocumentMut, name: &str) -> Result<bool, ApiError> {
    let Some(item) = doc.get_mut("interfaces") else {
        return Ok(false);
    };
    if let Some(table) = item.as_table_mut() {
        return Ok(table.remove(name).is_some());
    }
    if let Some(array) = item.as_array_of_tables_mut() {
        let index =
            array.iter().position(|table| table.get("name").and_then(Item::as_str) == Some(name));
        if let Some(index) = index {
            array.remove(index);
            return Ok(true);
        }
        return Ok(false);
    }
    Err(ApiError::unavailable("Daemon interfaces must be a table or array of tables"))
}

pub(super) fn saved_interfaces(doc: &DocumentMut) -> Result<Map<String, Value>, ApiError> {
    let parsed: toml::Value = toml::from_str(&doc.to_string())
        .map_err(|error| ApiError::unavailable(format!("Cannot parse daemon config: {error}")))?;
    let mut result = Map::new();
    match parsed.get("interfaces") {
        None => {}
        Some(toml::Value::Table(table)) => {
            for (name, value) in table {
                result.insert(name.clone(), interface_json(value)?);
            }
        }
        Some(toml::Value::Array(array)) => {
            for value in array {
                let name = value
                    .get("name")
                    .and_then(toml::Value::as_str)
                    .ok_or_else(|| ApiError::unavailable("An interface record has no name"))?;
                if result.insert(name.to_string(), interface_json(value)?).is_some() {
                    return Err(ApiError::unavailable(format!("Duplicate interface name {name}")));
                }
            }
        }
        Some(_) => {
            return Err(ApiError::unavailable(
                "Daemon interfaces must be a table or array of tables",
            ))
        }
    }
    Ok(result)
}

fn interface_json(value: &toml::Value) -> Result<Value, ApiError> {
    let mut value =
        serde_json::to_value(value).map_err(|error| ApiError::unavailable(error.to_string()))?;
    let object =
        value.as_object_mut().ok_or_else(|| ApiError::unavailable("Interface is not a table"))?;
    object.remove("name");
    if let Some(kind) = object.get("type").and_then(Value::as_str) {
        let meshchat_kind = match kind {
            "auto" => "AutoInterface",
            "tcp_client" => "TCPClientInterface",
            "tcp_server" => "TCPServerInterface",
            "udp" => "UDPInterface",
            "serial" => "SerialInterface",
            "kiss" => "KISSInterface",
            "ax25_kiss" => "AX25KISSInterface",
            "lora" => "RNodeInterface",
            "rnode_multi" => "RNodeMultiInterface",
            "pipe" => "PipeInterface",
            "i2p" => "I2PInterface",
            other => other,
        };
        object.insert("type".into(), Value::String(meshchat_kind.into()));
    }
    if object.get("type").and_then(Value::as_str) == Some("RNodeMultiInterface") {
        let children = object
            .iter()
            .filter_map(|(name, value)| {
                value.as_object().map(|child| {
                    let mut child = child.clone();
                    child.insert("name".into(), Value::String(name.clone()));
                    Value::Object(child)
                })
            })
            .collect::<Vec<_>>();
        for child in &children {
            if let Some(name) = child.get("name").and_then(Value::as_str) {
                object.remove(name);
            }
        }
        object.insert("sub_interfaces".into(), Value::Array(children));
    }
    Ok(value)
}
