# FieldCraft with an external NGSolve runtime

The Windows installer sets up FieldCraft for the current user. Start it from the Start menu. The Linux amd64 package supports Ubuntu 22.04 or later. Install it with `sudo apt install ./fieldcraft-ngsolve-linux-x64.deb` and select **FieldCraft with NGSolve** from the applications menu. Both installers carry the application, command-line interface, bridge, examples and a private CPython environment with the pinned wheels. On Linux, apt resolves the system graphics libraries.

| Installer | Native FieldCraft app and CLI | External solver |
|---|---|---|
| windows-x64 | Windows x64 | CPython x64 |
| windows-arm64 | Windows ARM64 | CPython x64 through Windows 11 x64 emulation |
| linux-x64 | Linux x86_64, glibc 2.35 or later | CPython x86_64 |

A Linux ARM64 installer requires compatible NGSolve and Netgen binaries. NGSolve 6.2.2608 on PyPI supplies CPython 3.13 Linux x86_64 wheels. The package lock records the selected interpreter and each wheel URL and SHA-256. Windows 11 supplies the x64 emulator used by the ARM64 edition. [NGSolve 6.2.2608 distributions](https://pypi.org/project/ngsolve/6.2.2608/#files) · [Windows on Arm emulation](https://learn.microsoft.com/en-us/windows/arm/apps-on-arm-x86-emulation)

## First calculation

Create a writable model directory. Copy the three files under `bridge/samples` into it and mesh the slab. The case command runs the selected script and opens its field and energy in a new FieldCraft window.

Linux shell:

```sh
bridge=/opt/fieldcraft-ngsolve/bridge
cp "$bridge"/samples/* .
fieldcraft-ngsolve-cli run slab.json
fieldcraft-ngsolve case slab.case.json first-run --gui /usr/bin/fieldcraft-ngsolve-gui
```

Windows PowerShell, using the default installation path:

```powershell
$fc = Join-Path $env:LOCALAPPDATA 'Programs/FieldCraft-NGSolve'
Copy-Item "$fc/bridge/samples/*" .
& "$fc/fieldcraft-cli.exe" run slab.json
& "$fc/fieldcraft-ngsolve.cmd" case slab.case.json first-run --gui "$fc/fieldcraft.exe"
```

The case file records the input model, script, named materials and boundaries, and preferred field. Change those entries for another example. To run a different NGSolve installation, pass its interpreter with `--python /path/to/python` or `--python C:\path\to\python.exe`. Running the case without `--gui` writes the fields and integrals for later viewing. Use `bridge/show_run.py` with the private interpreter to select an existing desktop instance or another field.

## Build and verification

The fork workflow pins the FieldCraft source separately from the installer branch. It builds native binaries on each target, installs the resulting package, verifies the runtime versions, solves the 1 V slab and checks its electrostatic energy against 4.4270939064 × 10⁻¹⁴ J. The verification stage imports both fields and captures the installed desktop window. Actions artifacts are available after that job passes. The installer and its download include [the external runtime notices and source references](SOLVER-NOTICES.txt). Python package notices stay with their original installed distributions. The Python distribution, external bridge and FieldCraft each include their licence files.
