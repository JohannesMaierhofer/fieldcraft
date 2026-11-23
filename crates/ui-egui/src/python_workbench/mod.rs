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

use fieldcraft_engine::Session;
use serde_json::{Value, json};

use crate::{ModelApp, inbox};
use process::Process;
use runtime::Runtime;

const MAX_FILE: u64 = 1024 * 1024;
const MAX_TABS: usize = 24;
const MAX_LOG_LINES: usize = 400;
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
    case: Option<PathBuf>,
    study: Option<PathBuf>,
    last_result: Option<PathBuf>,
    use_open_model: bool,
    task_folder: String,
    new_path: String,
    package: String,
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
}

struct Console {
    process: Process,
    completion: String,
    busy: bool,
}

struct Launch {
    command: std::process::Command,
    log: PathBuf,
    result: Option<RunResult>,
    name: &'static str,
}

struct RunResult {
    path: PathBuf,
    view: PreferredView,
}

#[derive(Default, serde::Deserialize)]
struct PreferredView {
    field: Option<String>,
    series: Option<String>,
    component: Option<String>,
}

enum Job {
    Preparing(inbox::Inbox<Result<Launch, String>>),
    Running { process: Process, result: Option<RunResult>, name: &'static str },
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
    pub(crate) fn open(&mut self, model: Option<&str>) {
        self.open = true;
        self.runtime.get_or_insert_with(Runtime::detect);
        if self.task_folder.is_empty() {
            self.task_folder = "tasks".to_string();
        }
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
            self.case = None;
            self.study = None;
            self.last_result = None;
            self.use_open_model = false;
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
        if path.file_name().and_then(|name| name.to_str()).is_some_and(|name| name.ends_with(".case.json")) {
            self.case = Some(path.clone());
        }
        if path.extension().and_then(|ext| ext.to_str()).is_some_and(|ext| matches!(ext, "yaml" | "yml")) {
            self.study = Some(path.clone());
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
            OpenOptions::new().write(true).create_new(true).open(&path).map_err(|error| error.to_string())?;
            self.open_file(&path)?;
        }
        self.new_path.clear();
        Ok(())
    }

    fn console_request(&mut self, code: &str, filename: &str, line: usize, document: Option<&str>) -> Result<(), String> {
        if code.trim().is_empty() || code.len() as u64 > MAX_FILE {
            return Err("Select up to 1 MiB of Python code or enter a console command".into());
        }
        if self.console.as_ref().is_some_and(|console| console.busy) {
            return Err("The Python console is running code".into());
        }
        if self.console.is_none() {
            let root = self.root()?;
            let mut command = self.runtime()?.command(root, false)?;
            let dir = run_folder(root, "console")?;
            let completion = format!("fieldcraft-console-complete-{}", RUN_ID.fetch_add(1, Ordering::Relaxed));
            command.args(["-u", "-c", runtime::CONSOLE, &completion]);
            let process = Process::spawn(&mut command, &dir.join("console.log"), true).map_err(|error| error.to_string())?;
            self.console = Some(Console { process, completion, busy: false });
        }
        let console = self.console.as_mut().ok_or("Could not start the Python console")?;
        let request = json!({"code":code,"file":filename,"line":line,"document":document});
        console.process.send(serde_json::to_vec(&request).map_err(|error| error.to_string())?)?;
        console.busy = true;
        let preview: String = code.lines().next().unwrap_or_default().chars().take(240).collect();
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

    fn use_run_in_console(&mut self) -> Result<(), String> {
        let path = self.last_result.as_ref().and_then(|file| file.parent()).ok_or("Run a case first")?;
        let mesh = serde_json::to_string(path_string(&path.join("mesh.npz"))?).map_err(|error| error.to_string())?;
        self.console_request(
            &format!("from fc_results import load_mesh\nmesh_path = {mesh}\nmesh, data = load_mesh(mesh_path)"),
            "<console>",
            1,
            None,
        )
    }

    fn idle(&self) -> Result<(), String> {
        if self.job.is_some() { Err("A workspace job is already running".into()) } else { Ok(()) }
    }

    fn launch(&mut self, mut launch: Launch) -> Result<(), String> {
        let process = Process::spawn(&mut launch.command, &launch.log, false).map_err(|error| error.to_string())?;
        append(&mut self.log, format!("{} started", launch.name));
        self.job = Some(Job::Running { process, result: launch.result, name: launch.name });
        Ok(())
    }

    fn run_case(&mut self, session: &Session) -> Result<(), String> {
        self.idle()?;
        if self.use_open_model && session.model.elements().is_empty() {
            return Err("The open model needs a mesh before running a case".into());
        }
        self.save_all()?;
        let root = self.root()?;
        let runtime = self.runtime()?;
        let case = in_project(root, self.case.as_deref().ok_or("Open a .case.json file first")?)?;
        let definition: Value = serde_json::from_str(&read_text(&case)?).map_err(|error| error.to_string())?;
        let view = serde_json::from_value(definition.clone()).map_err(|error| error.to_string())?;
        let mut command = runtime.command(root, false)?;
        let bridge = runtime.bridge()?;
        let dir = run_folder(root, "case")?;
        let out = dir.join("result");
        let snapshot = dir.join("model.fieldcraft");
        let case = if self.use_open_model { snapshot_case(definition, &case, &snapshot)? } else { case };
        command.args(["-u", "-c", runtime::RUN_CASE]).arg(case).arg(&out).arg(bridge);
        let launch =
            Launch { command, log: dir.join("case.log"), result: Some(RunResult { path: out.join("results.json"), view }), name: "NGSolve case" };
        if !self.use_open_model {
            return self.launch(launch);
        }
        let mut model = session.clone();
        let (send, receive) = inbox::channel();
        std::thread::Builder::new()
            .name("fieldcraft-python-snapshot".into())
            .spawn(move || {
                let result = path_string(&snapshot).and_then(|path| {
                    #[allow(clippy::disallowed_methods, reason = "the snapshot worker saves its own model copy")]
                    let saved = model.execute("file.save_as", &json!({"path":path,"results":false,"geometry":"refer"}));
                    saved.map_err(|error| error.to_string())?;
                    Ok(launch)
                });
                let _ = send.send(result);
            })
            .map_err(|error| error.to_string())?;
        append(&mut self.log, "Saving the open model for this case".into());
        self.job = Some(Job::Preparing(receive));
        Ok(())
    }

    fn run_study(&mut self) -> Result<(), String> {
        self.idle()?;
        self.save_all()?;
        let root = self.root()?;
        let study = in_project(root, self.study.as_deref().ok_or("Open a StudyPype YAML file first")?)?;
        let tasks = in_project(root, Path::new(&self.task_folder))?;
        let mut command = self.runtime()?.command(root, false)?;
        command.args(["-u", "-c", runtime::RUN_STUDY]).arg(study).arg(tasks);
        let dir = run_folder(root, "study")?;
        self.launch(Launch { command, log: dir.join("study.log"), result: None, name: "StudyPype study" })
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
        command.arg("-u").arg(file);
        let dir = run_folder(root, "script")?;
        self.launch(Launch { command, log: dir.join("script.log"), result: None, name: "Python script" })
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
        self.console = None;
        self.launch(Launch { command, log: dir.join("uv.log"), result: None, name: "Python environment sync" })
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
            Job::Running { process, result, name } => match process.poll(|line| append(&mut self.log, line)) {
                Ok(None) => None,
                Ok(Some(status)) => {
                    let result = if status.success() { result.take() } else { None };
                    append(&mut self.log, format!("{name} {}", if status.success() { "finished".into() } else { format!("exited: {status}") }));
                    self.job = None;
                    if let Some(result) = &result {
                        self.last_result = Some(result.path.clone());
                    }
                    result
                }
                Err(error) => {
                    self.job = None;
                    self.error = Some(error.to_string());
                    None
                }
            },
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

fn snapshot_case(mut definition: Value, case: &Path, snapshot: &Path) -> Result<PathBuf, String> {
    let fields = definition.as_object_mut().ok_or("A solver case must be a JSON object")?;
    let script =
        fields.get("script").and_then(Value::as_str).filter(|path| !path.trim().is_empty()).ok_or("The solver case needs a Python script")?;
    let original_folder = case.parent().ok_or("The original case needs a parent folder")?;
    let script = original_folder.join(script);
    fields.insert("script".into(), json!(path_string(&script)?));
    fields.insert("model".into(), json!(path_string(snapshot)?));
    let path = snapshot.with_file_name("snapshot.case.json");
    fs::write(&path, serde_json::to_vec_pretty(&definition).map_err(|error| error.to_string())?).map_err(|error| error.to_string())?;
    Ok(path)
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
        app.python_workspace.open(app.session.path.as_deref());
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

pub(crate) use panel::show;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn selection_offsets_are_unicode_characters_and_preserve_the_source_line() {
        let source = "# µ\nvalue = 5\n";
        assert_eq!(selected(source, &Selection { start: 4, end: 13 }), Some(("value = 5".into(), 2)));
        assert_eq!(selected(source, &Selection { start: 0, end: 0 }), None);
    }

    #[test]
    fn saves_refuse_to_overwrite_a_script_changed_by_a_study() {
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
    fn snapshot_cases_keep_script_paths_relative_to_the_original_case() {
        let parent = std::env::temp_dir().join(format!("fc-snapshot-case-{}", std::process::id()));
        fs::create_dir(&parent).unwrap();
        let case = parent.join("cases/magnetic.case.json");
        let snapshot = parent.join("model.fieldcraft");
        let definition = json!({"model":"will-be-saved.fieldcraft","script":"solve.py","materials":["steel"]});
        let copy = snapshot_case(definition, &case, &snapshot).unwrap();
        let effective: Value = serde_json::from_str(&read_text(&copy).unwrap()).unwrap();
        assert_eq!(effective["model"], json!(snapshot));
        assert_eq!(effective["script"], json!(parent.join("cases/solve.py")));
        assert_eq!(effective["materials"], json!(["steel"]));
        fs::remove_dir_all(parent).unwrap();
    }

    #[test]
    fn persistent_console_shares_variables_recovers_after_errors_and_writes_files() {
        let Some(python) = std::env::var_os("FIELDCRAFT_WORKSPACE_TEST_PYTHON") else { return };
        let dir = std::env::temp_dir().join(format!("fc-python-console-{}", std::process::id()));
        fs::create_dir(&dir).unwrap();
        let mut workspace = Workspace::default();
        workspace.set_project(&dir).unwrap();
        workspace.runtime = Some(Runtime { python: python.to_string_lossy().into_owned(), bridge: String::new() });
        let mut run = |source, line| {
            workspace.console_request(source, "<console>", line, None).unwrap();
            for _ in 0..400 {
                workspace.poll();
                if workspace.console.as_ref().is_some_and(|console| !console.busy) {
                    return;
                }
                std::thread::sleep(Duration::from_millis(10));
            }
            panic!("console returned no completion marker");
        };
        run("value = 41", 1);
        run("raise ValueError('controlled failure')", 5);
        run("print(value + 1); from pathlib import Path; Path('answer.txt').write_text(str(value + 1))", 1);
        assert!(workspace.log.iter().any(|line| line.contains("line 5")));
        assert!(workspace.log.iter().any(|line| line == "42"));
        assert_eq!(fs::read_to_string(dir.join("answer.txt")).unwrap(), "42");
        drop(workspace);
        fs::remove_dir_all(dir).unwrap();
    }
}
