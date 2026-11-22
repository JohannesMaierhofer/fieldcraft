//! A mesh-bound scalar contour from a separately run program. The file is session input:
//! reloading it reproduces the display; it never masquerades as a native solved step.
use crate::cmd::{file, file_param, forms, opt};
use crate::requirement::Requirement;
use crate::{CommandKind, CommandSpec, EngineError, Result, Session};
use fieldcraft_io::roots::Access;
use fieldcraft_render::nodal::{FieldFile, NodalContour};
use serde_json::{Value, json};
use std::sync::Arc;

pub(crate) const CONTOUR: CommandSpec = CommandSpec {
    id: "results.external.contour",
    label: "Show External Nodal Field",
    params: r#"{"path":"pressure.field.json", "component"?: "auto|magnitude|x|y|z"} — load a fieldcraft-nodal-field/1 on this exact SI mesh; the file supplies the name, unit and scalar or vector values for each original node label. A vector defaults to its magnitude. {"show":false} removes the external contour."#,
    form: Some(forms::EXTERNAL_CONTOUR),
    kind: CommandKind::Session,
    requires: &[Requirement::Mesh],
    run: contour,
};

#[derive(Debug)]
pub struct ExternalField {
    revision: u64,
    pub display_id: u64,
    contour: NodalContour,
}

impl std::ops::Deref for ExternalField {
    type Target = NodalContour;
    fn deref(&self) -> &NodalContour {
        &self.contour
    }
}

impl Session {
    /// Any intervening model edit invalidates the display until the file is rechecked.
    pub fn external_contour(&self) -> Option<&ExternalField> {
        self.external_field.as_deref().filter(|f| f.revision == self.revision() && self.results_view.is_none())
    }
}

fn invalid(msg: &str) -> EngineError {
    EngineError::Params(msg.into())
}

fn contour(s: &mut Session, p: &Value) -> Result<Value> {
    if opt::<bool>(p, "show")? == Some(false) {
        s.external_field = None;
        return Ok(json!({"shown":false}));
    }
    let path = file_param(s, p, "path", Access::Read)?;
    let bytes = file::read_file(&path, 64 * 1024 * 1024)?;
    let data: FieldFile = serde_json::from_slice(&bytes).map_err(|e| EngineError::Params(format!("invalid nodal field: {e}")))?;
    let component = opt::<&str>(p, "component")?.filter(|value| *value != "auto");
    let hash = fieldcraft_io::sha256_hex(&fieldcraft_io::mesh::encode(&s.model)?);
    let contour = data.prepare(s.model.nodes().keys(), &hash, component).map_err(invalid)?;
    let field = ExternalField { revision: s.revision(), display_id: crate::next_revision(), contour };
    let reply = json!({"shown": true, "field":field.title, "min":field.min, "max":field.max, "nodes":s.model.nodes().len()});
    s.results_view = None;
    s.external_field = Some(Arc::new(field));
    Ok(reply)
}
