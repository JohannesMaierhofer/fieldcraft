# Windows package verification

## Desktop provenance

The network installers use FieldCraft desktop commit `9def8cf427505a7faf5fc79c014357ce7e170375`. The separate bridge pin for these network installers is `74393901cf1b6fe70e5956003f948c9fe5eee889`. The original desktop binaries and their original generated third-party notices were extracted from two earlier CI packages. SHA-256 was checked independently at the artifact, inner installer, selected file and final installer levels. The two GUI/CLI pairs passed the original CI on the respective Windows architectures. [Original build and hardware-specific jobs](https://github.com/JohannesMaierhofer/fieldcraft/actions/runs/45060336394)

| Embedded desktop | Earlier Windows host | GUI / CLI PE | Solver PE | Calculated slab energy (J) | Animated magnetic diffusion |
|---|---|---|---|---:|---|
| x64 | 10.0.26100 | 0x8664 / 0x8664 | 0x8664 | 4.427093906400089e-14 | 17 frames, indices 0–16 |
| ARM64 | 10.0.26200 | 0xaa64 / 0xaa64 | 0x8664 | 4.427093906400092e-14 | 17 frames, indices 0–16 |

The original recorded solver was CPython 3.13.18, NGSolve/Netgen 6.2.2608 and NumPy 2.3.5. The two scalar/vector slab fields were potential (V) and electric field (V/m). Both historical diffusion playback records ran from 0 to 0.00025464790891341067 s. The archives containing those records are artifact 14770571301 (x64, SHA-256 `03daee18e2585bc0d26f3e16f2e8351a32df095688fb3ef9d9a2433a6b2dd78c`) and 14770707078 (ARM64, `c620c3bdfc36d958aeca70cb2e9159cb488f1c665e3529c5d69e2ac4dc888df1`). The earlier test run predates the current bridge pin.

## Network installer verification

The two network installers were built locally on Intel macOS using the cached Debian Bookworm NSIS 3.08-3+deb12u1 toolchain. Only the two matching original FieldCraft executables, notices and bootstrap files are embedded. The private Python interpreter, five solver wheels and current bridge are downloaded separately from the pinned publisher endpoints at installation time.

| Locally assembled binary | Bytes | SHA-256 | Independently matched contents |
|---|---:|---|---|
| `FieldCraft-NGSolve-0.8.1-9def8cf4-windows-x64-network.exe` | 133923600 | `0bc0f396305c35640fdde0fe2a0f20e36b2966471d6685a0e0b74dbefcbd4150` | 13/13 entries |
| `FieldCraft-NGSolve-0.8.1-9def8cf4-windows-arm64-network.exe` | 116905687 | `9a892ef342f6047eb5faaf285272134c56df2cc2f90a35ac7e44c8bfc171097d` | 13/13 entries |

The final installers were compared entry by entry with the preceding local draft. Four entries changed in each package: install.ps1, INSTALL.txt, identity.json and runtime-lock.json. The other nine entries matched byte for byte, including every FieldCraft executable and license. The full distributed runtime lock has SHA-256 `f80a8787de6f5483a01084a324cd2812d70fd772c86dc6c8719dc5c738b88868`. The separate [companion build and verification helpers](https://github.com/JohannesMaierhofer/fieldcraft-ngsolve/tree/dfe7578de6e01676f31cde9e27743ccef7d4c011/packaging/windows) carry a regression test that stages two packages differing only by one wheel digest. Each staged lock matched its complete input verbatim, and the generated installation identifiers differed.

PowerShell 7.4.21 executed the production recovery routines. A separate writer exited with code 86 before it could publish a completed receipt. The next claim preserved the interrupted folder and its contents. The test also rejected a foreign directory, a mismatched receipt and a missing case module, and accepted a complete generation. The selected public bridge's real `case --help` invocation ran on the Intel Mac with the private solver Python. Removing case_run.py made that same invocation fail; restoring it made it succeed. Profile tests verified both disabled defaults, byte-identical retention of existing preferences, and a custom recovery directory. Both Windows acceptance scripts and the OpenSSH transport were checked locally.

## Native Windows x64 acceptance

The final x64 installer (`0bc0f396…`) ran on a physical x64 Windows host, build 26300. Fresh installation, repair after deliberately removing case_run.py, and a healthy rerun all succeeded. The user settings remained byte-identical after both later invocations. The GUI, CLI and Python PE machine were 0x8664. The installed runtime reported Python 3.13.18, NGSolve/Netgen 6.2.2608 and NumPy 2.3.5. The first slab returned two fields and an electrostatic energy of 4.427093906400091e-14 J.

A second project used `Meine Platte.case.json` to run `Eigener Solver.py` with an explicitly selected interpreter. Its returned script hash matched the chosen file. All 81 node IDs and all scalar/vector values were read back. The potential ranged from approximately 0 to 1 V, the x component of the electric field differed from 1 V/m by less than 9e-14 V/m, and the largest transverse component was below 1.1e-13 V/m. Its independently named energy integral returned 4.4270939064000896e-14 J. Both fields imported in the installed CLI.

An independently started instance of the installed FieldCraft GUI returned two visually inspected 1100 × 720 PNGs through `ui.capture`, showing the potential and the x component of the electric field with the integral panel. The app reported renderer `gpu`. The SSH and GUI processes ran in Windows session 0. The active console was session 1. These captures came from the actual GUI offscreen. Physical monitor presentation and Start menu interaction remain unverified. The owned temporary installation was removed after collecting the reports. ARM64 execution of the current network installer remains pending.
