# FieldCraft with NGSolve

Prepare geometry, name the material volumes and boundary faces, and mesh the model in FieldCraft. Choose a Python script to solve on that mesh with NGSolve. Open the resulting scalar fields, vector components, time series and integral values in the same FieldCraft application.

This is Johannes Maierhofer’s integration fork of FieldCraft, the clean-room Rust CAE application developed by Tangency Labs and FieldCraft contributors. The external Python bridge passes the mesh to a separate NGSolve process. You can use the packaged private interpreter or choose an existing NGSolve installation. [Upstream FieldCraft](https://github.com/tangency-labs/fieldcraft) · [Python bridge and examples](https://github.com/JohannesMaierhofer/fieldcraft-ngsolve) · [Upstream discussion on user-defined PDEs and magnetic forces](https://github.com/tangency-labs/fieldcraft/issues/963)

## Download and run a first case

**[Download fork installers and checksums from Releases](https://github.com/JohannesMaierhofer/fieldcraft/releases).** Choose the installer for your processor. Fork tags use `ngsolve-v<upstream version>-r<fork revision>`. For example, `ngsolve-v0.8.1-r1` denotes the first fork release based on FieldCraft 0.8.1. The release notes identify the source revision and assets for each build. Install newer fork versions from this page so the application and solver bridge are updated together. For earlier builds that offer upstream updates, turn off **Help › Settings › Updates › Check for updates automatically** and use the fork installers.

| Tested system | Installer | FieldCraft and NGSolve |
|---|---|---|
| Intel macOS | `.pkg` | Both run natively on x86_64 |
| Apple silicon macOS | `.pkg` | Both run natively on arm64 |
| Windows x64 | `.exe` | Both run natively on x64 |
| Windows 11 on Arm | ARM64 `.exe` | Native ARM64 FieldCraft and CLI, x64 Python and NGSolve through Windows emulation |
| Ubuntu 22.04 x86_64 | `.deb` | Both run natively on x86_64 |

The macOS installer downloads the pinned solver runtime on first installation. The Windows and Linux packages include it. The macOS application has an ad-hoc signature, and the package is unsigned and unnotarized. Check the download’s SHA-256 against the release checksum before opening it. Use `shasum -a 256 <file>` on macOS, `sha256sum <file>` on Linux or `Get-FileHash -Algorithm SHA256 <file>` in PowerShell.

The bundled capacitor case starts from a 1 V potential difference. Mesh its slab recipe, run the case, then inspect the potential, electric field and stored energy. The transient magnetic diffusion example adds a time slider and playback on a fixed mesh. On all five targets, the installer tests for revision `9def8cf` solved both examples, verified the capacitor’s energy against 4.4270939064 × 10⁻¹⁴ J and exercised the native playback controls. [macOS first-case instructions](packaging/macos/ngsolve/README.md) · [Windows and Linux first-case instructions](packaging/ngsolve/README.md) · [macOS verification](https://github.com/JohannesMaierhofer/fieldcraft/actions/runs/45060336388) · [Windows and Linux verification](https://github.com/JohannesMaierhofer/fieldcraft/actions/runs/45060336394)

## Working with your own model

Open a SolidCraft design or a STEP/IGES solid, assign names to the regions and faces your equations use, and generate the mesh in FieldCraft. In the case file, specify the saved model, those names and your script. The script can return scalar fields, three-component vector fields, timestamped nodal frames and named scalar or vector integrals. FieldCraft checks the mesh fingerprint and original node labels before displaying nodal results. For magnetostatics, the script can report flux density in tesla, force in newtons and torque in newton metres about a stated origin. [Native field format](docs/recipes/external-field.md) · [Bridge and case format](https://github.com/JohannesMaierhofer/fieldcraft-ngsolve)

In the release desktop with the model browser, open the generated `view-run.json` using **File → Run Script…**.
Under **External Analysis**, select a field to draw its contour or a time series to show its playback controls.
Expand **Integrals** to read values with their units. The run also lists its named material regions, boundaries,
interfaces and loads. The armature example documents magnetic force and torque by virtual work at fixed excitation.
[3D armature and mesh refinement](https://github.com/JohannesMaierhofer/fieldcraft-ngsolve/tree/74393901cf1b6fe70e5956003f948c9fe5eee889/examples/armature)

## Upstream collaboration and credits

The fork builds on the FieldCraft 0.8.1 release and subsequent upstream development through `a6d29ed`. The Intel build and solver-independent result readers are separate upstream review candidates. [Upstream base](https://github.com/tangency-labs/fieldcraft/commit/a6d29ed54b7587ed7e7a566a4b9a61e7104cd880) · [Intel macOS builds](https://github.com/JohannesMaierhofer/fieldcraft/tree/ci/intel-macos-artifacts) · [Native fields and integrals](https://github.com/JohannesMaierhofer/fieldcraft/tree/codex/external-solver-results) · [Native time series](https://github.com/JohannesMaierhofer/fieldcraft/tree/codex/external-field-time-series) · [External model browser](https://github.com/JohannesMaierhofer/fieldcraft/tree/codex/external-model-browser) · [Local fork releases](docs/releasing.md#ngsolve-community-fork) · [Upstream contributing guide](https://github.com/tangency-labs/fieldcraft/blob/develop/CONTRIBUTING.md)

FieldCraft code is available under MIT or Apache-2.0. Third-party code, fonts, artwork and runtime distributions retain their respective licences. The upstream attribution register identifies the contributors and sources. ADR 0001 records FieldCraft’s permission for the three SolidCraft interface crates. The application packages include the FieldCraft notices, and the private Python environment retains the notices supplied with its distributions. [MIT](LICENSE-MIT) · [Apache-2.0](LICENSE-APACHE) · [Project notice](NOTICE) · [Asset and code attribution](ATTRIBUTION.md) · [SolidCraft interface licence record](docs/adr/0001-libraries.md) · [Runtime notices](packaging/ngsolve/SOLVER-NOTICES.txt)
