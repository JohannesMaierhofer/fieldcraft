//! FieldCraft owns the saved analysis, mesh exchange, and result publication. The solver runs in CPython.

mod exchange;
mod python;

pub use python::with_helpers;

use std::collections::BTreeSet;
use std::fs::{self, File};
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use fieldcraft_io::fs::{read_capped, write_atomic};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::{EngineError, Result, Session};

const MAX_MANIFEST: u64 = 4 * 1024 * 1024;
const MAX_INPUT_FILE: u64 = 512 * 1024 * 1024;
static NEXT_STAGE: AtomicU64 = AtomicU64::new(0);
/// Both desktop and headless analyses use the same solver time limit.
pub const SOLVER_TIMEOUT: Duration = Duration::from_secs(3600);

pub const ANALYSIS_FORMAT: &str = "fieldcraft-ngsolve-analysis/1";

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Analysis {
    format: String,
    model: PathBuf,
    script: PathBuf,
    #[serde(default = "volume_dimension")]
    dimension: u8,
    materials: Vec<String>,
    boundaries: Vec<String>,
    #[serde(default)]
    interfaces: Vec<String>,
    pub field: Option<String>,
    pub series: Option<String>,
    pub component: Option<String>,
}

fn volume_dimension() -> u8 {
    3
}

fn invalid(message: impl Into<String>) -> EngineError {
    EngineError::Params(message.into())
}

fn io_error(path: &Path, error: std::io::Error) -> EngineError {
    invalid(format!("{}: {error}", path.display()))
}

fn path_text(path: &Path) -> Result<&str> {
    path.to_str().ok_or_else(|| invalid(format!("{} is not a UTF-8 path", path.display())))
}

fn save_new(path: &Path, data: &impl Serialize) -> Result<()> {
    let mut writer = BufWriter::new(File::create_new(path).map_err(|error| io_error(path, error))?);
    serde_json::to_writer(&mut writer, data).map_err(|error| invalid(format!("{}: {error}", path.display())))?;
    writer.flush().map_err(|error| io_error(path, error))
}

fn read_json(path: &Path, limit: u64) -> Result<Value> {
    serde_json::from_slice(&read_capped(path, limit)?).map_err(|error| invalid(format!("{}: {error}", path.display())))
}

impl Analysis {
    /// Resolve paths relative to their original analysis file. The saved model is opened only when used.
    pub fn read(path: &Path) -> Result<Self> {
        if !is_analysis_file(path) {
            return Err(invalid("Choose an NGSolve .analysis.json file"));
        }
        let source = path.canonicalize().map_err(|error| io_error(path, error))?;
        let root = source.parent().ok_or_else(|| invalid("The analysis needs a parent folder"))?;
        let mut analysis: Self =
            serde_json::from_slice(&read_capped(&source, 64 * 1024)?).map_err(|error| invalid(format!("{}: {error}", source.display())))?;
        if analysis.format != ANALYSIS_FORMAT || !matches!(analysis.dimension, 2 | 3) {
            return Err(invalid(format!("Expected {ANALYSIS_FORMAT} with dimension 2 or 3")));
        }
        if path_text(&analysis.script)?.trim().is_empty() {
            return Err(invalid("The analysis needs a Python script path"));
        }
        if analysis.field.iter().chain(&analysis.series).any(|s| s.trim().is_empty())
            || (analysis.field.is_some() && analysis.series.is_some())
            || analysis.component.as_deref().is_some_and(|s| !matches!(s, "x" | "y" | "z" | "magnitude"))
        {
            return Err(invalid("Choose one result field or series and a valid component"));
        }
        if analysis.materials.is_empty() || analysis.boundaries.is_empty() || (analysis.dimension == 2 && !analysis.interfaces.is_empty()) {
            return Err(invalid("The analysis needs material and boundary sets; interior interfaces require a 3D mesh"));
        }
        for names in [&analysis.materials, &analysis.boundaries, &analysis.interfaces] {
            if names.len() > 4096
                || names.iter().any(|name| name.trim().is_empty() || name.len() > 120 || name.chars().any(char::is_control))
                || names.iter().collect::<BTreeSet<_>>().len() != names.len()
            {
                return Err(invalid("Analysis groups need distinct names of up to 120 bytes"));
            }
        }
        if analysis.interfaces.iter().any(|name| analysis.boundaries.contains(name)) {
            return Err(invalid("Exterior and interior group names must be distinct"));
        }
        if !path_text(&analysis.model)?.trim().is_empty() {
            analysis.model = root.join(analysis.model);
        }
        let script = root.join(&analysis.script);
        analysis.script = script.canonicalize().map_err(|error| io_error(&script, error))?;
        Ok(analysis)
    }

    /// Use the saved model, or a copy of the session and a path for its snapshot.
    pub fn prepare(mut self, destination: &Path, snapshot: Option<(Session, &Path)>) -> Result<Prepared> {
        let stage = Stage::new(destination)?;
        let session = if let Some((mut session, snapshot)) = snapshot {
            session.execute("file.save_as", &json!({"path":path_text(snapshot)?,"results":false,"geometry":"refer"}))?;
            self.model = snapshot.to_path_buf();
            session
        } else {
            if path_text(&self.model)?.trim().is_empty() {
                return Err(invalid("Enter a saved model path or choose Use Open Model"));
            }
            self.model = self.model.canonicalize().map_err(|error| io_error(&self.model, error))?;
            let mut session = Session::default();
            session.execute("file.open", &json!({"path":path_text(&self.model)?}))?;
            session
        };
        let mesh_sha256 = fieldcraft_io::sha256_hex(&fieldcraft_io::mesh::encode(&session.model)?);
        let exchange = exchange::export(&session.model, &self, &mesh_sha256)?;
        save_new(&stage.directory.join("mesh.json"), &exchange)?;
        let script_sha256 = fieldcraft_io::sha256_hex(&read_capped(&self.script, 16 * 1024 * 1024)?);
        let model_sha256 = fieldcraft_io::sha256_hex(&read_capped(&self.model, MAX_INPUT_FILE)?);
        Ok(Prepared { analysis: self, stage, session, mesh_sha256, script_sha256, model_sha256 })
    }
}

pub fn is_analysis_file(path: &Path) -> bool {
    path.file_name().and_then(|name| name.to_str()).is_some_and(|name| name.ends_with(".analysis.json"))
}

struct Stage {
    directory: PathBuf,
    destination: PathBuf,
}

impl Stage {
    fn new(path: &Path) -> Result<Self> {
        let absolute = std::path::absolute(path).map_err(|error| io_error(path, error))?;
        let parent = absolute.parent().ok_or_else(|| invalid("The run needs an output parent folder"))?;
        fs::create_dir_all(parent).map_err(|error| io_error(parent, error))?;
        let parent = parent.canonicalize().map_err(|error| io_error(parent, error))?;
        let destination = parent.join(absolute.file_name().ok_or_else(|| invalid("The run needs a new output directory"))?);
        unused_destination(&destination)?;
        for _ in 0..32 {
            let id = NEXT_STAGE.fetch_add(1, Ordering::Relaxed);
            let directory = parent.join(format!(".fieldcraft-ngsolve-{}-{id}", std::process::id()));
            match fs::create_dir(&directory) {
                Ok(()) => return Ok(Self { directory, destination }),
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
                Err(error) => return Err(io_error(&directory, error)),
            }
        }
        Err(invalid("Could not reserve a solver staging directory"))
    }
}

impl Drop for Stage {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.directory);
    }
}

fn unused_destination(path: &Path) -> Result<()> {
    match fs::symlink_metadata(path) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(io_error(path, error)),
        Ok(_) => Err(invalid(format!("Choose a new output directory: {}", path.display()))),
    }
}

/// Start Python with the selected environment; keep the venv executable's symlink intact.
pub fn python_command(python: &Path, directory: &Path) -> Result<Command> {
    if !python.is_absolute() || !python.is_file() {
        return Err(invalid("Select the installed FieldCraft Python interpreter in Settings → Python or pass --python"));
    }
    let mut command = Command::new(python);
    command
        .current_dir(directory)
        .env_remove("PYTHONHOME")
        .env_remove("PYTHONPATH")
        .env_remove("VIRTUAL_ENV")
        .env("PYTHONNOUSERSITE", "1")
        .env("PYTHONUNBUFFERED", "1")
        .env("PYTHONUTF8", "1")
        .arg("-s");
    Ok(command)
}

pub struct Prepared {
    analysis: Analysis,
    stage: Stage,
    session: Session,
    mesh_sha256: String,
    script_sha256: String,
    model_sha256: String,
}

impl Prepared {
    pub fn command(&self, python: &Path) -> Result<Command> {
        let mut command = python_command(python, &self.stage.directory)?;
        command.args(["-u", "-c", &with_helpers(python::ANALYSIS)]).arg(&self.analysis.script).arg(&self.stage.directory);
        Ok(command)
    }

    /// Check all returned fields and time steps against the exact mesh used to start the solver.
    pub fn verify(mut self, python: &Path) -> Result<Verified> {
        let folder = &self.stage.directory;
        let path = folder.join("results.json");
        let mut manifest = read_json(&path, MAX_MANIFEST)?;
        let output = manifest.as_object_mut().ok_or_else(|| invalid("Solver results must be a JSON object"))?;
        if output.get("mesh_sha256").and_then(Value::as_str) != Some(self.mesh_sha256.as_str()) {
            return Err(invalid("Solver results belong to a different mesh"));
        }
        let mut runtime = read_json(&folder.join("runtime.json"), 64 * 1024)?;
        let platform = runtime.as_object_mut().and_then(|record| record.remove("platform")).unwrap_or(Value::Null);
        let archive = read_capped(&folder.join("mesh.npz"), MAX_INPUT_FILE)?;
        let provenance = [
            ("format", json!("fieldcraft-external-run/1")),
            ("model", json!(self.analysis.model)),
            ("script", json!(self.analysis.script)),
            ("interpreter", json!(python)),
            ("model_sha256", json!(self.model_sha256)),
            ("script_sha256", json!(self.script_sha256)),
            ("exchange_sha256", json!(fieldcraft_io::sha256_hex(&archive))),
            ("runtime", runtime),
            ("platform", platform),
            ("dimension", json!(self.analysis.dimension)),
            ("materials", json!(self.analysis.materials)),
            ("boundaries", json!(self.analysis.boundaries)),
            ("interfaces", json!(self.analysis.interfaces)),
        ];
        output.extend(provenance.into_iter().map(|(key, value)| (key.to_string(), value)));
        let bytes = serde_json::to_vec_pretty(&manifest).map_err(|error| invalid(error.to_string()))?;
        if bytes.len() as u64 > MAX_MANIFEST {
            return Err(invalid("Solver result manifest exceeds its size limit"));
        }
        write_atomic(&path, &bytes, false)?;
        let catalog = self.session.read_external_catalog(&path, true)?;
        if catalog.fields.is_empty() && catalog.integrals.is_empty() && catalog.series.is_empty() {
            return Err(invalid("The solver returned no fields, time series or integrals"));
        }
        if let Some(field) = &self.analysis.field
            && !catalog.fields.iter().any(|entry| &entry.name == field)
        {
            return Err(invalid(format!("The solver did not return the requested field {field:?}")));
        }
        if let Some(series) = &self.analysis.series
            && !catalog.series.iter().any(|entry| &entry.name == series)
        {
            return Err(invalid(format!("The solver did not return the requested series {series:?}")));
        }
        let catalog = Arc::new(catalog);
        self.session.result_catalog = Some(catalog.clone());
        for (command, prefix, entries) in
            [("results.external.contour", "view", &catalog.fields), ("results.external.series", "view-series", &catalog.series)]
        {
            for (index, entry) in entries.iter().enumerate() {
                let selected = if prefix == "view" { self.analysis.field.as_deref() } else { self.analysis.series.as_deref() };
                let mut params = json!({"entry":entry.name});
                if selected == Some(entry.name.as_str())
                    && let Some(component) = &self.analysis.component
                {
                    params["component"] = json!(component);
                }
                self.session.execute(command, &params)?;
                self.session.field_series = None;
                self.session.external_field = None;
                let name = Path::new(&entry.path).file_name().ok_or_else(|| invalid("A result file needs a name"))?;
                params.as_object_mut().ok_or_else(|| invalid("Invalid result parameters"))?.remove("entry");
                params["path"] = json!(self.stage.destination.join(name));
                save_new(
                    &folder.join(format!("{prefix}-{}.json", index + 1)),
                    &[json!({"command":"file.open","params":{"path":self.analysis.model}}), json!({"command":command,"params":params})],
                )?;
            }
        }
        save_new(
            &folder.join("view-run.json"),
            &[
                json!({"command":"file.open","params":{"path":self.analysis.model}}),
                json!({"command":"results.external.integrals","params":{"path":self.stage.destination.join("results.json"),"catalog_only":true}}),
            ],
        )?;
        for name in ["mesh.json", "runtime.json"] {
            let path = folder.join(name);
            fs::remove_file(&path).map_err(|error| io_error(&path, error))?;
        }
        Ok(Verified(self.stage))
    }
}

/// Only a verified solver run can be published. Dropping it cleans up the uncommitted output.
pub struct Verified(Stage);

impl Verified {
    pub fn publish(self) -> Result<PathBuf> {
        unused_destination(&self.0.destination)?;
        fs::rename(&self.0.directory, &self.0.destination).map_err(|error| io_error(&self.0.destination, error))?;
        Ok(self.0.destination.join("results.json"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn analysis_requires_its_new_file_extension_and_format() {
        let legacy = Path::new("example.case.json");
        assert!(!is_analysis_file(legacy));
        assert!(Analysis::read(legacy).err().unwrap().to_string().contains(".analysis.json"));
        let root = std::env::temp_dir();
        let stage = Stage::new(&root.join(format!("fc-analysis-test-{}", std::process::id()))).unwrap();
        let definition = stage.directory.join("example.analysis.json");
        save_new(&definition, &json!({"format":"fieldcraft-ngsolve-case/1","model":"","script":"solve.py","materials":[],"boundaries":[]})).unwrap();
        assert!(Analysis::read(&definition).err().unwrap().to_string().contains(ANALYSIS_FORMAT));
    }
}
