//! The desktop Python workspace. FieldCraft owns the document and imports validated results.

mod panel;
mod process;
mod runtime;

use std::collections::VecDeque;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::TryRecvError;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use fieldcraft_engine::{Session, ngsolve};
use serde_json::json;

use crate::{ModelApp, inbox};
use process::Process;
use runtime::Runtime;

const MAX_FILE: u64 = 1024 * 1024;
const MAX_TABS: usize = 24;
const MAX_LOG_LINES: usize = 400;
const ENVIRONMENT_JOB: &str = "Python environment sync";
static RUN_ID: AtomicU64 = AtomicU64::new(1);

#[derive(Default)]
pub(crate) struct Workspace {
    open: bool,
    detached: bool,
    dock_width: Option<f32>,
    project: Option<PathBuf>,
    folder: String,
    runtime: Option<Runtime>,
    tabs: Vec<Document>,
    active: Option<PathBuf>,
    analysis: Option<PathBuf>,
    last_result: Option<PathBuf>,
    use_open_model: bool,
    new_path: String,
    package: String,
    environment_status: Option<Result<String, String>>,
    console: Option<Console>,
    console_input: String,
    job: Option<Job>,
    log: VecDeque<String>,
    error: Option<String>,
    selection: Option<Selection>,
    jump_line: Option<usize>,
}

struct Document {
    path: PathBuf,
    saved: String,
    text: String,
}

impl Document {
    fn dirty(&self) -> bool {
        self.saved != self.text
    }
}

struct Selection {
    start: usize,
    end: usize,
    caret: usize,
}

struct Console {
    process: Process,
    completion: String,
    busy: bool,
}

struct Launch {
    command: std::process::Command,
    log: PathBuf,
    analysis: Option<Box<AnalysisJob>>,
    name: &'static str,
}

struct AnalysisJob {
    prepared: ngsolve::Prepared,
    interpreter: PathBuf,
    view: PreferredView,
}

struct RunResult {
    path: PathBuf,
    view: PreferredView,
}

#[derive(Default)]
struct PreferredView {
    field: Option<String>,
    series: Option<String>,
    component: Option<String>,
}

enum Job {
    Preparing(inbox::Inbox<Result<Launch, String>>),
    Running { process: Process, analysis: Option<Box<AnalysisJob>>, name: &'static str, started: std::time::Instant },
    Verifying(inbox::Inbox<Result<(ngsolve::Verified, PreferredView), String>>),
}

fn read_text(path: &Path) -> Result<String, String> {
    let bytes = fieldcraft_io::fs::read_capped(path, MAX_FILE).map_err(|error| error.to_string())?;
    String::from_utf8(bytes).map_err(|_| format!("{} is not a UTF-8 text file", path.display()))
}

fn in_project(root: &Path, path: &Path) -> Result<PathBuf, String> {
    let absolute = root.join(path).canonicalize().map_err(|error| format!("{}: {error}", path.display()))?;
    if !absolute.starts_with(root) {
        return Err(format!("{} is outside the project folder", absolute.display()));
    }
    Ok(absolute)
}

fn path_string(path: &Path) -> Result<&str, String> {
    path.to_str().ok_or_else(|| format!("{} is not a UTF-8 path", path.display()))
}

fn run_folder(root: &Path, kind: &str) -> Result<PathBuf, String> {
    let dir = root.join("runs");
    fs::create_dir_all(&dir).map_err(|error| format!("{}: {error}", dir.display()))?;
    let dir = in_project(root, &dir)?;
    let time = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_millis();
    let id = RUN_ID.fetch_add(1, Ordering::Relaxed);
    let path = dir.join(format!("{kind}-{time}-{}-{id}", std::process::id()));
    fs::create_dir(&path).map_err(|error| format!("{}: {error}", path.display()))?;
    Ok(path)
}

fn append(log: &mut VecDeque<String>, line: String) {
    if log.len() >= MAX_LOG_LINES {
        log.pop_front();
    }
    log.push_back(line);
}

impl Workspace {
    fn prepare(&mut self, model: Option<&str>, python: &str) {
        self.runtime.get_or_insert_with(|| Runtime::with_override(python));
        if self.project.is_none()
            && let Some(parent) = model.and_then(|path| Path::new(path).parent()).filter(|path| path.is_absolute())
        {
            let _ = self.set_project(parent);
        }
    }

    pub(crate) fn set_project(&mut self, path: &Path) -> Result<(), String> {
        let project = path.canonicalize().map_err(|error| format!("{}: {error}", path.display()))?;
        if !project.is_dir() {
            return Err("Choose an existing project folder".into());
        }
        let folder = path_string(&project)?.to_string();
        if self.project.as_ref() != Some(&project) {
            if self.job.is_some() || self.tabs.iter().any(Document::dirty) {
                return Err("Stop the current job and save or close edited files before changing projects".into());
            }
            self.console = None;
            self.tabs.clear();
            self.active = None;
            self.selection = None;
            self.analysis = None;
            self.last_result = None;
            self.use_open_model = false;
            self.environment_status = None;
            self.project = Some(project);
        }
        self.folder = folder;
        Ok(())
    }

    pub(crate) fn inspect(&self) -> serde_json::Value {
        json!({
            "open":self.open,
            "detached":self.detached,
            "project":self.project,
            "active":self.active,
            "editor_dirty":self.tabs.iter().any(Document::dirty),
            "job_running":self.job.is_some(),
            "console_running":self.console.is_some(),
            "console_busy":self.console.as_ref().is_some_and(|console| console.busy),
            "last_result":self.last_result,
            "error":self.error,
        })
    }

    pub(crate) fn set_detached(&mut self, detached: bool) {
        self.detached = detached;
    }

    fn root(&self) -> Result<&Path, String> {
        self.project.as_deref().ok_or_else(|| "Open a project folder first".to_string())
    }

    fn runtime(&self) -> Result<&Runtime, String> {
        self.runtime.as_ref().ok_or_else(|| "Open the Python Workspace first".to_string())
    }

    fn open_file(&mut self, path: &Path) -> Result<(), String> {
        let path = in_project(self.root()?, path)?;
        if is_model_file(&path) {
            return Err("Open FieldCraft models from the File menu".into());
        }
        if let Some(tab) = self.tabs.iter_mut().find(|tab| tab.path == path) {
            if !tab.dirty() {
                tab.saved = read_text(&path)?;
                tab.text.clone_from(&tab.saved);
            }
        } else {
            if self.tabs.len() >= MAX_TABS {
                return Err("Close an editor tab before opening another file".into());
            }
            let text = read_text(&path)?;
            self.tabs.push(Document { path: path.clone(), saved: text.clone(), text });
        }
        self.activate(path);
        Ok(())
    }

    fn activate(&mut self, path: PathBuf) {
        if ngsolve::is_analysis_file(&path) {
            self.analysis = Some(path.clone());
        }
        self.selection = None;
        self.active = Some(path);
    }

    fn active_tab(&self) -> Option<usize> {
        let path = self.active.as_ref()?;
        self.tabs.iter().position(|tab| &tab.path == path)
    }

    fn save(&mut self, index: usize) -> Result<(), String> {
        let tab = self.tabs.get(index).ok_or("No file selected")?;
        if !tab.dirty() {
            return Ok(());
        }
        if tab.text.len() as u64 > MAX_FILE {
            return Err("Editor files are limited to 1 MiB".into());
        }
        let path = in_project(self.root()?, &tab.path)?;
        if path != tab.path || read_text(&path)? != tab.saved {
            return Err("This file changed on disk. Save your changes to a new file or reopen the file".into());
        }
        fieldcraft_io::fs::write_atomic(&path, tab.text.as_bytes(), false).map_err(|error| error.to_string())?;
        if let Some(tab) = self.tabs.get_mut(index) {
            tab.saved.clone_from(&tab.text);
        }
        Ok(())
    }

    fn save_all(&mut self) -> Result<(), String> {
        for index in 0..self.tabs.len() {
            self.save(index)?;
        }
        Ok(())
    }

    fn create(&mut self, folder: bool) -> Result<(), String> {
        let root = self.root()?;
        let relative = Path::new(self.new_path.trim());
        if relative.as_os_str().is_empty() || relative.components().any(|component| !matches!(component, Component::Normal(_))) {
            return Err("Enter a path relative to the project folder".into());
        }
        let parent = in_project(root, relative.parent().ok_or("Missing parent folder")?)?;
        let path = parent.join(relative.file_name().ok_or("Enter a new file or folder name")?);
        if folder {
            fs::create_dir(path).map_err(|error| error.to_string())?;
        } else {
            if is_model_file(&path) {
                return Err("Use FieldCraft to create model files".into());
            }
            OpenOptions::new().write(true).create_new(true).open(&path).map_err(|error| error.to_string())?;
            self.open_file(&path)?;
        }
        self.new_path.clear();
        Ok(())
    }

    fn new_analysis(&mut self, model: Option<&Path>, dimension: u8) -> Result<(), String> {
        if self.tabs.len() >= MAX_TABS {
            return Err("Close an editor tab before opening another file".into());
        }
        let root = self.root()?;
        let model = model
            .filter(|path| is_model_file(path))
            .and_then(|path| path.canonicalize().ok())
            .and_then(|path| path.strip_prefix(root).ok().map(Path::to_path_buf))
            .unwrap_or_default();
        let script = self
            .active
            .as_deref()
            .filter(|path| path.extension().is_some_and(|ext| ext.eq_ignore_ascii_case("py")))
            .and_then(|path| path.strip_prefix(root).ok())
            .unwrap_or(Path::new(""));
        let stem: String = model.file_stem().and_then(|name| name.to_str()).unwrap_or("new").chars().take(40).collect();
        let mut template = serde_json::to_string_pretty(&json!({
            "format": ngsolve::ANALYSIS_FORMAT,
            "model": model,
            "script": script,
            "dimension": dimension,
            "materials": [],
            "boundaries": [],
            "interfaces": [],
            "field": null,
            "series": null,
            "component": null,
        }))
        .map_err(|error| error.to_string())?;
        template.push('\n');
        for index in 1..=100 {
            let name = if index == 1 { format!("{stem}.analysis.json") } else { format!("{stem}-{index}.analysis.json") };
            let path = root.join(name);
            match OpenOptions::new().write(true).create_new(true).open(&path) {
                Ok(mut file) => {
                    if let Err(error) = file.write_all(template.as_bytes()) {
                        drop(file);
                        let _ = fs::remove_file(&path);
                        return Err(error.to_string());
                    }
                    return self.open_file(&path);
                }
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
                Err(error) => return Err(error.to_string()),
            }
        }
        Err("Could not find an unused analysis file name".into())
    }

    fn console_request(&mut self, code: &str, filename: &str, line: usize, document: Option<&str>) -> Result<(), String> {
        if code.trim().is_empty() || code.len() as u64 > MAX_FILE {
            return Err("Select up to 1 MiB of Python code or enter a console command".into());
        }
        let preview: String = code.lines().next().unwrap_or_default().chars().take(240).collect();
        self.send_console_request(json!({"code":code,"file":filename,"line":line,"document":document}), &preview)
    }

    fn send_console_request(&mut self, request: serde_json::Value, preview: &str) -> Result<(), String> {
        if self.console.as_ref().is_some_and(|console| console.busy) {
            return Err("The Python console is running code".into());
        }
        if self.console.is_none() {
            let root = self.root()?;
            let mut command = self.runtime()?.command(root, false)?;
            let dir = run_folder(root, "console")?;
            let completion = format!("fieldcraft-console-complete-{}", RUN_ID.fetch_add(1, Ordering::Relaxed));
            command.args(["-u", "-c", &ngsolve::with_helpers(runtime::CONSOLE), &completion]);
            let process = Process::spawn(&mut command, &dir.join("console.log"), true).map_err(|error| error.to_string())?;
            self.console = Some(Console { process, completion, busy: false });
        }
        let console = self.console.as_mut().ok_or("Could not start the Python console")?;
        console.process.send(serde_json::to_vec(&request).map_err(|error| error.to_string())?)?;
        console.busy = true;
        append(&mut self.log, format!(">>> {preview}"));
        Ok(())
    }

    fn run_selection(&mut self) -> Result<(), String> {
        let tab = self.active_tab().and_then(|index| self.tabs.get(index)).ok_or("Open a Python file first")?;
        if tab.path.extension().and_then(|ext| ext.to_str()) != Some("py") {
            return Err("Run Selection needs a Python file".into());
        }
        let selection = self.selection.as_ref().ok_or("Select Python code in the editor")?;
        let (code, line) = selected(&tab.text, selection).ok_or("Select Python code in the editor")?;
        let file = path_string(&tab.path)?.to_string();
        let source = tab.text.clone();
        self.console_request(&code, &file, line, Some(&source))
    }

    fn run_section(&mut self) -> Result<(), String> {
        let tab = self.active_tab().and_then(|index| self.tabs.get(index)).ok_or("Open a Python file first")?;
        if tab.path.extension().and_then(|ext| ext.to_str()) != Some("py") || tab.text.len() as u64 > MAX_FILE {
            return Err("Run Section needs a Python file up to 1 MiB".into());
        }
        let caret = self.selection.as_ref().map_or(0, |selection| selection.caret);
        let line = tab.text.chars().take(caret).filter(|ch| *ch == '\n').count() + 1;
        let file = path_string(&tab.path)?;
        let request = json!({"section":line,"file":file,"document":tab.text});
        self.send_console_request(request, &format!("Run section at {}:{line}", tab.path.file_name().unwrap_or_default().to_string_lossy()))
    }

    fn idle(&self) -> Result<(), String> {
        if self.job.is_some() { Err("A workspace job is already running".into()) } else { Ok(()) }
    }

    fn launch(&mut self, mut launch: Launch) -> Result<(), String> {
        let process = Process::spawn(&mut launch.command, &launch.log, false).map_err(|error| error.to_string())?;
        append(&mut self.log, format!("{} started", launch.name));
        self.job = Some(Job::Running { process, analysis: launch.analysis, name: launch.name, started: std::time::Instant::now() });
        Ok(())
    }

    fn run_analysis(&mut self, session: &Session) -> Result<(), String> {
        self.idle()?;
        if self.use_open_model && session.model.elements().is_empty() {
            return Err("The open model needs a mesh before running an analysis".into());
        }
        self.save_all()?;
        let root = self.root()?;
        let analysis = in_project(root, self.analysis.as_deref().ok_or("Open a .analysis.json file first")?)?;
        let analysis = ngsolve::Analysis::read(&analysis).map_err(|error| error.to_string())?;
        let view = PreferredView { field: analysis.field.clone(), series: analysis.series.clone(), component: analysis.component.clone() };
        let interpreter = self.runtime()?.python(root);
        let dir = run_folder(root, "analysis")?;
        let model = self.use_open_model.then(|| session.clone());
        let (send, receive) = inbox::channel();
        std::thread::Builder::new()
            .name("fieldcraft-ngsolve-prepare".into())
            .spawn(move || {
                let snapshot = dir.join("model.fieldcraft");
                let result = analysis
                    .prepare(&dir.join("result"), model.map(|model| (model, snapshot.as_path())))
                    .and_then(|prepared| {
                        let command = prepared.command(&interpreter)?;
                        let analysis = Box::new(AnalysisJob { prepared, interpreter, view });
                        Ok(Launch { command, log: dir.join("analysis.log"), analysis: Some(analysis), name: "NGSolve analysis" })
                    })
                    .map_err(|error| error.to_string());
                let _ = send.send(result);
            })
            .map_err(|error| error.to_string())?;
        append(&mut self.log, "Preparing the FieldCraft mesh for NGSolve".into());
        self.job = Some(Job::Preparing(receive));
        Ok(())
    }

    fn run_file(&mut self) -> Result<(), String> {
        self.idle()?;
        self.save_all()?;
        let root = self.root()?;
        let file = in_project(root, self.active.as_deref().ok_or("Open a Python file first")?)?;
        if file.extension().and_then(|ext| ext.to_str()) != Some("py") {
            return Err("Open a Python file first".into());
        }
        let mut command = self.runtime()?.command(root, false)?;
        command.args(["-u", "-c", &ngsolve::with_helpers(runtime::RUN_FILE)]).arg(file);
        let dir = run_folder(root, "script")?;
        self.launch(Launch { command, log: dir.join("script.log"), analysis: None, name: "Python script" })
    }

    fn uv(&mut self, add: bool) -> Result<(), String> {
        self.idle()?;
        self.save_all()?;
        let root = self.root()?;
        let runtime = self.runtime()?;
        let mut command = runtime.command(root, true)?;
        let requirement = self.package.trim();
        if add && (requirement.is_empty() || requirement.len() > 1024 || requirement.starts_with('-')) {
            return Err("Enter a Python package requirement such as scipy or scipy>=1.15".into());
        }
        let project = root.join("pyproject.toml");
        if !project.exists() {
            OpenOptions::new()
                .create_new(true)
                .write(true)
                .open(&project)
                .and_then(|mut file| file.write_all(runtime::PROJECT.as_bytes()))
                .map_err(|error| error.to_string())?;
        }
        command.args(["-I", "-m", "uv", if add { "add" } else { "sync" }, "--no-python-downloads", "--python", &runtime.python]);
        command.env("UV_PROJECT_ENVIRONMENT", root.join(".venv"));
        if add {
            command.args(["--", requirement]);
        }
        let dir = run_folder(root, "environment")?;
        self.launch(Launch { command, log: dir.join("uv.log"), analysis: None, name: ENVIRONMENT_JOB })?;
        self.console = None;
        self.environment_status = Some(Ok("Updating the project environment…".into()));
        Ok(())
    }

    fn stop_job(&mut self) {
        if matches!(self.job.as_ref(), Some(Job::Running { name, .. }) if *name == ENVIRONMENT_JOB) {
            self.environment_status = Some(Ok("Stopped updating the project environment".into()));
        }
        self.job = None;
        append(&mut self.log, "Stopped job".into());
    }

    fn poll(&mut self) -> Option<RunResult> {
        if let Some(console) = &mut self.console {
            let polled = console.process.poll(|line| {
                if line == console.completion {
                    console.busy = false;
                } else {
                    append(&mut self.log, line);
                }
            });
            match polled {
                Ok(None) => {}
                Ok(Some(status)) => {
                    self.console = None;
                    append(&mut self.log, format!("Python console exited: {status}"));
                }
                Err(error) => {
                    self.console = None;
                    self.error = Some(error.to_string());
                }
            }
        }
        match self.job.as_mut()? {
            Job::Preparing(receive) => match receive.try_recv() {
                Err(TryRecvError::Empty) => None,
                result => {
                    self.job = None;
                    if let Err(error) = result.map_err(|error| error.to_string()).and_then(|plan| plan).and_then(|plan| self.launch(plan)) {
                        self.error = Some(error);
                    }
                    None
                }
            },
            Job::Verifying(receive) => match receive.try_recv() {
                Err(TryRecvError::Empty) => None,
                result => {
                    self.job = None;
                    match result
                        .map_err(|error| error.to_string())
                        .and_then(|result| result)
                        .and_then(|(run, view)| run.publish().map(|path| RunResult { path, view }).map_err(|error| error.to_string()))
                    {
                        Ok(result) => {
                            self.last_result = Some(result.path.clone());
                            append(&mut self.log, "NGSolve analysis finished".into());
                            Some(result)
                        }
                        Err(error) => {
                            self.error = Some(error);
                            None
                        }
                    }
                }
            },
            Job::Running { process, analysis, name, started } => match process.poll(|line| append(&mut self.log, line)) {
                Ok(None) if analysis.is_some() && started.elapsed() >= ngsolve::SOLVER_TIMEOUT => {
                    self.job = None;
                    self.error = Some("NGSolve exceeded the one-hour analysis time limit".into());
                    None
                }
                Ok(None) => None,
                Ok(Some(status)) => {
                    if status.success()
                        && let Some(analysis) = analysis.take()
                    {
                        let (send, receive) = inbox::channel();
                        let verify = std::thread::Builder::new().name("fieldcraft-ngsolve-verify".into()).spawn(move || {
                            let AnalysisJob { prepared, interpreter, view } = *analysis;
                            let result = prepared.verify(&interpreter).map(|verified| (verified, view)).map_err(|error| error.to_string());
                            let _ = send.send(result);
                        });
                        self.job = match verify {
                            Ok(_) => Some(Job::Verifying(receive)),
                            Err(error) => {
                                self.error = Some(error.to_string());
                                None
                            }
                        };
                        return None;
                    }
                    append(&mut self.log, format!("{name} {}", if status.success() { "finished".into() } else { format!("exited: {status}") }));
                    if *name == ENVIRONMENT_JOB {
                        self.environment_status = Some(if status.success() {
                            Ok("Project environment is ready".into())
                        } else {
                            Err(format!("uv exited with {status}. See the Python Workspace console for details."))
                        });
                    }
                    self.job = None;
                    None
                }
                Err(error) => {
                    if *name == ENVIRONMENT_JOB {
                        self.environment_status = Some(Err(error.to_string()));
                    }
                    self.job = None;
                    self.error = Some(error.to_string());
                    None
                }
            },
        }
    }
}

fn is_model_file(path: &Path) -> bool {
    path.extension().is_some_and(|ext| ext.eq_ignore_ascii_case("fieldcraft"))
}

impl ModelApp {
    pub(crate) fn new_python_analysis(&mut self) -> Result<(), String> {
        self.open_python_workspace();
        let elements = self.session.model.elements();
        let planar = !elements.is_empty() && elements.values().all(|element| element.kind == fieldcraft_model::ElementKind::Tri3);
        self.python_workspace.new_analysis(self.session.path.as_deref().map(Path::new), if planar { 2 } else { 3 })
    }

    pub(crate) fn run_python_analysis(&mut self, path: Option<&Path>) -> Result<(), String> {
        if let Some(path) = path {
            if !ngsolve::is_analysis_file(path) {
                return Err("Choose an NGSolve .analysis.json file".into());
            }
            self.open_python_workspace();
            self.python_workspace.open_file(path)?;
        }
        self.python_workspace.run_analysis(&self.session)
    }

    pub(crate) fn prepare_python_workspace(&mut self) {
        self.python_workspace.prepare(self.session.path.as_deref(), &self.prefs.python_interpreter);
    }

    pub(crate) fn open_python_workspace(&mut self) {
        self.prepare_python_workspace();
        self.ui.select_tab(fieldcraft_engine::catalogue::Tab::ExternalAnalysis);
        self.ui.menu = None;
        self.python_workspace.open = true;
    }

    pub(crate) fn set_python_interpreter(&mut self, python: String) {
        if self.prefs.python_interpreter != python {
            self.python_workspace.runtime = Some(Runtime::with_override(&python));
            self.prefs.python_interpreter = python;
            self.prefs.save(self.services.prefs.as_ref());
        }
    }
}

fn selected(source: &str, selection: &Selection) -> Option<(String, usize)> {
    if selection.start >= selection.end {
        return None;
    }
    let line = source.chars().take(selection.start).filter(|c| *c == '\n').count() + 1;
    let code = source.chars().skip(selection.start).take(selection.end - selection.start).collect();
    Some((code, line))
}

fn show_result(app: &mut ModelApp, path: &Path, view: &PreferredView) -> Result<(), String> {
    let manifest = path_string(path)?;
    app.run("results.external.integrals", json!({"path":manifest,"catalog_only":true})).map_err(|error| error.to_string())?;
    app.tree_open.insert("external".into());
    let preferred = app.session.external_catalog().and_then(|catalog| {
        if let Some(name) = &view.series
            && let Some(series) = catalog.series.iter().find(|series| &series.name == name)
        {
            app.tree_open.insert("external/series".into());
            return Some(("results.external.series", series.path.clone()));
        }
        if let Some(field) = catalog.fields.first() {
            app.tree_open.insert("external/fields".into());
            let field = view.field.as_ref().and_then(|name| catalog.fields.iter().find(|field| &field.name == name)).unwrap_or(field);
            Some(("results.external.contour", field.path.clone()))
        } else {
            catalog.series.first().map(|series| ("results.external.series", series.path.clone()))
        }
    });
    if let Some((command, path)) = preferred {
        let mut params = json!({"path":path});
        if let Some(component) = &view.component {
            params["component"] = json!(component);
        }
        app.run(command, params).map_err(|error| error.to_string())?;
    }
    Ok(())
}

pub(crate) fn poll(app: &mut ModelApp, ctx: &egui::Context) {
    if ctx.input(|input| input.viewport().close_requested()) && app.python_workspace.tabs.iter().any(Document::dirty) {
        ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
        app.open_python_workspace();
        app.python_workspace.error = Some("Save or discard the editor changes before closing FieldCraft".into());
    }
    if let Some(result) = app.python_workspace.poll()
        && let Err(error) = show_result(app, &result.path, &result.view)
    {
        append(&mut app.python_workspace.log, format!("Saved results. Open the matching model to display them. {error}"));
    }
    if app.python_workspace.job.is_some() || app.python_workspace.console.as_ref().is_some_and(|console| console.busy) {
        ctx.request_repaint_after(Duration::from_millis(80));
    } else if app.python_workspace.open {
        ctx.request_repaint_after(Duration::from_millis(300));
    }
}

pub(crate) use panel::{settings_page, show};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn selection_offsets_are_unicode_characters_and_preserve_the_source_line() {
        let source = "# µ\nvalue = 5\n";
        assert_eq!(selected(source, &Selection { start: 4, end: 13, caret: 13 }), Some(("value = 5".into(), 2)));
        assert_eq!(selected(source, &Selection { start: 0, end: 0, caret: 0 }), None);
    }

    #[test]
    fn new_analysis_opens_a_unique_editable_template() {
        let dir = std::env::temp_dir().join(format!("fc-python-analysis-{}", std::process::id()));
        fs::create_dir(&dir).unwrap();
        let model = dir.join("sample.fieldcraft");
        let script = dir.join("solve.py");
        let existing = dir.join("sample.analysis.json");
        fs::write(&model, "").unwrap();
        fs::write(&script, "pass\n").unwrap();
        fs::write(&existing, "existing definition").unwrap();
        let mut workspace = Workspace::default();
        workspace.set_project(&dir).unwrap();
        assert!(workspace.open_file(&model).is_err());
        workspace.open_file(&script).unwrap();
        workspace.new_analysis(Some(&model), 2).unwrap();
        let path = dir.canonicalize().unwrap().join("sample-2.analysis.json");
        assert_eq!(workspace.active.as_ref(), Some(&path));
        assert_eq!(workspace.analysis.as_ref(), Some(&path));
        let saved: serde_json::Value = serde_json::from_slice(&fs::read(path).unwrap()).unwrap();
        assert_eq!(saved["format"], ngsolve::ANALYSIS_FORMAT);
        assert_eq!(saved["model"], "sample.fieldcraft");
        assert_eq!(saved["script"], "solve.py");
        assert_eq!(saved["dimension"], 2);
        assert_eq!(saved["materials"], json!([]));
        assert_eq!(fs::read_to_string(existing).unwrap(), "existing definition");
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn saves_refuse_to_overwrite_a_script_changed_by_another_job() {
        let dir = std::env::temp_dir().join(format!("fc-python-editor-{}", std::process::id()));
        fs::create_dir(&dir).unwrap();
        let path = dir.join("solve.py");
        fs::write(&path, "old content").unwrap();
        let mut workspace = Workspace::default();
        workspace.set_project(&dir).unwrap();
        workspace.open_file(&path).unwrap();
        workspace.tabs[0].text = "local edit".into();
        fs::write(&path, "output from a job").unwrap();
        assert!(workspace.save(0).is_err());
        assert_eq!(fs::read_to_string(path).unwrap(), "output from a job");
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn persistent_console_shares_variables_recovers_after_errors_and_writes_files() {
        let Some(python) = std::env::var_os("FIELDCRAFT_WORKSPACE_TEST_PYTHON") else { return };
        let dir = std::env::temp_dir().join(format!("fc-python-console-{}", std::process::id()));
        fs::create_dir(&dir).unwrap();
        let mut workspace = Workspace::default();
        workspace.set_project(&dir).unwrap();
        workspace.runtime = Some(Runtime::with_override(&python.to_string_lossy()));
        fn wait_for_console(workspace: &mut Workspace) {
            for _ in 0..400 {
                workspace.poll();
                if workspace.console.as_ref().is_some_and(|console| !console.busy) {
                    return;
                }
                std::thread::sleep(Duration::from_millis(10));
            }
            panic!("console returned no completion marker");
        }
        let mut run = |source, line| {
            workspace.console_request(source, "<console>", line, None).unwrap();
            wait_for_console(&mut workspace);
        };
        run("value = 41", 1);
        run("raise ValueError('controlled failure')", 5);
        run("print(value + 1); from pathlib import Path; Path('answer.txt').write_text(str(value + 1))", 1);
        assert!(workspace.log.iter().any(|line| line.contains("line 5")));
        assert!(workspace.log.iter().any(|line| line == "42"));
        assert_eq!(fs::read_to_string(dir.join("answer.txt")).unwrap(), "42");
        let script = dir.join("sections.py");
        let source = "# %% Define µ\nvalue += 2\ntext = \"\"\"\n# %% In a string\n\"\"\"\n#%% Inspect\nprint(value)\n# %% Fail\nraise RuntimeError('Section error')\n";
        fs::write(&script, source).unwrap();
        workspace.open_file(&script).unwrap();
        for marker in ["# %% In a string", "#%% Inspect", "# %% Fail"] {
            let caret = source[..source.find(marker).unwrap()].chars().count();
            workspace.selection = Some(Selection { start: caret, end: caret, caret });
            workspace.run_section().unwrap();
            wait_for_console(&mut workspace);
        }
        assert!(workspace.log.iter().any(|line| line == "43"));
        assert!(workspace.log.iter().any(|line| line.contains("sections.py\", line 9")));
        drop(workspace);
        fs::remove_dir_all(dir).unwrap();
    }
}
