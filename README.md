# FieldCraft with NGSolve

Prepare geometry, name the material volumes and boundary faces, and mesh the model in FieldCraft. Choose a Python script to solve on that mesh with NGSolve. Open the resulting scalar fields, vector components, time series and integral values in the same FieldCraft application.

This is Johannes Maierhofer's integration fork of FieldCraft, the clean-room Rust CAE application developed by Tangency Labs and FieldCraft contributors. FieldCraft prepares the mesh and launches the analysis in a separate Python process. The installer provides a private interpreter with NGSolve, NumPy, and uv. [Upstream FieldCraft](https://github.com/tangency-labs/fieldcraft) · [Upstream discussion on user-defined PDEs and magnetic forces](https://github.com/tangency-labs/fieldcraft/issues/963)

## Install FieldCraft with NGSolve

Choose your processor on the [fork's Releases page](https://github.com/JohannesMaierhofer/fieldcraft/releases). Tags use `ngsolve-v<upstream version>-r<fork revision>`. The r3 installer for Intel macOS includes the Python Workspace and downloads the checksummed runtime during setup. After setup, analyses run offline. The r2 release lists the earlier installers for Apple silicon, Windows, and Linux. [Intel macOS packaging](packaging/macos/ngsolve/README.md) · [r2 installers](https://github.com/JohannesMaierhofer/fieldcraft/releases/tag/ngsolve-v0.8.1-r2)

## Working with your own model

Open a SolidCraft design or a STEP or IGES solid, name the material and boundary sets, and generate the mesh in FieldCraft. Switch to **External Analysis** and open the **Python Workspace**. Choose **New Analysis** to create an `.analysis.json` definition linking the model, sets, and Python script. Choose **Run Analysis** to solve on the exported mesh. Run `# %%` sections and selections in the persistent console. Use **Settings** to install project packages with uv. [Python Workspace](docs/python-workspace.md)

Expand **External Analysis** in the model browser to inspect fields, time series, and integrals. FieldCraft verifies the original mesh fingerprint and node labels before displaying nodal results. For a displacement field in meters, use **Deformed → Show** and set a scale to inspect the displacement. [Native field format](docs/recipes/external-field.md)

## Upstream collaboration and credits

The fork builds on the FieldCraft 0.8.1 release and subsequent upstream development through `a6d29ed`. The Intel build and solver-independent result readers are separate upstream review candidates. [Upstream base](https://github.com/tangency-labs/fieldcraft/commit/a6d29ed54b7587ed7e7a566a4b9a61e7104cd880) · [Intel macOS builds](https://github.com/JohannesMaierhofer/fieldcraft/tree/ci/intel-macos-artifacts) · [Native fields and integrals](https://github.com/JohannesMaierhofer/fieldcraft/tree/codex/external-solver-results) · [Native time series](https://github.com/JohannesMaierhofer/fieldcraft/tree/codex/external-field-time-series) · [External model browser](https://github.com/JohannesMaierhofer/fieldcraft/tree/codex/external-model-browser) · [Adjustable mesh deformation](https://github.com/JohannesMaierhofer/fieldcraft/tree/codex/external-displacement-view) · [Local fork releases](docs/releasing.md#ngsolve-community-fork) · [Upstream contributing guide](https://github.com/tangency-labs/fieldcraft/blob/develop/CONTRIBUTING.md)

FieldCraft code is available under MIT or Apache-2.0. Third-party code, fonts, artwork and runtime distributions retain their respective licenses. The upstream attribution register identifies the contributors and sources. ADR 0001 records FieldCraft’s permission for the three SolidCraft interface crates. The application packages include the FieldCraft notices, and the private Python environment retains the notices supplied with its distributions. [MIT](LICENSE-MIT) · [Apache-2.0](LICENSE-APACHE) · [Project notice](NOTICE) · [Asset and code attribution](ATTRIBUTION.md) · [SolidCraft interface license record](docs/adr/0001-libraries.md) · [Runtime notices](packaging/ngsolve/SOLVER-NOTICES.txt)
