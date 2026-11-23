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

| Locally assembled binary | Bytes | SHA-256 | Independently extracted and matched |
|---|---:|---|---|
| `FieldCraft-NGSolve-0.8.1-9def8cf4-windows-x64-network.exe` | 133921814 | `f39e853227ce7026656de148c7d2d01d77587f9fb6a80964cf2861ff80b075a1` | 13/13 entries |
| `FieldCraft-NGSolve-0.8.1-9def8cf4-windows-arm64-network.exe` | 116903895 | `a66c53442a789ea592eb2299ab8d899ebfb7109580d013bcb70a7e7f1ff0b852` | 13/13 entries |

Both finished installer hashes were checked again after staging for delivery. Each final package was compared with its previous local draft entry by entry. The two changed entries were install.ps1 and INSTALL.txt. The other eleven entries remained byte-identical, including all six original application files. The installer recipe uses the [published companion helpers at 376dd39](https://github.com/JohannesMaierhofer/fieldcraft-ngsolve/tree/376dd39e02dabb9abb06dcaa558c288ac667bdf2/packaging/windows). The fresh-profile function ran under PowerShell 7.4.21 with a temporary user profile. It set both switches to false and preserved a previously created explicit true/true preference file byte for byte. The custom recovery location and cleanup of temporary files also passed. Both PowerShell files parsed with that version. The OpenSSH wrapper's three PowerShell commands also parsed, and offline tests exercised transfer order, quoted paths, hash validation, proof handling and scratch-only cleanup. Native current-installer execution on Windows is pending. Building and inspecting the NSIS package on the Intel Mac does not establish Windows installation, networking or GUI behavior for this edition.
