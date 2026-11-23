//! Session-only browser rows for an external run on the current mesh. Registering a run
//! reads its manifest, not the nodal arrays. Choosing a row rechecks its file and mesh.
use crate::cmd::{file, file_param, opt};
use crate::{EngineError, Result, Session};
use fieldcraft_io::roots::Access;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;

const MAX_ROWS: usize = 4096;
const MANIFEST_BYTES: u64 = 4 * 1024 * 1024;

#[derive(Debug, Clone)]
pub struct CatalogField {
    pub name: String,
    pub unit: String,
    pub path: String,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct Quantity {
    pub name: String,
    pub unit: String,
    pub value: QuantityValue,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(untagged)]
pub enum QuantityValue {
    Scalar(f64),
    Vector([f64; 3]),
}

#[derive(Debug, Default)]
pub struct ExternalCatalog {
    revision: u64,
    pub fields: Vec<CatalogField>,
    pub series: Vec<CatalogField>,
    pub integrals: Vec<Quantity>,
    pub materials: Vec<String>,
    pub boundaries: Vec<String>,
    pub interfaces: Vec<String>,
    pub loads: Vec<String>,
}

#[derive(Deserialize)]
struct Entry {
    name: String,
    unit: String,
    file: String,
}

#[derive(Deserialize)]
struct Manifest {
    format: String,
    mesh_sha256: String,
    #[serde(default)]
    fields: Vec<Entry>,
    #[serde(default)]
    series: Vec<Entry>,
    integrals: Option<Vec<Quantity>>,
    #[serde(default)]
    materials: Vec<String>,
    #[serde(default)]
    boundaries: Vec<String>,
    #[serde(default)]
    interfaces: Vec<String>,
    #[serde(default)]
    loads: Vec<String>,
}

#[derive(Clone, Copy)]
pub(crate) enum Kind {
    Field,
    Series,
}

impl Kind {
    pub(crate) fn limit(self) -> u64 {
        match self {
            Self::Field => 64 * 1024 * 1024,
            Self::Series => 1024 * 1024,
        }
    }
    fn rows(self, catalog: &ExternalCatalog) -> &[CatalogField] {
        match self {
            Self::Field => &catalog.fields,
            Self::Series => &catalog.series,
        }
    }
}

fn invalid(message: &str) -> EngineError {
    EngineError::Params(message.into())
}

fn valid_label(s: &str, bytes: usize) -> bool {
    !s.trim().is_empty() && s.len() <= bytes && !s.chars().any(char::is_control)
}

fn unique_names<'a>(count: usize, mut names: impl Iterator<Item = &'a str>) -> Result<()> {
    let mut seen = BTreeSet::new();
    if count > MAX_ROWS || !names.all(|name| valid_label(name, 120) && seen.insert(name)) {
        return Err(invalid("each catalog list needs at most 4096 distinct nonempty names of at most 120 bytes"));
    }
    Ok(())
}

/// Return a stable path and reject non-regular and oversized disk files before opening them.
fn checked_file(s: &Session, path: &Path, limit: u64) -> Result<PathBuf> {
    let input = path.to_str().ok_or_else(|| invalid("result path must be UTF-8"))?;
    let checked = PathBuf::from(s.checked_path(input, Access::Read)?);
    #[cfg(not(target_arch = "wasm32"))]
    {
        let resolved = checked.canonicalize().map_err(|e| EngineError::Params(e.to_string()))?;
        let info = resolved.metadata().map_err(|e| EngineError::Params(e.to_string()))?;
        if !info.is_file() || info.len() == 0 || info.len() > limit {
            return Err(invalid("expected a nonempty regular result file within its size limit"));
        }
        Ok(resolved)
    }
    #[cfg(target_arch = "wasm32")]
    {
        let _ = limit; // The virtual filesystem applies the cap when opening the selected file.
        if !fieldcraft_io::fs::is_file(&checked) {
            return Err(invalid("result file is missing"));
        }
        Ok(checked)
    }
}

fn utf8(path: PathBuf) -> Result<String> {
    path.into_os_string().into_string().map_err(|_| invalid("result path must be UTF-8"))
}

/// `directory` is the canonical directory captured on registration, not a directory re-resolved
/// through a new link when the user clicks the row.
pub(crate) fn sibling(s: &Session, directory: &Path, name: &str, limit: u64) -> Result<String> {
    let resolved = checked_file(s, &directory.join(sibling_name(name)?), limit)?;
    if resolved.parent() != Some(directory) {
        return Err(invalid("external catalog entry resolves outside the manifest directory"));
    }
    utf8(resolved)
}

fn sibling_name(name: &str) -> Result<&str> {
    if !valid_label(name, 255) || matches!(name, "." | "..") || name.contains(['/', '\\', ':']) {
        return Err(invalid("an external catalog entry must name a single sibling file"));
    }
    Ok(name)
}

/// Compact archives keep the integral manifest without its optional nodal files. Only the
/// ordinary integral reader accepts such an archive. A dangling link is an existing entry.
fn absent_sibling(folder: &Path, name: &str) -> bool {
    let Ok(name) = sibling_name(name) else { return false };
    let path = folder.join(name);
    #[cfg(not(target_arch = "wasm32"))]
    {
        matches!(std::fs::symlink_metadata(path), Err(e) if e.kind() == std::io::ErrorKind::NotFound)
    }
    #[cfg(target_arch = "wasm32")]
    {
        !fieldcraft_io::fs::is_file(&path)
    }
}

fn rows(s: &Session, kind: Kind, folder: &Path, entries: Vec<Entry>, paths: &mut BTreeSet<String>, archived: bool) -> Result<Vec<CatalogField>> {
    let mut rows = Vec::new();
    for e in entries {
        // The underlying nodal field contract permits an empty unit for dimensionless data.
        if e.unit.len() > 48 || e.unit.chars().any(char::is_control) {
            return Err(invalid("external field units must be at most 48 bytes and contain no control characters"));
        }
        let path = if archived { utf8(folder.join(sibling_name(&e.file)?))? } else { sibling(s, folder, &e.file, kind.limit())? };
        if !paths.insert(path.clone()) {
            return Err(invalid("multiple catalog entries refer to the same file"));
        }
        if !archived {
            rows.push(CatalogField { name: e.name, unit: e.unit, path });
        }
    }
    Ok(rows)
}

impl Session {
    /// Cheap to inspect or share with a UI frame; an intervening model edit hides the catalog.
    pub fn external_catalog(&self) -> Option<&Arc<ExternalCatalog>> {
        self.result_catalog.as_ref().filter(|catalog| catalog.revision == self.revision())
    }

    /// The source of the currently displayed external field or series, including its original name.
    pub fn external_selection(&self) -> Option<&CatalogField> {
        self.external_contour()?;
        self.selected_result.as_ref()
    }

    pub(crate) fn read_external_catalog(&self, path: &Path, catalog_only: bool) -> Result<ExternalCatalog> {
        let path = checked_file(self, path, MANIFEST_BYTES)?;
        let bytes = file::read_file(path.to_str().ok_or_else(|| invalid("result path must be UTF-8"))?, MANIFEST_BYTES)?;
        let m: Manifest = serde_json::from_slice(&bytes).map_err(|e| EngineError::Params(format!("invalid external results: {e}")))?;
        if m.format != "fieldcraft-external-run/1" || m.mesh_sha256 != fieldcraft_io::sha256_hex(&fieldcraft_io::mesh::encode(&self.model)?) {
            return Err(invalid("expected fieldcraft-external-run/1 for the current mesh"));
        }
        if m.integrals.is_none() && m.fields.is_empty() && m.series.is_empty() {
            return Err(invalid("external result manifest has no fields, series or integrals"));
        }
        let integrals = m.integrals.unwrap_or_default();
        unique_names(integrals.len(), integrals.iter().map(|q| q.name.as_str()))?;
        for q in &integrals {
            if !valid_label(&q.unit, 48) {
                return Err(invalid("integrals need nonempty units of at most 48 bytes"));
            }
        }
        for list in [&m.materials, &m.boundaries, &m.interfaces, &m.loads] {
            unique_names(list.len(), list.iter().map(String::as_str))?;
        }
        let folder = path.parent().ok_or_else(|| invalid("missing manifest directory"))?;
        for entries in [&m.fields, &m.series] {
            unique_names(entries.len(), entries.iter().map(|e| e.name.as_str()))?;
        }
        let archived = !catalog_only && !integrals.is_empty() && m.fields.iter().chain(&m.series).all(|e| absent_sibling(folder, &e.file));
        let mut paths = BTreeSet::from([utf8(path.clone())?]);
        Ok(ExternalCatalog {
            revision: self.revision(),
            fields: rows(self, Kind::Field, folder, m.fields, &mut paths, archived)?,
            series: rows(self, Kind::Series, folder, m.series, &mut paths, archived)?,
            integrals,
            materials: m.materials,
            boundaries: m.boundaries,
            interfaces: m.interfaces,
            loads: m.loads,
        })
    }
}

pub(crate) struct Selection {
    pub(crate) path: String,
    expected: Option<CatalogField>,
    kind: Kind,
}

impl Selection {
    pub(crate) fn resolve(s: &Session, p: &Value, kind: Kind) -> Result<Self> {
        let rows = s.external_catalog().map_or(&[][..], |catalog| kind.rows(catalog));
        match (p.get("path"), opt::<&str>(p, "entry")?) {
            (Some(_), None) => {}
            (None, Some(name)) => {
                let row = rows.iter().find(|row| row.name == name).ok_or_else(|| invalid("unknown external result entry on this mesh"))?;
                return Self::from_row(s, kind, row);
            }
            _ => return Err(invalid("give one of `path` or `entry` to select an external result")),
        }
        let path = file_param(s, p, "path", Access::Read)?;
        if let Some(supplied) = p.get("path").and_then(Value::as_str)
            && let Some(row) = rows.iter().find(|row| row.path == supplied || row.path == path)
        {
            return Self::from_row(s, kind, row);
        }
        let path = utf8(checked_file(s, Path::new(&path), kind.limit())?)?;
        let expected = rows.iter().find(|row| row.path == path).cloned();
        Ok(Self { path, expected, kind })
    }

    fn from_row(s: &Session, kind: Kind, row: &CatalogField) -> Result<Self> {
        let path = Path::new(&row.path);
        let folder = path.parent().ok_or_else(|| invalid("missing result directory"))?;
        let file = path.file_name().and_then(|x| x.to_str()).ok_or_else(|| invalid("missing result file name"))?;
        let path = sibling(s, folder, file, kind.limit())?;
        Ok(Self { path, expected: Some(row.clone()), kind })
    }

    /// Registering a standalone file starts a one-row catalog. Selecting a row from the current
    /// run preserves all its other fields and regions. Only call after validation of all values.
    pub(crate) fn register(self, s: &mut Session, name: &str, unit: &str) -> Result<()> {
        if let Some(row) = self.expected {
            if row.name != name || row.unit != unit {
                return Err(invalid("selected result name or unit differs from its catalog entry"));
            }
            s.selected_result = Some(row);
            return Ok(());
        }
        let mut catalog = ExternalCatalog { revision: s.revision(), ..Default::default() };
        let row = CatalogField { name: name.to_owned(), unit: unit.to_owned(), path: self.path };
        s.selected_result = Some(row.clone());
        match self.kind {
            Kind::Field => catalog.fields.push(row),
            Kind::Series => catalog.series.push(row),
        }
        s.result_catalog = Some(Arc::new(catalog));
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::fs;

    fn fixture() -> (crate::tests::TempDir, Session, PathBuf, Value) {
        let (dir, seed) = crate::tests::design_file(b"{}");
        let manifest = Path::new(&seed).with_file_name("results.json");
        let mut s = Session::default();
        s.execute("mesh.node.add", &json!({"label":7,"xyz":[0,0,0]})).unwrap();
        s.execute("mesh.node.add", &json!({"label":9,"xyz":[0,1,0]})).unwrap();
        s.execute("mesh.element.add", &json!({"label":1,"kind":"rod2","nodes":[7,9]})).unwrap();
        let hash = fieldcraft_io::sha256_hex(&fieldcraft_io::mesh::encode(&s.model).unwrap());
        let mut field =
            json!({"format":"fieldcraft-nodal-field/1","mesh_sha256":hash,"name":"B","unit":"T","components":1,"node_ids":[7,9],"values":[2.,1.]});
        fs::write(manifest.with_file_name("field.json"), field.to_string()).unwrap();
        field["values"] = json!([3., 2.]);
        fs::write(manifest.with_file_name("last.json"), field.to_string()).unwrap();
        let series = json!({"format":"fieldcraft-nodal-series/1","mesh_sha256":hash,"name":"B","unit":"T","components":1,"time_unit":"s",
                  "frames":[{"time":0.,"file":"field.json"},{"time":1.,"file":"last.json"}]});
        fs::write(manifest.with_file_name("series.json"), series.to_string()).unwrap();
        let run = json!({"format":"fieldcraft-external-run/1","mesh_sha256":hash,
            "fields":[{"name":"B","unit":"T","file":"field.json"}],
            "series":[{"name":"B","unit":"T","file":"series.json"}],
            "materials":["air"],"boundaries":["wall"],"interfaces":["gap"],"loads":["coil current"],
            "integrals":[{"name":"force","unit":"N","value":[0.,1.,2.]}]});
        fs::write(&manifest, run.to_string()).unwrap();
        (dir, s, manifest, run)
    }

    #[test]
    fn selecting_an_external_result_requires_exactly_one_source() {
        let mut session = Session::default();
        for command in ["results.external.contour", "results.external.series"] {
            for params in [json!({}), json!({"path":"field.json","entry":"B"})] {
                let err = session.execute(command, &params).unwrap_err().to_string();
                assert!(err.contains("`path`") && err.contains("`entry`"), "{command}: {err}");
            }
        }
    }

    #[test]
    fn catalog_rows_select_real_fields_and_series_without_losing_groups() {
        let (_dir, mut s, path, data) = fixture();
        let reply = s.execute("results.external.integrals", &json!({"path":path})).unwrap();
        assert_eq!(reply, json!({"integrals":data["integrals"]}));
        let catalog = Arc::clone(s.external_catalog().unwrap());
        assert_eq!(catalog.fields[0].name, "B");
        assert_eq!(catalog.series[0].unit, "T");
        assert_eq!(catalog.materials, ["air"]);
        assert_eq!(catalog.boundaries, ["wall"]);
        assert_eq!(catalog.interfaces, ["gap"]);
        assert_eq!(catalog.loads, ["coil current"]);
        assert_eq!(s.execute("results.external.series", &json!({"entry":"B"})).unwrap()["frames"], 2);
        assert_eq!(s.external_selection().unwrap().path, catalog.series[0].path);
        assert_eq!(s.execute("results.external.playback", &json!({"frame":1})).unwrap()["time"], 1.);
        assert_eq!(s.execute("results.external.contour", &json!({"entry":"B"})).unwrap()["max"], json!([7, 2.]));
        assert_eq!(s.external_selection().unwrap().path, catalog.fields[0].path);
        assert!(Arc::ptr_eq(&catalog, s.external_catalog().unwrap()));
        s.execute("results.external.contour", &json!({"show":false})).unwrap();
        assert!(s.external_contour().is_none());
        assert!(s.external_selection().is_none());
        assert!(Arc::ptr_eq(&catalog, s.external_catalog().unwrap()));
        s.execute("mesh.node.add", &json!({"label":8,"xyz":[1,0,0]})).unwrap();
        assert!(s.external_catalog().is_none());
        assert!(s.execute("results.external.series", &json!({"entry":"B"})).is_err());
    }

    #[test]
    fn standalone_field_and_series_and_legacy_integrals_remain_usable() {
        let (_dir, mut s, path, mut run) = fixture();
        s.execute("results.external.contour", &json!({"path":path.with_file_name("field.json")})).unwrap();
        assert_eq!(s.external_catalog().unwrap().fields[0].name, "B");
        s.execute("results.external.series", &json!({"path":path.with_file_name("series.json")})).unwrap();
        assert_eq!(s.external_catalog().unwrap().series[0].name, "B");
        let legacy = json!({"format":run["format"],"mesh_sha256":run["mesh_sha256"],"integrals":run["integrals"]});
        fs::write(&path, legacy.to_string()).unwrap();
        assert_eq!(s.execute("results.external.integrals", &json!({"path":path})).unwrap()["integrals"], legacy["integrals"]);
        run.as_object_mut().unwrap().remove("integrals");
        fs::write(&path, run.to_string()).unwrap();
        assert_eq!(
            s.execute("results.external.integrals", &json!({"path":path,"catalog_only":true})).unwrap(),
            json!({"registered":true,"fields":1,"series":1,"integrals":0})
        );
    }

    #[test]
    fn integral_archive_without_any_field_files_still_returns_signed_quantities() {
        let (_dir, mut s, path, mut run) = fixture();
        fs::remove_file(path.with_file_name("field.json")).unwrap();
        fs::remove_file(path.with_file_name("series.json")).unwrap();
        let q = json!({"name":"signed work","unit":"J","value":-2.5});
        run["integrals"].as_array_mut().unwrap().push(q);
        fs::write(&path, run.to_string()).unwrap();
        assert_eq!(s.execute("results.external.integrals", &json!({"path":path})).unwrap()["integrals"], run["integrals"]);
        let catalog = Arc::clone(s.external_catalog().unwrap());
        assert!(catalog.fields.is_empty() && catalog.series.is_empty());
        assert!(s.execute("results.external.integrals", &json!({"path":path, "catalog_only":true})).is_err());
        run["mesh_sha256"] = json!("different mesh");
        fs::write(&path, run.to_string()).unwrap();
        assert!(s.execute("results.external.integrals", &json!({"path":path})).is_err());
        assert!(Arc::ptr_eq(&catalog, s.external_catalog().unwrap()));
    }

    #[test]
    fn dimensionless_fields_and_series_match_the_standalone_format() {
        let (_dir, mut s, path, mut run) = fixture();
        for file in ["field.json", "last.json", "series.json"] {
            let source = path.with_file_name(file);
            let mut data: Value = serde_json::from_slice(&fs::read(&source).unwrap()).unwrap();
            data["unit"] = json!("");
            fs::write(source, data.to_string()).unwrap();
        }
        run["fields"][0]["unit"] = json!("");
        run["series"][0]["unit"] = json!("");
        fs::write(&path, run.to_string()).unwrap();
        s.execute("results.external.integrals", &json!({"path":path,"catalog_only":true})).unwrap();
        assert_eq!(s.execute("results.external.contour", &json!({"entry":"B"})).unwrap()["field"], "B");
        assert_eq!(s.execute("results.external.series", &json!({"entry":"B"})).unwrap()["frames"], 2);
        run["integrals"][0]["unit"] = json!("");
        fs::write(&path, run.to_string()).unwrap();
        assert!(s.execute("results.external.integrals", &json!({"path":path})).is_err());
    }

    #[test]
    fn invalid_manifests_and_replaced_files_preserve_the_previous_catalog_and_contour() {
        let (_dir, mut s, path, run) = fixture();
        s.execute("results.external.integrals", &json!({"path":path})).unwrap();
        s.execute("results.external.contour", &json!({"entry":"B"})).unwrap();
        let catalog = Arc::clone(s.external_catalog().unwrap());
        let display = s.external_contour().unwrap().display_id;
        for (key, bad) in [
            ("fields", json!([{"name":"B","unit":"T","file":"../field.json"}])),
            ("series", json!([{"name":"B","unit":"T","file":"absent.json"}])),
            ("fields", json!([{"name":"B","unit":"T","file":"series.json"}])),
            ("fields", json!([{"name":"B","unit":"T","file":"results.json"}])),
            ("integrals", json!([{"name":"bad","unit":"N","value":[1,2]}])),
            ("integrals", json!([run["integrals"][0], run["integrals"][0]])),
            ("mesh_sha256", json!("bad")),
            ("loads", json!(["x", "x"])),
            ("loads", json!([""])),
            ("materials", json!(vec!["x"; MAX_ROWS + 1])),
        ] {
            let mut bad_run = run.clone();
            bad_run[key] = bad;
            fs::write(&path, bad_run.to_string()).unwrap();
            assert!(s.execute("results.external.integrals", &json!({"path":path})).is_err(), "{key}");
            assert!(Arc::ptr_eq(&catalog, s.external_catalog().unwrap()));
            assert_eq!(s.external_contour().unwrap().display_id, display);
        }
        let field_path = path.with_file_name("field.json");
        let mut changed: Value = serde_json::from_slice(&fs::read(&field_path).unwrap()).unwrap();
        changed["unit"] = json!("mT");
        fs::write(&field_path, changed.to_string()).unwrap();
        for params in [json!({"entry":"B"}), json!({"path":field_path}), json!({"path":field_path,"entry":"B"})] {
            assert!(s.execute("results.external.contour", &params).is_err());
            assert_eq!(s.external_contour().unwrap().display_id, display);
        }
    }

    #[test]
    fn changed_roots_empty_files_and_files_exceeding_the_limit_are_refused_before_activation() {
        let (_dir, mut s, path, _) = fixture();
        s.execute("results.external.integrals", &json!({"path":path})).unwrap();
        let catalog = Arc::clone(s.external_catalog().unwrap());
        let (_other_dir, other) = crate::tests::design_file(b"{}");
        s.file_access = fieldcraft_io::roots::Roots::new(Path::new(&other).parent(), None).unwrap();
        assert!(s.execute("results.external.contour", &json!({"entry":"B"})).is_err());
        s.file_access = Default::default();
        let file = fs::File::create(path.with_file_name("series.json")).unwrap();
        for size in [0, Kind::Series.limit() + 1] {
            file.set_len(size).unwrap();
            assert!(s.execute("results.external.series", &json!({"entry":"B"})).is_err());
            assert!(s.execute("results.external.integrals", &json!({"path":path})).is_err());
            assert!(Arc::ptr_eq(&catalog, s.external_catalog().unwrap()));
            assert!(s.external_selection().is_none());
        }
    }

    #[cfg(unix)]
    #[test]
    fn replacement_link_outside_the_manifest_folder_is_refused_even_without_a_read_root() {
        use std::os::unix::fs::symlink;
        let (_dir, mut s, path, _) = fixture();
        s.execute("results.external.integrals", &json!({"path":path})).unwrap();
        let field = s.external_catalog().unwrap().fields[0].path.clone();
        let (_elsewhere, target) = crate::tests::design_file(&fs::read(&field).unwrap());
        fs::remove_file(&field).unwrap();
        symlink(&target, &field).unwrap();
        assert!(s.execute("results.external.contour", &json!({"entry":"B"})).is_err());
        assert!(s.execute("results.external.contour", &json!({"path":field})).is_err());
        assert!(s.execute("results.external.integrals", &json!({"path":path})).is_err());
        assert!(s.external_contour().is_none());
    }
}
