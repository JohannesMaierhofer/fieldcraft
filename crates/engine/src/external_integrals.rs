//! Mesh-bound integral quantities from an external analysis, displayed by the command reply panel.
use crate::cmd::{file_param, forms, opt};
use crate::requirement::Requirement;
use crate::{CommandKind, CommandSpec, Result, Session};
use fieldcraft_io::roots::Access;
use serde_json::{Value, json};
use std::path::Path;
use std::sync::Arc;

pub(crate) const READ: CommandSpec = CommandSpec {
    id: "results.external.integrals",
    label: "External Integral Results",
    params: r#"{"path":"results.json","catalog_only"?:false} — register fields, time series, integrals and named regions from an external run on this mesh; the default reply lists named scalar and vector quantities with units"#,
    form: Some(forms::EXTERNAL_INTEGRALS),
    kind: CommandKind::Session,
    requires: &[Requirement::Mesh],
    run: read,
};

fn read(s: &mut Session, p: &Value) -> Result<Value> {
    let path = file_param(s, p, "path", Access::Read)?;
    let catalog_only = opt::<bool>(p, "catalog_only")?.unwrap_or(false);
    let catalog = s.read_external_catalog(Path::new(&path), catalog_only)?;
    let reply = if catalog_only {
        json!({"registered":true, "fields":catalog.fields.len(), "series":catalog.series.len(), "integrals":catalog.integrals.len()})
    } else {
        json!({"integrals":catalog.integrals})
    };
    s.result_catalog = Some(Arc::new(catalog));
    Ok(reply)
}
