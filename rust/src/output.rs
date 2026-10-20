use std::{fs::File, io::Write, path::Path};

use anyhow::Result;
use serde_json::Value;

fn cell(v: &Value) -> String {
    match v {
        Value::Null => String::new(),
        Value::String(s) => s.clone(),
        other => other.to_string(),
    }
}

/// Writes JSON, JSONL or CSV based on the file extension, or JSON to stdout.
pub fn write_records(records: &[Value], path: Option<&str>) -> Result<()> {
    let Some(path) = path.filter(|p| *p != "-") else {
        println!("{}", serde_json::to_string_pretty(records)?);
        return Ok(());
    };
    let ext = Path::new(path).extension().and_then(|e| e.to_str()).unwrap_or("").to_lowercase();
    let mut f = File::create(path)?;
    match ext.as_str() {
        "csv" => {
            let mut keys: Vec<String> = Vec::new();
            for k in records.iter().filter_map(Value::as_object).flat_map(|o| o.keys()) {
                if !keys.contains(k) {
                    keys.push(k.clone());
                }
            }
            let mut w = csv::Writer::from_writer(f);
            w.write_record(&keys)?;
            for r in records {
                w.write_record(keys.iter().map(|k| cell(r.get(k).unwrap_or(&Value::Null))))?;
            }
            w.flush()?;
        }
        "jsonl" => {
            for r in records {
                writeln!(f, "{}", serde_json::to_string(r)?)?;
            }
        }
        _ => serde_json::to_writer_pretty(&mut f, records)?,
    }
    Ok(())
}
