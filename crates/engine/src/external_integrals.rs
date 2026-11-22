//! Mesh-bound integral quantities from an external analysis, displayed by the command reply panel.
use crate::cmd::{file, file_param, forms};
use crate::requirement::Requirement;
use crate::{CommandKind, CommandSpec, EngineError, Result, Session};
use fieldcraft_io::roots::Access;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::BTreeSet;

pub(crate) const READ: CommandSpec = CommandSpec {
    id: "results.external.integrals",
    label: "External Integral Results",
    params: r#"{"path":"results.json"} — show named scalar and vector quantities with units from an external analysis of this mesh"#,
    form: Some(forms::EXTERNAL_INTEGRALS),
    kind: CommandKind::Session,
    requires: &[Requirement::Mesh],
    run: read,
};

#[derive(Deserialize)]
struct Manifest {
    format: String,
    mesh_sha256: String,
    integrals: Vec<Quantity>,
}

#[derive(Deserialize, Serialize)]
struct Quantity {
    name: String,
    unit: String,
    value: Components,
}

#[derive(Deserialize, Serialize)]
#[serde(untagged)]
enum Components {
    Scalar(f64),
    Vector([f64; 3]),
}

fn read(s: &mut Session, p: &Value) -> Result<Value> {
    let path = file_param(s, p, "path", Access::Read)?;
    let bytes = file::read_file(&path, 4 * 1024 * 1024)?;
    let mesh_hash = fieldcraft_io::sha256_hex(&fieldcraft_io::mesh::encode(&s.model)?);
    quantities(&bytes, &mesh_hash)
}

fn quantities(bytes: &[u8], mesh_hash: &str) -> Result<Value> {
    let manifest: Manifest = serde_json::from_slice(bytes).map_err(|e| EngineError::Params(format!("invalid external results: {e}")))?;
    if manifest.format != "fieldcraft-external-run/1" || manifest.mesh_sha256 != mesh_hash {
        return Err(EngineError::Params("expected fieldcraft-external-run/1 for the current mesh".into()));
    }
    let mut names = BTreeSet::new();
    for q in &manifest.integrals {
        let valid_label = |value: &str, limit| !value.trim().is_empty() && value.len() <= limit && !value.chars().any(char::is_control);
        if !valid_label(&q.name, 120) || !valid_label(&q.unit, 48) || !names.insert(&q.name) {
            return Err(EngineError::Params("integrals need unique names and units".into()));
        }
    }
    Ok(json!({ "integrals": manifest.integrals }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_integral_units_and_rejects_another_mesh_or_duplicate_names() {
        let q = json!({"name":"force","unit":"N","value":[1.,2.,3.]});
        let mut example = json!({"format":"fieldcraft-external-run/1","mesh_sha256":"abc","integrals":[q]});
        assert_eq!(quantities(example.to_string().as_bytes(), "abc").unwrap()["integrals"][0], q);
        assert!(quantities(example.to_string().as_bytes(), "xyz").is_err());
        example["integrals"] = json!([q, q]);
        assert!(quantities(example.to_string().as_bytes(), "abc").is_err());
        example["integrals"][0]["value"] = json!([1., 2.]);
        assert!(quantities(example.to_string().as_bytes(), "abc").is_err());
    }
}
