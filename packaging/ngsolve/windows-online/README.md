# Windows r2 network installers

These per-user installers package the GUI and CLI compiled from FieldCraft commit `8b495f31389512c7f599ef17b5ee3a4d0cc39bc9`. Both executables report the desktop version `0.8.1-ngsolve.r1`. The installer version is `0.8.1-r2`. Windows x64 runs the native x64 desktop and solver. Windows 11 on ARM64 runs an ARM64 desktop and uses Windows x64 emulation for the separately installed Python and NGSolve wheels.

## Run the first case

Open **Solver examples** from **FieldCraft with NGSolve (network)** in the Start menu. Copy the sample files to a new project folder and open PowerShell in that folder. Set `$app` to the installed directory that contains `fieldcraft.exe`. The **FieldCraft** shortcut's **Properties → Target** shows the executable path.

```powershell
& "$app\fieldcraft-cli.exe" run slab.json
& "$app\fieldcraft-ngsolve.cmd" case slab.case.json run-01 --gui "$app\fieldcraft.exe"
```

The first command creates the mesh. The second runs the Python script named in the case and opens the result window. Under **External Analysis**, expand **Fields** and select `potential` or `electric_field`. **Integrals** contains the stored energy, approximately 4.42709 × 10⁻¹⁴ J. For another calculation, edit the case or its script, save it, and specify a new output directory. A Python exception prints its traceback in PowerShell.

## Build and package in Linux Docker

Use a Linux Docker engine (AMD64 or ARM64) and run these commands in this directory. The image installs Rust 1.99.1, Clang/LLVM, cargo-xwin and NSIS. Each container checks out the exact public desktop commit, cross-compiles the selected MSVC target and packages it with Linux NSIS. For ARM64, the container translates clang-cl system-include flags when a dependency invokes the GNU Clang driver directly. Cargo and the Microsoft SDK download caches live in a project-owned Linux volume. Allow sufficient disk space on the Docker engine. The build fetches the Microsoft CRT and SDK under their distribution terms. [cargo-xwin documentation](https://github.com/rust-cross/cargo-xwin)

```sh
docker build -t fieldcraft-windows-r2 .
docker volume create fieldcraft-windows-r2-cache
for arch in x64 arm64; do
    job="fieldcraft-windows-r2-$arch"
    docker run --name "$job" \
        --mount source=fieldcraft-windows-r2-cache,target=/cache \
        fieldcraft-windows-r2 "$arch" || exit 1
    docker cp "$job:/out/." .
    docker rm "$job"
done
```

Each build checks its PE machine type and the compiled external-deformation command, regenerates the dependency license notices, and compares all 13 files after independently decompressing the installer. The outputs include `FieldCraft-NGSolve-0.8.1-r2-windows-{x64,arm64}-network.exe`, its SHA-256 sidecar, and a JSON receipt identifying the source, build tools, container, PE target, and all payload hashes. Run the physical Windows acceptance test below for runtime verification.

## Install and verify

Check the installer SHA-256, then run the installer matching the host OS architecture. The installer requires network access to the upstream file hosts named in `runtime-lock.json`. It verifies the x64 CPython runtime, each wheel and the bridge at `74393901cf1b6fe70e5956003f948c9fe5eee889` before installing them. An existing healthy generation can be reused. A repaired generation preserves its damaged predecessor. See [user instructions](INSTALL.txt).

Run the acceptance test on an actual machine of the target architecture with a fresh working directory and the published installer. It installs, deliberately removes one bridge file in that isolated generation, repairs the installation, checks repeat installation and validates the native PE identities. It runs the one-volt capacitor, checks the energy and imports both fields. A separate manufactured shear field on the same mesh exercises catalog selection and deformation factors of 2.5, 0.25 and zero. Results are written to `verification.json`. An ARM64 PE check on a cross-build host leaves the Windows ARM64 runtime test outstanding.

```powershell
.\verify.ps1 -Installer F:\output-x64\FieldCraft-NGSolve-0.8.1-r2-windows-x64-network.exe -Arch x64 -Work F:\fresh-qa
```
