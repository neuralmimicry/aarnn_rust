//! Explicit model/IO digest encoding. This is a source-schema witness, separate
//! from the authoritative neural runtime's state digest and logical semantics.
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::path::Path;
fn encode(value: &Value, hash: &mut Sha256) -> Result<(), String> {
    match value {
        Value::Null => hash.update(b"n"),
        Value::Bool(v) => hash.update(if *v { b"t" } else { b"f" }),
        Value::Number(n) => {
            if n.is_i64() || n.is_u64() {
                hash.update(b"i");
                hash.update(n.to_string().as_bytes());
                hash.update(b";");
            } else {
                let value = n.as_f64().ok_or("invalid model scalar")?;
                if !value.is_finite() {
                    return Err("nonfinite model scalar".into());
                }
                hash.update(b"d");
                hash.update(value.to_le_bytes());
            }
        }
        Value::String(s) => {
            hash.update(b"s");
            hash.update((s.len() as u64).to_le_bytes());
            hash.update(s.as_bytes());
        }
        Value::Array(values) => {
            hash.update(b"a");
            hash.update((values.len() as u64).to_le_bytes());
            for value in values {
                encode(value, hash)?;
            }
        }
        Value::Object(values) => {
            hash.update(b"o");
            hash.update((values.len() as u64).to_le_bytes());
            let mut keys: Vec<_> = values.keys().collect();
            keys.sort();
            for key in keys {
                encode(&Value::String(key.clone()), hash)?;
                encode(&values[key], hash)?;
            }
        }
    }
    Ok(())
}
pub fn digest(value: &Value) -> Result<String, String> {
    let mut hash = Sha256::new();
    encode(value, &mut hash)?;
    Ok(format!("{:x}", hash.finalize()))
}
pub fn verify_record(record: &Value) -> Result<(), String> {
    for value in [record, record.get("io").ok_or("IO manifest missing")?] {
        if value.get("schema_version").and_then(Value::as_u64) != Some(1)
            || value.get("digest_encoding").and_then(Value::as_str) != Some("embodiment-binary-v1")
        {
            return Err("unsupported model/IO schema".into());
        }
        let expected = value
            .get("digest")
            .and_then(Value::as_str)
            .ok_or("digest missing")?;
        let mut content = value.clone();
        content
            .as_object_mut()
            .ok_or("invalid record")?
            .remove("digest");
        if digest(&content)? != expected {
            return Err("model/IO content digest mismatch".into());
        }
    }
    Ok(())
}
pub fn load_model(path: &Path) -> Result<crate::aer::Model, String> {
    let bytes = std::fs::read(path.with_extension("json")).map_err(|e| e.to_string())?;
    if bytes.len() > 8 * 1024 * 1024 {
        return Err("model record budget".into());
    }
    let value: Value = serde_json::from_slice(&bytes).map_err(|e| e.to_string())?;
    verify_record(&value)?;
    let model: crate::aer::Model = serde_json::from_value(value).map_err(|e| e.to_string())?;
    let xml = std::fs::read(path).map_err(|e| e.to_string())?;
    if format!("{:x}", Sha256::digest(&xml)) != model.native_mjcf_sha256 {
        return Err("native model bytes differ from pinned record".into());
    }
    Ok(model)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn typed_numbers_and_key_order_are_stable() {
        let a: Value =
            serde_json::from_str(r#"{"z":0.000001,"a":[-0.0,1,1.0,true,null,"µ"]}"#).unwrap();
        let b: Value =
            serde_json::from_str(r#"{"a":[-0.0,1,1.0,true,null,"µ"],"z":1e-6}"#).unwrap();
        assert_eq!(digest(&a).unwrap(), digest(&b).unwrap());
        assert_ne!(
            digest(&serde_json::json!(1)).unwrap(),
            digest(&serde_json::json!(1.0)).unwrap()
        );
    }
}
