# Windows r2 network installers

These per-user installers package the GUI and CLI compiled from FieldCraft commit `8b495f31389512c7f599ef17b5ee3a4d0cc39bc9`. Both executables report the desktop version `0.8.1-ngsolve.r1`. The installer version is `0.8.1-r2`. Windows x64 runs the native x64 desktop and solver. Windows 11 on ARM64 runs an ARM64 desktop and uses Windows x64 emulation for the separately installed Python and NGSolve wheels.

## Run the first case

Open **Solver examples** from **FieldCraft with NGSolve (network)** in the Start menu. Copy the sample files to a new project folder and open PowerShell in that folder. Set `$app` to the installed directory that contains `fieldcraft.exe`. The **FieldCraft** shortcut's **Properties → Target** shows the executable path.

```powershell
& "$app\fieldcraft-cli.exe" run slab.json
& "$app\fieldcraft-ngsolve.cmd" case slab.case.json run-01 --gui "$app\fieldcraft.exe"
```

The first command creates the mesh. The second runs the Python script named in the case and opens the result window. Under **External Analysis**, expand **Fields** and select `potential` or `electric_field`. **Integrals** contains the stored energy, approximately 4.42709 × 10⁻¹⁴ J. For another calculation, edit the case or its script, save it, and specify a new output directory. A Python exception prints its traceback in PowerShell.

## Build from source on Windows

Use a clean checkout of the desktop revision above, Rust 1.99.1 with the target standard library, the matching Microsoft C++ toolchain and Windows SDK, and the NSIS compiler with its distribution license. The installer recipe may live in another checkout. For a native build, start the Visual Studio developer shell for the target architecture. A Windows x64 host can also cross-compile the ARM64 installer. Set `CARGO_HOME`, `RUSTUP_HOME`, `CARGO_TARGET_DIR` and `TEMP` to a sufficiently large build drive when required.

```powershell
.\package-source.ps1 -Arch x64 -SourceRoot F:\desktop -Output F:\output-x64 -MakeNsis F:\nsis\makensis.exe -NsisLicense F:\nsis\COPYING
.\package-source.ps1 -Arch arm64 -SourceRoot F:\desktop -Output F:\output-arm64 -MakeNsis F:\nsis\makensis.exe -NsisLicense F:\nsis\COPYING
```

The script checks the source revision and cleanliness, builds both executables with Cargo's locked dependency graph, checks their PE machine type, generates third-party license notices, and records the SHA-256 of each staged file. When the host architecture matches the target, it also runs the CLI and compares the feature catalog with the reference documentation. Outputs include `FieldCraft-NGSolve-0.8.1-r2-windows-{x64,arm64}-network.exe`, its SHA-256 sidecar, the payload, and a per-architecture build receipt. The example compiler distribution is the [NSIS 3.11 portable ZIP](https://sourceforge.net/projects/nsis/files/NSIS%203/3.11/nsis-3.11.zip/download), SHA-256 `c7d27f780ddb6cffb4730138cd1591e841f4b7edb155856901cdf5f214394fa1`.

## Install and verify

Check the installer SHA-256, then run the installer matching the host OS architecture. The installer requires network access to the upstream file hosts named in `runtime-lock.json`. It verifies the x64 CPython runtime, each wheel and the bridge at `74393901cf1b6fe70e5956003f948c9fe5eee889` before installing them. An existing healthy generation can be reused. A repaired generation preserves its damaged predecessor. See [user instructions](INSTALL.txt).

Run the acceptance test on an actual machine of the target architecture with a fresh working directory and the published installer. It installs, deliberately removes one bridge file in that isolated generation, repairs the installation, checks repeat installation and validates the native PE identities. It runs the one-volt capacitor, checks the energy and imports both fields. A separate manufactured shear field on the same mesh exercises catalog selection and deformation factors of 2.5, 0.25 and zero. Results are written to `verification.json`. An ARM64 PE check on a cross-build host leaves the Windows ARM64 runtime test outstanding.

```powershell
.\verify.ps1 -Installer F:\output-x64\FieldCraft-NGSolve-0.8.1-r2-windows-x64-network.exe -Arch x64 -Work F:\fresh-qa
```
