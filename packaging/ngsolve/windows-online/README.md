# Windows network installers

The x64 and ARM64 packages use the FieldCraft 0.8.1 desktop and CLI from revision `9def8cf4` and install the external bridge at `74393901`. The published binaries in the original per-platform build are reused byte for byte. Newer model-browser functionality requires a subsequent desktop build.

The user runs one per-user installer. It downloads its private CPython from Astral, the five pinned wheels from PyPI, and the bridge from the bridge project's source archive. Each download must match the SHA-256 in the shared runtime lock before extraction or execution. Select x64 for Windows 10 (1809+) or Windows 11 on Intel/AMD. Windows 11 on ARM64 runs the native ARM64 desktop and the x64 solver under Windows' application emulation. Windows PowerShell 5.1, the system tar.exe, and outbound HTTPS to the pinned origins are required.

The default installation directory is `%LOCALAPPDATA%\Programs\FieldCraft-NGSolve-Network\9def8cf4-74393901-<arch>`. A fresh profile starts with the Donate button and upstream update checks disabled. The installer preserves an existing preference file byte for byte. The Start menu includes the desktop and examples. The installed network-edition instructions describe removal. Windows' signature verification reports an unsigned publisher for these local builds. Verify the separately supplied installer digest.

## Rebuild

The build host uses Python 3.11+ and a separate set of [companion build helpers at revision 376dd39](https://github.com/JohannesMaierhofer/fieldcraft-ngsolve/tree/376dd39e02dabb9abb06dcaa558c288ac667bdf2/packaging/windows). Check out that exact commit and set `helpers` to its absolute `packaging/windows` directory. Run the following steps from this FieldCraft `windows-online` directory. The helpers read the FieldCraft recipe via `--recipe` and leave the separately pinned solver bridge unchanged.

The original archives are the `fieldcraft-ngsolve-windows-x64` (artifact 14769718249) and `fieldcraft-ngsolve-windows-arm64` (14770950364) outputs of the [desktop build](https://github.com/JohannesMaierhofer/fieldcraft/actions/runs/45060336394). The immutable original ZIP and internal installer digests are in the companion `prepare.py`. Building new installers does not execute the old installers.

1. Verify the archive and unwrap the inner installer with `python "$helpers/prepare.py" legacy x64 original.zip original.exe`. Substitute `arm64` for the other platform.
2. Install the build-only BSD archive reader [binary-refinery 0.11.3](https://pypi.org/project/binary-refinery/0.11.3/) in a separate packaging environment. Its source archive SHA-256 is `8b5bf2113939cb1e2b8ca234ba40bef07aff87a33685dfcbdc7105d59675a520`. Run `python "$helpers/prepare.py" core x64 original.exe CORE`. Only the two FieldCraft binaries and their four notice files are selected and checked against the original Inno file digests.
3. Run `python "$helpers/prepare.py" stage --recipe "$PWD" x64 CORE /usr/share/doc/nsis/copyright payload`. This checks the SHA-256 of the six original application files and the GUI/CLI PE machine. It stages the desktop, CLI, their four original notices, the NSIS notice, the solver notices, installation instructions, scripts and pins.
4. Compile with NSIS 3.08 on the local packaging host. Use absolute paths for PAYLOAD and OUTPUT. Run from this source directory and repeat for ARM64.

   ```sh
   makensis -DARCH=x64 -DPAYLOAD=/work/payload -DOUTPUT=/work/FieldCraft-NGSolve-0.8.1-9def8cf4-windows-x64-network.exe setup.nsi
   ```

5. Run `python "$helpers/verify_payload.py" payload installer.exe` with 7-Zip on the tool path to verify every extracted byte and the complete inventory against the staged files. Record a SHA-256 for the finished installer. The earlier offline package and the downloaded runtime archives are excluded from its file list.

## Check a fresh installation

Run this in Windows PowerShell with a fresh local scratch directory. The test installs into a path containing spaces, skips Start menu shortcuts, checks fresh-profile settings and preservation of explicit settings on reinstall, verifies the three PE machine IDs, computes a 1 V dielectric slab and imports both fields in the installed CLI. It writes `verification.json` under the scratch directory, which you can remove after inspection.

```powershell
.\verify.ps1 -Installer C:\Downloads\FieldCraft-NGSolve-0.8.1-9def8cf4-windows-x64-network.exe -Arch x64 -Work "$env:TEMP\fieldcraft-online-qa"
```

The analytical energy is 4.4270939064 × 10⁻¹⁴ J. The script accepts a relative deviation up to 10⁻⁸. Historical verification of the embedded desktop and new network-installer verification are recorded separately in `VERIFICATION.md`.

### Run the same test over OpenSSH

Use a Windows host whose SSH key you have already verified. The local machine needs Python 3.11+, OpenSSH and an adjacent `.exe.sha256`. Select the remote host explicitly. The command below expects the usual OpenSSH agent or configuration to provide authentication.

```sh
python "$helpers/remote_verify.py" known-windows-host x64 ./FieldCraft-NGSolve-0.8.1-9def8cf4-windows-x64-network.exe ./fresh-windows-proof.json --verify-script ./verify.ps1
```

A unique directory below the selected account's Windows temporary folder holds the installer, transcript, downloaded runtime, test prefix and solved model. The installer hash is verified before and after transfer. The command fetches the result JSON and removes its own test directory after success. On failure, the printed remote directory retains the logs. The commands use noninteractive key authentication with strict host-key checking. Installing private keys or starting an SSH server is outside the test.
