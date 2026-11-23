//! A mesh-bound scalar contour from a separately run program. The file is session input:
//! reloading it reproduces the display; it never masquerades as a native solved step.
use crate::cmd::{file, forms, opt};
use crate::external_catalog::{Kind, Selection};
use crate::requirement::Requirement;
use crate::{CommandKind, CommandSpec, EngineError, Result, Session};
use fieldcraft_render::nodal::{FieldFile, NodalContour};
use serde_json::{Value, json};
use std::sync::Arc;

pub(crate) const CONTOUR: CommandSpec = CommandSpec {
    id: "results.external.contour",
    label: "Show External Nodal Field",
    params: r#"{"path":"pressure.field.json", "component"?: "auto|magnitude|x|y|z"} — load a fieldcraft-nodal-field/1 on this exact SI mesh; the file supplies the name, unit and scalar or vector values for each original node label. Use {"entry":"name"} instead of path to reopen a checked catalog row. A vector defaults to its magnitude. {"show":false} removes the external contour."#,
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

impl ExternalField {
    pub(crate) fn new(contour: NodalContour, revision: u64) -> Self {
        Self { revision, display_id: crate::next_revision(), contour }
    }
    pub(crate) fn set_range(&mut self, range: [f64; 2]) {
        self.contour.range = range;
    }
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
        self.external_series()
            .and_then(|series| series.field())
            .or_else(|| self.external_field.as_deref().filter(|f| f.revision == self.revision() && self.results_view.is_none()))
    }
}

fn invalid(msg: &str) -> EngineError {
    EngineError::Params(msg.into())
}

fn contour(s: &mut Session, p: &Value) -> Result<Value> {
    if opt::<bool>(p, "show")? == Some(false) {
        s.field_series = None;
        s.external_field = None;
        s.selected_result = None;
        return Ok(json!({"shown":false}));
    }
    let selected = Selection::resolve(s, p, Kind::Field)?;
    let bytes = file::read_file(&selected.path, Kind::Field.limit())?;
    let data: FieldFile = serde_json::from_slice(&bytes).map_err(|e| EngineError::Params(format!("invalid nodal field: {e}")))?;
    let component = opt::<&str>(p, "component")?.filter(|value| *value != "auto");
    let hash = fieldcraft_io::sha256_hex(&fieldcraft_io::mesh::encode(&s.model)?);
    if data.name().len() > 120 || data.unit().len() > 48 {
        return Err(invalid("invalid field name or unit"));
    }
    let (name, unit) = (data.name().to_owned(), data.unit().to_owned());
    let contour = data.prepare(s.model.nodes().keys(), &hash, component).map_err(invalid)?;
    let field = ExternalField::new(contour, s.revision());
    let reply = json!({"shown": true, "field":field.title, "min":field.min, "max":field.max, "nodes":s.model.nodes().len()});
    selected.register(s, &name, &unit)?;
    s.results_view = None;
    s.field_series = None;
    s.external_field = Some(Arc::new(field));
    Ok(reply)
}
