//! Use the installed bridge through its public Python entry points.

use std::env;
use std::path::{Path, PathBuf};
use std::process::Command;

/// A selection has the same namespace as the console prompt. Read requests from the original
/// stdin; the script sees EOF on stdin so input() cannot consume the next editor request.
pub(super) const CONSOLE: &str = r#"
import json, linecache, os, sys, textwrap, traceback

def main():
    requests, response = sys.stdin, sys.stdout
    sys.stdin = sys.__stdin__ = open(os.devnull)
    namespace = {'__name__': '__main__'}
    for raw in requests:
        request = json.loads(raw)
        filename = request['file']
        if request.get('document') is not None:
            document = request['document']
            linecache.cache[filename] = (len(document), None, document.splitlines(keepends=True), filename)
            namespace['__file__'] = filename
        source = '\n' * (request['line'] - 1) + textwrap.dedent(request['code'])
        original_path = sys.path[:]
        if os.path.isabs(filename):
            sys.path.insert(0, os.path.dirname(filename))
        try:
            try:
                expression = compile(source, filename, 'eval')
            except SyntaxError:
                expression = None
            if expression is not None:
                sys.displayhook(eval(expression, namespace))
            else:
                exec(compile(source, filename, 'exec'), namespace)
        except BaseException as error:
            traceback.print_exception(type(error), error, error.__traceback__.tb_next)
        finally:
            sys.path[:] = original_path
            response.write('\n' + sys.argv[1] + '\n')
            response.flush()

main()
"#;

pub(super) const RUN_CASE: &str = r#"
import sys
from pathlib import Path
from types import SimpleNamespace

case_path, output, bridge = sys.argv[1:4]
sys.path.insert(0, bridge)
from case_run import load_case
from workflow import run

case = load_case(Path(case_path))
run(SimpleNamespace(model=case['model'],
    script=case['script'], out=Path(output), python=sys.executable,
    adapter=Path(bridge), timeout=3600, dimension=case.get('dimension', 3),
    materials=case['materials'], boundaries=case['boundaries'], interfaces=case['interfaces']))
"#;

pub(super) const PROJECT: &str = "[project]\nname = \"fieldcraft-project\"\nversion = \"0.1.0\"\nrequires-python = \"==3.13.*\"\ndependencies = [\n    \"ngsolve==6.2.2608\",\n    \"numpy==2.3.5\",\n]\n";

#[derive(Clone, Debug, Default)]
pub(super) struct Runtime {
    pub(super) python: String,
    pub(super) bridge: String,
}

impl Runtime {
    pub(super) fn with_overrides(python: &str, bridge: &str) -> Self {
        let mut runtime = if python.is_empty() || bridge.is_empty() { Self::detect() } else { Self::default() };
        if !python.is_empty() {
            runtime.python = python.to_string();
        }
        if !bridge.is_empty() {
            runtime.bridge = bridge.to_string();
        }
        runtime
    }

    fn detect() -> Self {
        let windows = cfg!(windows);
        let relative = if windows { "python/python.exe" } else { "python/bin/python3" };
        let mut roots = Vec::new();
        if let Some(root) = env::var_os("FIELDCRAFT_NGSOLVE_HOME") {
            roots.push(PathBuf::from(root));
        }
        #[cfg(target_os = "macos")]
        {
            roots.push(PathBuf::from("/Library/Application Support/FieldCraft/NGSolve"));
            if let Some(home) = env::home_dir() {
                roots.push(home.join("Library/Application Support/FieldCraft/NGSolve"));
            }
        }
        #[cfg(windows)]
        if let Some(local) = env::var_os("LOCALAPPDATA") {
            roots.push(PathBuf::from(local).join("Programs/FieldCraft-NGSolve"));
        }
        #[cfg(all(unix, not(target_os = "macos")))]
        roots.push(PathBuf::from("/opt/fieldcraft-ngsolve"));
        if let Ok(executable) = env::current_exe()
            && let Some(parent) = executable.parent()
        {
            roots.push(parent.to_path_buf());
        }
        roots.into_iter().find(|root| root.join(relative).is_file() && root.join("bridge/workflow.py").is_file()).map_or_else(Self::default, |root| {
            Self { python: root.join(relative).to_string_lossy().into_owned(), bridge: root.join("bridge").to_string_lossy().into_owned() }
        })
    }

    pub(super) fn python(&self, root: &Path) -> PathBuf {
        let local = root.join(if cfg!(windows) { ".venv/Scripts/python.exe" } else { ".venv/bin/python" });
        if local.is_file() { local } else { PathBuf::from(&self.python) }
    }

    pub(super) fn command(&self, root: &Path, base: bool) -> Result<Command, String> {
        // A venv's executable path must keep its symlink so CPython finds pyvenv.cfg.
        let python = if base { PathBuf::from(&self.python) } else { self.python(root) };
        if !python.is_absolute() || !python.is_file() {
            return Err("Select the installed FieldCraft Python interpreter in Settings → Python".into());
        }
        let mut command = Command::new(python);
        command
            .current_dir(root)
            .env_remove("PYTHONHOME")
            .env_remove("PYTHONPATH")
            .env_remove("VIRTUAL_ENV")
            .env("PYTHONNOUSERSITE", "1")
            .env("PYTHONUNBUFFERED", "1")
            .env("PYTHONUTF8", "1")
            .arg("-s");
        if !self.bridge.is_empty() {
            command.env("PYTHONPATH", &self.bridge);
        }
        Ok(command)
    }

    pub(super) fn bridge(&self) -> Result<&Path, String> {
        let bridge = Path::new(&self.bridge);
        if bridge.is_absolute() && bridge.join("workflow.py").is_file() {
            Ok(bridge)
        } else {
            Err("Select the installed NGSolve bridge folder in Settings → Python".into())
        }
    }
}
