//! Select the private or project Python environment and run the persistent console.

use std::env;
use std::path::{Path, PathBuf};
use std::process::Command;

/// A selection has the same namespace as the console prompt. Read requests from the original
/// stdin; the script sees EOF on stdin so input() cannot consume the next editor request.
pub(super) const CONSOLE: &str = r#"
import io, json, linecache, os, sys, textwrap, tokenize, traceback

def section(document, line):
    lines = io.StringIO(document).readlines()
    start, end = 1, len(lines) + 1
    has_marker = False
    for token in tokenize.generate_tokens(io.StringIO(document).readline):
        if token.type != tokenize.COMMENT or not token.string[1:].lstrip().startswith('%%'):
            continue
        row, column = token.start
        if lines[row - 1][:column].strip():
            continue
        has_marker = True
        if row > line:
            end = row
            break
        start = row + 1
    if not has_marker:
        raise ValueError('Add a # %% line to define sections.')
    code = ''.join(lines[start - 1:end - 1])
    if not code.strip():
        raise ValueError('The current section is empty.')
    return start, code

def main():
    requests, response = sys.stdin, sys.stdout
    sys.stdin = sys.__stdin__ = open(os.devnull)
    namespace = {'__name__': '__main__'}
    for raw in requests:
        request = json.loads(raw)
        filename = request['file']
        document = request.get('document')
        if document is not None:
            linecache.cache[filename] = (len(document), None, document.splitlines(keepends=True), filename)
            namespace['__file__'] = filename
        original_path = sys.path[:]
        if os.path.isabs(filename):
            sys.path.insert(0, os.path.dirname(filename))
        try:
            if 'section' in request:
                try:
                    line, code = section(document, request['section'])
                except (tokenize.TokenError, SyntaxError, ValueError) as error:
                    print(f'Run Section: {error}')
                    continue
            else:
                line, code = request['line'], request['code']
            source = '\n' * (line - 1) + textwrap.dedent(code)
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

pub(super) const RUN_FILE: &str = r#"
import runpy
script = Path(sys.argv.pop(1))
sys.argv[0] = str(script)
sys.path.insert(0, str(script.parent))
runpy.run_path(str(script), run_name='__main__')
"#;

pub(super) const PROJECT: &str = "[project]\nname = \"fieldcraft-project\"\nversion = \"0.1.0\"\nrequires-python = \"==3.13.*\"\ndependencies = [\n    \"ngsolve==6.2.2608\",\n    \"numpy==2.3.5\",\n]\n";

#[derive(Clone, Debug, Default)]
pub(super) struct Runtime {
    pub(super) python: String,
}

impl Runtime {
    pub(super) fn with_override(python: &str) -> Self {
        if python.is_empty() { Self::detect() } else { Self { python: python.to_string() } }
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
        roots
            .into_iter()
            .map(|root| root.join(relative))
            .find(|path| path.is_absolute() && path.is_file())
            .map_or_else(Self::default, |path| Self { python: path.to_string_lossy().into_owned() })
    }

    pub(super) fn python(&self, root: &Path) -> PathBuf {
        let local = root.join(if cfg!(windows) { ".venv/Scripts/python.exe" } else { ".venv/bin/python" });
        if local.is_file() { local } else { PathBuf::from(&self.python) }
    }

    pub(super) fn command(&self, root: &Path, base: bool) -> Result<Command, String> {
        let python = if base { PathBuf::from(&self.python) } else { self.python(root) };
        fieldcraft_engine::ngsolve::python_command(&python, root).map_err(|error| error.to_string())
    }
}
