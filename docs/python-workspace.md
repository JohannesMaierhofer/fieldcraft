# Python Workspace

Choose **External Analysis** in the desktop workspace switcher, then **Open Python Workspace** on the **Python** ribbon tab. The NGSolve installer sets up CPython, NGSolve, NumPy, and uv. Open a project folder with **Browse** or use the folder of the open model. Open Python, YAML, JSON, and other text files from the file tree. Open FieldCraft models from the main File menu. Use **New File** or **New Folder** with a project-relative path. Existing files are protected against edits made on disk while their editor tabs are open.

The workspace opens docked on the right. Drag the divider to resize it. Choose **Undock** to move it to a separate window and **Dock** to return it to the main window. The project, editor tabs, running jobs, and Python console stay with the workspace as it moves.

## Run an analysis

Choose **New Analysis** to create and open a new `.analysis.json` in the project folder. FieldCraft fills in the open model and the active Python script when they are in that folder. Fill in the material and boundary set names, then choose any interior sets or a preferred result. If the file name is already in use, FieldCraft creates a numbered file.

Open an analysis definition (`.analysis.json`) to select it, edit its Python script, and choose **Run Analysis**. FieldCraft saves edited files and exports the selected mesh regions before starting a fresh Python process. Select **Use Open Model** to save a snapshot of the current in-memory model into the run folder. Otherwise FieldCraft opens the model named in the analysis definition.

The JSON format is `fieldcraft-ngsolve-analysis/1`. Set `model` to a native `.fieldcraft` file and `script` to a Python file. FieldCraft resolves relative paths from the definition file. The `materials` and `boundaries` lists name the model sets that cover all cells and exterior faces. Include `interfaces` for named interior faces in 3D. Set `dimension` to 2 for a planar TRI3 mesh or use the default of 3 for a TET4 mesh. To choose the first output displayed in FieldCraft, set `field` or `series` and, for vector data, optionally set `component` to `x`, `y`, `z`, or `magnitude`.

Save a definition beside your script as, for example, `slab.analysis.json`. These material and boundary names must match the sets in the slab model.

```json
{
  "format": "fieldcraft-ngsolve-analysis/1",
  "model": "slab.fieldcraft",
  "script": "potential.py",
  "materials": ["dielectric"],
  "boundaries": ["left", "right", "wall"],
  "field": "electric_field",
  "component": "x"
}
```

Existing scripts can use `from fc_results import load_mesh, write_results`. FieldCraft provides this module inside the Python process. `load_mesh()` returns the NGSolve mesh and its FieldCraft metadata. `write_results()` publishes fields, integrals, and series for FieldCraft to validate and display.

Each run keeps a log and its outputs in `runs/`. The matching model receives validated fields, time series, and integrals in its **External Analysis** tree. Open a previous `results.json` and choose **Show Results** to display the run again. If the run's mesh differs from the open model, open the matching model from the File menu first. CSV previews plot the first two numeric columns, including a force-versus-gap series written by a script.

## Run sections or selections in the console

Start a Python section with a line such as `# %% Material properties`. Place the cursor in a section and choose **Run Section** or press Cmd+Enter on macOS and Ctrl+Enter elsewhere. To run highlighted code, choose **Run Selection** or add Shift to that shortcut. Sections, selections, and the console prompt share one interpreter. Imports and variables survive subsequent runs. An expression entered at the prompt prints its value. Traceback frames within the project open the named file at the reported line.

Choose **Run Python File** to save and execute the whole script in a fresh process with the project folder as its working directory.

**Reset Console** clears its Python state and stops a running selection. **Stop Job** stops the analysis, script, or environment command. These are local processes run with your account's permissions. Opening a project or a file leaves its Python code dormant until you choose to run it. Scripts launched by the workspace receive EOF on standard input.

## Manage project packages

Open **Settings** on the **Python** ribbon tab and choose **Sync Environment** to create or update `.venv` with the installer-provided uv and Python. If the project has no `pyproject.toml`, FieldCraft creates one with NGSolve and NumPy pinned to the installer versions. Enter `scipy`, `studypype`, or a requirement such as `scipy>=1.15` and choose **Add Package** to update `pyproject.toml`, `uv.lock`, and `.venv`. Version `pyproject.toml` and `uv.lock` with your scripts. The first synchronization may download wheels. Analyses, scripts, and new console sessions use the synced interpreter when the project has a `.venv`. Starting a package operation resets the workspace console.

Use packages from a script or the console with normal Python imports, such as `import studypype`. [StudyPype usage](https://github.com/Maierhofer-Technology/studypype#running-a-study)

**Settings → Python** also accepts a compatible base interpreter. Leave that field empty for automatic detection. Use **Reset Console** after changing runtime paths to start a new interpreter. For a relocatable installation, set `FIELDCRAFT_NGSOLVE_HOME` to the directory containing `python/` before starting FieldCraft.
