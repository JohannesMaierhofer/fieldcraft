# FieldCraft with a private NGSolve interpreter on macOS

Build a native product installer from an already signed FieldCraft.app on a Mac. The default installer contains the FieldCraft application and a small installer component. During installation, that component downloads the unmodified CPython archive, NGSolve bridge and wheels directly from GitHub and PyPI. It checks each SHA-256 against the selected architecture lock, installs into a private directory, runs `pip check`, and verifies an NGSolve integral before activating the version. Future solver runs work offline. Existing installations of Python and NGSolve remain available to other applications.

```sh
packaging/macos/ngsolve-package.sh --arch x86_64 \
  --app dist/FieldCraft.app --output dist/FieldCraft-NGSolve-intel.pkg
```

For Apple Silicon, pass `--arch arm64` and an arm64 or universal application. `/Library/Application Support/FieldCraft/NGSolve/python/bin/python3` is the stable executable path for external analysis scripts. `-I` runs it with user site-packages and `PYTHONPATH` disabled. This base install provides NGSolve, Netgen, OCCT and NumPy. Scripts importing additional packages require those packages in the selected interpreter.

The bridge, selected by commit and archive digest, lives at `/Library/Application Support/FieldCraft/NGSolve/bridge`. Copy the files from `bridge/samples` to a working directory and run `fieldcraft-cli run slab.json` there to create the native model. The case combines that editable model with the Python formulation. With the installed app and its private runtime, one invocation solves the case and opens its selected electric-field component and energy in a new FieldCraft window. The run directory must be new.

```sh
runtime="/Library/Application Support/FieldCraft/NGSolve"
"$runtime/fieldcraft-ngsolve" case slab.case.json run-01 \
  --gui /Applications/FieldCraft.app/Contents/MacOS/FieldCraft
```

The launcher excludes Python user packages and inherited Python library paths. The same invocation without `--gui` creates the result files headlessly. `bridge/show_run.py` can select a different field or show integrals on a running FieldCraft control port.

The Python-build-standalone GitHub release `20261110` publishes the interpreter hashes in its asset metadata. Wheel hashes are the digests of the exact macOS files on each pinned PyPI release. Both architecture locks preserve complete `https` URLs and SHA-256 digests. The working tree contains only the install recipe, notices and locks. At install time the private runtime keeps the original Python license under `lib/python3.13/LICENSE.txt` and the original legal notices from each wheel. See [Python-build-standalone](https://github.com/astral-sh/python-build-standalone/releases/tag/20261110), [NGSolve](https://pypi.org/project/ngsolve/6.2.2608/), [Netgen](https://pypi.org/project/netgen-mesher/6.2.2608/), [OCCT wheels](https://pypi.org/project/netgen-occt/7.8.1/) and [NumPy](https://pypi.org/project/numpy/2.3.5/).

For an offline **local** installer, provide a directory containing all six archives specified in the chosen lock, under their original filenames, with the GitHub source archive named `bridge-<commit>.zip`. The builder checks them and attaches those exact archives to the runtime component. Installation performs the same verification without a download. The public recipe leaves upstream solver binaries with their distributors. Anyone distributing an offline copy is responsible for the corresponding-source obligations of the included components, including LGPL 2.1 §4. [LGPL 2.1](https://www.gnu.org/licenses/old-licenses/lgpl-2.1.en.html)

```sh
packaging/macos/ngsolve-package.sh --arch x86_64 --app dist/FieldCraft.app \
  --offline-cache /path/to/verified-archives --output dist/FieldCraft-NGSolve-intel-local.pkg
pkgutil --expand dist/FieldCraft-NGSolve-intel-local.pkg /tmp/fieldcraft-installer-inspection
```

A release build can set `MACOS_INSTALLER_IDENTITY` to an installed Developer ID Installer identity. With no identity the local product package is unsigned. The app itself must already pass strict code-signature verification and contain the requested slice. Public distribution additionally uses the project's release signing and notarization process for the finished `.pkg`.
