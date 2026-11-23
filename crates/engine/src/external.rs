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
    params: r#"{"path":"pressure.field.json", "component"?: "auto|magnitude|x|y|z"} — load a fieldcraft-nodal-field/1 on this exact SI mesh; the file supplies the name, unit and scalar or vector values for each original node label. Use {"entry":"name"} instead of path to reopen a checked catalog row. A vector defaults to its magnitude. For a three-component field in meters, deformation_scale draws the mesh at X + scale * u. Zero shows the reference shape and one shows true scale. {"show":false} removes the external contour."#,
    form: Some(forms::EXTERNAL_CONTOUR),
    kind: CommandKind::Session,
    requires: &[Requirement::Mesh],
    run: contour,
};

pub(crate) const DEFORMED: CommandSpec = CommandSpec {
    id: "results.external.deformed",
    label: "External Displacement Scale",
    params: r#"{"scale":1.0} — draw the current external displacement field in meters at a dimensionless magnification. Zero shows the reference mesh. One shows the true shape. The multiplier applies to all frames of a series and only affects the display."#,
    form: Some(forms::EXTERNAL_DEFORMED),
    kind: CommandKind::Session,
    requires: &[Requirement::ExternalDisplacement],
    run: deformed,
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
    /// The dimensionless external displacement magnification, or zero for the reference mesh.
    pub fn external_deformation_scale(&self) -> f64 {
        if self.external_contour().is_some() { self.external_deformation_scale } else { 0. }
    }

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

pub(crate) fn validate_scale(s: &Session, scale: f64, field: &NodalContour) -> Result<()> {
    if !(0.0..=crate::results::MAX_DEFORMATION_SCALE).contains(&scale) {
        return Err(invalid("invalid external displacement scale"));
    }
    if scale == 0.0 {
        return Ok(());
    }
    if !field.has_displacements() {
        return Err(invalid("mesh deformation requires a three-component nodal field in meters (m)"));
    }
    for (label, node) in s.model.nodes() {
        let u = field.displacement(*label).ok_or_else(|| invalid("missing nodal displacement"))?;
        let p = fieldcraft_render::Vec3::from(node.xyz) + u * scale;
        if !p.to_f32().iter().all(|x| x.is_finite()) {
            return Err(invalid("scaled displacement exceeds the display coordinate range"));
        }
    }
    Ok(())
}

pub(crate) fn deformation_scale(s: &Session, p: &Value, field: &NodalContour) -> Result<f64> {
    let scale = opt::<f64>(p, "deformation_scale")?.unwrap_or_default();
    validate_scale(s, scale, field)?;
    Ok(scale)
}

fn deformed(s: &mut Session, p: &Value) -> Result<Value> {
    let scale = opt::<f64>(p, "scale")?.unwrap_or(1.0);
    if let Some(series) = s.external_series() {
        series.validate_scale(s, scale)?;
    } else {
        let field = s.external_contour().ok_or_else(|| invalid("open a nodal displacement field first"))?;
        validate_scale(s, scale, field)?;
    }
    s.external_deformation_scale = scale;
    Ok(json!({"scale":scale,"deformed":scale > 0.0}))
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
    let scale = deformation_scale(s, p, &contour)?;
    let field = ExternalField::new(contour, s.revision());
    let reply = json!({"shown": true, "field":field.title, "min":field.min, "max":field.max, "nodes":s.model.nodes().len()});
    selected.register(s, &name, &unit)?;
    s.results_view = None;
    s.field_series = None;
    s.external_deformation_scale = scale;
    s.external_field = Some(Arc::new(field));
    Ok(reply)
}
