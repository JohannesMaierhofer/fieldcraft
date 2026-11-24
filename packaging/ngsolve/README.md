# FieldCraft with an external NGSolve runtime

R5 installs FieldCraft and downloads its private, checksummed Python runtime on Windows, Linux, and macOS. [Downloads](https://github.com/Maierhofer-Technology/fieldcraft/releases/latest) · [macOS packaging](../macos/ngsolve/README.md)

The Windows installer sets up FieldCraft for the current user. Start it from the Start menu. The Linux amd64 package supports Ubuntu 22.04 or later. Install it with `sudo apt install ./FieldCraft-NGSolve-0.8.1-r5-linux-x64-network.deb` and select **FieldCraft with NGSolve** from the applications menu. Both installers include the application and command-line interface. On Linux, apt resolves the system graphics libraries.

| Installer | Native FieldCraft app and CLI | External solver |
|---|---|---|
| windows-x64 | Windows x64 | CPython x64 |
| windows-arm64 | Windows ARM64 | CPython x64 through Windows 11 x64 emulation |
| linux-x64 | Linux x86_64, glibc 2.35 or later | CPython x86_64 |

A Linux ARM64 installer requires compatible NGSolve and Netgen binaries. NGSolve 6.2.2608 on PyPI supplies CPython 3.13 Linux x86_64 wheels. The package lock records the selected interpreter and each wheel URL and SHA-256. Windows 11 supplies the x64 emulator used by the ARM64 edition. [NGSolve 6.2.2608 distributions](https://pypi.org/project/ngsolve/6.2.2608/#files) · [Windows on Arm emulation](https://learn.microsoft.com/en-us/windows/arm/apps-on-arm-x86-emulation)

## First calculation

In **External Analysis**, choose **Open Python Workspace**, open your analysis definition (`.analysis.json`), and choose **Run Analysis**. FieldCraft exports the mesh, starts NGSolve in the private interpreter, and displays the selected result. Use **Settings** to manage project packages with the installed uv. [Python Workspace](../../docs/python-workspace.md)

Run the same analysis headlessly with the installed launcher. The output directory must be new.

```sh
fieldcraft-ngsolve run slab.analysis.json first-run
```

On Windows, open PowerShell in the version directory containing the installed desktop executable. The default installation places its versions under `%LOCALAPPDATA%\Programs\FieldCraft-NGSolve-Network`.

```powershell
.\fieldcraft-ngsolve.cmd run C:\Studies\Slab\slab.analysis.json C:\Studies\Slab\first-run
```

For another interpreter, invoke the platform's FieldCraft CLI as `fieldcraft-cli ngsolve run slab.analysis.json first-run --python /path/to/python`. Open the output manifest from the matching model's Python Workspace to view the result later.

## Build and verification

The fork workflow builds FieldCraft from the same commit as the installer recipe. It builds native binaries on each target, installs the resulting package, verifies the runtime versions, solves the 1 V slab and checks its electrostatic energy against 4.4270939064 × 10⁻¹⁴ J. The verification stage imports both fields and captures the installed desktop window. Actions artifacts are available after that job passes. The installer and its download include [the external runtime notices and source references](SOLVER-NOTICES.txt). Python package notices stay with their original installed distributions. The Python distribution and FieldCraft each include their license files.
