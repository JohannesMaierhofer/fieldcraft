# FieldCraft with an external NGSolve runtime

The Windows installer sets up FieldCraft for the current user. Start it from the Start menu. The Linux amd64 package supports Ubuntu 22.04 or later. Install it with `sudo apt install ./fieldcraft-ngsolve-linux-x64.deb` and select **FieldCraft with NGSolve** from the applications menu. Both installers carry the application, command-line interface, bridge, examples and a private CPython environment with the pinned wheels. On Linux, apt resolves the system graphics libraries.

| Installer | Native FieldCraft app and CLI | External solver |
|---|---|---|
| windows-x64 | Windows x64 | CPython x64 |
| windows-arm64 | Windows ARM64 | CPython x64 through Windows 11 x64 emulation |
| linux-x64 | Linux x86_64, glibc 2.35 or later | CPython x86_64 |

A Linux ARM64 installer requires compatible NGSolve and Netgen binaries. NGSolve 6.2.2608 on PyPI supplies CPython 3.13 Linux x86_64 wheels. The package lock records the selected interpreter and each wheel URL and SHA-256.

## First calculation

Create a writable model directory. Copy `bridge/samples/slab.json` into it, mesh the slab with FieldCraft, then run the installed example.

Linux shell:

```sh
bridge=/opt/fieldcraft-ngsolve/bridge
cp "$bridge/samples/slab.json" .
fieldcraft-ngsolve-cli run slab.json
fieldcraft-ngsolve slab.fieldcraft "$bridge/samples/potential.py" first-run --materials dielectric --boundaries left right wall
fieldcraft-ngsolve-gui --control 7070
```

Windows PowerShell, using the default installation path:

```powershell
$fc = Join-Path $env:LOCALAPPDATA 'Programs/FieldCraft-NGSolve'
Copy-Item "$fc/bridge/samples/slab.json" .
& "$fc/fieldcraft-cli.exe" run slab.json
& "$fc/fieldcraft-ngsolve.cmd" slab.fieldcraft "$fc/bridge/samples/potential.py" first-run --materials dielectric --boundaries left right wall
& "$fc/fieldcraft.exe" --control 7070
```

In a second terminal, call the installed Python executable with `bridge/show_run.py first-run --port 7070 --field potential --integrals --open-model`. The interpreter is `python/python.exe` under the Windows installation or `/opt/fieldcraft-ngsolve/python/bin/python3` on Linux. Replace the example Python file with your own formulation and select the sets defined by your FieldCraft model.

## Build and verification

The fork workflow pins the FieldCraft source separately from the installer branch. It builds native binaries on each target, installs the resulting package, verifies the runtime versions, solves the 1 V slab and checks its electrostatic energy against 4.4270939064 × 10⁻¹⁴ J. The verification stage imports both fields and captures the installed desktop window. Actions artifacts are available after that job passes. Python package notices stay with their original installed distributions. The Python distribution, external bridge and FieldCraft each include their licence files.
