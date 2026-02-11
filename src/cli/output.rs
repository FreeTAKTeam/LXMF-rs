use anyhow::Result;
use comfy_table::{presets::UTF8_FULL, Cell, ContentArrangement, Table};
use serde::Serialize;
use serde_json::{Map, Value};

#[derive(Debug, Clone)]
pub struct Output {
    pub json: bool,
    pub quiet: bool,
}

impl Output {
    pub fn new(json: bool, quiet: bool) -> Self {
        Self { json, quiet }
    }

    pub fn emit_status<T: Serialize>(&self, value: &T) -> Result<()> {
        if self.quiet {
            return Ok(());
        }
        if self.json {
            println!("{}", serde_json::to_string_pretty(value)?);
        } else {
            let json = serde_json::to_value(value)?;
            self.print_object_as_table("status", &json);
        }
        Ok(())
    }

    pub fn emit_list<T: Serialize>(&self, list: &[T], title: &str) -> Result<()> {
        if self.quiet {
            return Ok(());
        }

        if self.json {
            println!("{}", serde_json::to_string_pretty(list)?);
            return Ok(());
        }

        let mut table = Table::new();
        table
            .load_preset(UTF8_FULL)
            .set_content_arrangement(ContentArrangement::Dynamic);
        table.set_header(vec!["#", title]);
        for (idx, item) in list.iter().enumerate() {
            table.add_row(vec![
                Cell::new(idx + 1),
                Cell::new(serde_json::to_string(item).unwrap_or_else(|_| "<invalid>".into())),
            ]);
        }
        println!("{table}");
        Ok(())
    }

    pub fn emit_lines(&self, lines: &[String]) {
        if self.quiet {
            return;
        }
        for line in lines {
            println!("{line}");
        }
    }

    pub fn emit_message(&self, message: impl AsRef<str>) {
        if self.quiet {
            return;
        }
        println!("{}", message.as_ref());
    }

    pub fn emit_kv_rows(&self, title: &str, rows: &[(String, String)]) {
        if self.quiet {
            return;
        }

        if self.json {
            let mut map = Map::new();
            for (k, v) in rows {
                map.insert(k.clone(), Value::String(v.clone()));
            }
            println!(
                "{}",
                serde_json::to_string_pretty(&Value::Object(map)).unwrap_or_else(|_| "{}".into())
            );
            return;
        }

        let mut table = Table::new();
        table
            .load_preset(UTF8_FULL)
            .set_content_arrangement(ContentArrangement::Dynamic);
        table.set_header(vec!["field", "value"]);
        for (k, v) in rows {
            let key_cell = Cell::new(k);
            table.add_row(vec![key_cell, Cell::new(v)]);
        }

        println!("{title}");
        println!("{table}");
    }

    fn print_object_as_table(&self, title: &str, value: &Value) {
        match value {
            Value::Object(obj) => {
                let rows = obj
                    .iter()
                    .map(|(k, v)| (k.clone(), to_string(v)))
                    .collect::<Vec<_>>();
                self.emit_kv_rows(title, &rows);
            }
            _ => self.emit_message(to_string(value)),
        }
    }
}

fn to_string(value: &Value) -> String {
    match value {
        Value::Null => "null".into(),
        Value::Bool(v) => v.to_string(),
        Value::Number(v) => v.to_string(),
        Value::String(v) => v.clone(),
        other => serde_json::to_string(other).unwrap_or_else(|_| "<invalid>".into()),
    }
}
