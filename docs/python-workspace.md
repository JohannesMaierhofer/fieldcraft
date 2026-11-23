# Python Workspace

Open **File → Python Workspace** in the FieldCraft desktop. The NGSolve installer sets up CPython, NGSolve, NumPy, StudyPype, and uv. Open a project folder with **Browse** or use the folder of the open model. The file tree opens models in the viewport and Python, YAML, JSON, and other text files in the editor. Use **New File** or **New Folder** with a project-relative path. Existing files are protected against edits made on disk while their editor tabs are open.

The workspace opens docked on the right. Drag the divider to resize it. Choose **Undock** to move it to a separate window and **Dock** to return it to the main window. The project, editor tabs, running jobs, and Python console stay with the workspace as it moves.

## Run a saved solver case

Open a `.case.json` file to select it, then edit its Python formulation and choose **Run Case**. The workspace saves edited files before it starts a fresh Python process. Select **Use Open Model** to save a snapshot of the current in-memory model into the run folder. Otherwise the case uses the model named in its JSON file. The case still supplies the named materials, exterior boundaries, and optional interior interfaces. [Case format](https://github.com/JohannesMaierhofer/fieldcraft-ngsolve#run-a-saved-case)

Each run keeps a log and its outputs in `runs/`. The matching model receives validated fields, time series, and integrals in its **External Analysis** tree. Open a previous `results.json` and choose **Show Results** to display the run again. If the run's mesh differs from the open model, open the matching model from the project tree first. CSV previews plot the first two numeric columns, including a force-versus-gap series saved by a study.

## Run a selection or use the console

Select Python code in an editor tab and choose **Run Selection** or press Ctrl/Cmd+Enter. The selection uses the same long-lived interpreter as the console prompt. Imports and variables survive subsequent selections. An expression entered at the prompt prints its value. Traceback frames within the project open the named file at the reported line.

Choose **Run Python File** to save and execute the whole script in a fresh process with the project folder as its working directory.

After running a case, **Load Last Case Mesh** makes `mesh_path`, `mesh`, and `data` available in the console through the bridge. **Reset Console** clears its Python state and stops a running selection. **Stop Job** stops the saved case, study, or environment command. These are local processes run with your account's permissions. Opening a project or a file leaves its Python code dormant until you choose to run it. Scripts launched by the workspace receive EOF on standard input.

## Run a StudyPype study

Open a StudyPype YAML file and select its job directory, `tasks` by default. **Run Study** starts in the project folder. Use project-relative paths in the YAML as you would when launching the study from that folder in a terminal. StudyPype expands `!sweep` values, runs dependency-ordered jobs, and creates its usual result tree. The workspace shows the files as they appear. Each job can use `ngsolve`, the FieldCraft bridge, and the project's other packages. [StudyPype usage](https://github.com/Maierhofer-Technology/studypype#running-a-study)

## Manage project packages

Use **Python Environment → Sync Environment** to create or update `.venv` with the installer-provided uv and Python. If the project has no `pyproject.toml`, FieldCraft creates one with NGSolve, NumPy, and StudyPype pinned to the installer versions. For an existing `pyproject.toml`, declare the packages your scripts import there. Enter `scipy` or a requirement such as `scipy>=1.15` and choose **Add Package** to update `pyproject.toml`, `uv.lock`, and `.venv`. Version `pyproject.toml` and `uv.lock` with your case and study sources. The first synchronization may download wheels. The synced project interpreter is used by cases, studies, and the console. Without a project environment, the workspace uses the installed base interpreter.

Advanced users can select a compatible base interpreter and bridge under **Python Environment**. For a relocatable installation, set `FIELDCRAFT_NGSOLVE_HOME` to the directory containing `python/` and `bridge/` before starting FieldCraft.
