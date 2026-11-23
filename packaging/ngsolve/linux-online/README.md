# Linux x86_64 network installer for the 9def desktop

`sudo apt install ./FieldCraft-NGSolve-0.8.1-9def8cf4-linux-x64-network.deb` installs the FieldCraft GUI and CLI, fetches CPython and the solver wheels from their publishers, verifies each SHA-256 and checks a finite-element integral and both bridge command entry points before enabling the runtime. An Internet connection is needed during installation. For upgrades between network-installer generations, a failed acquisition leaves the previous active runtime link intact. Reconfiguration restores missing Python bridge modules from their pinned archive. An edited file encountered during repair is preserved and reported. Run `sudo dpkg --configure fieldcraft-ngsolve-network` to retry configuration.

Choose **FieldCraft with NGSolve** in the applications menu. On a fresh user profile the launcher hides the Donate button and turns off automatic upstream update checks. It preserves existing preference files byte for byte. The preference file is under `$XDG_DATA_HOME/fieldcraft` or `$HOME/.local/share/fieldcraft`. With an explicit `FIELDCRAFT_RECOVERY_DIR` it is in the parent of that directory. A control or CI session uses the temporary automation profile unless given an explicit recovery directory. The GUI and CLI are the verified 9def8cf4 compatibility desktop. The external catalogue tree introduced on later desktop revisions requires a newer GUI.

```sh
work=$(mktemp -d)
cd "$work" && cp /opt/fieldcraft-ngsolve/bridge/samples/* .
fieldcraft-ngsolve-cli run slab.json
fieldcraft-ngsolve case slab.case.json result --gui /usr/bin/fieldcraft-ngsolve-gui
```

## Migrating the old offline Debian package

Before replacing the old `fieldcraft-ngsolve` package, keep a local copy of its original installer and your models and results. APT removes the conflicting offline package when installing `fieldcraft-ngsolve-network`, before the network package fetches its solver. If network setup fails, recover by running `sudo dpkg --configure fieldcraft-ngsolve-network` once the download is available. To restore the previous offline installation, run `sudo apt install ./fieldcraft-ngsolve-linux-x64.deb` with the saved original file. APT will ask to replace the network package. The shared executable paths make simultaneous installation of the two packages unsupported.

The private network runtime lives under `/var/lib/fieldcraft-ngsolve`. Package purge removes its complete, lock-identified runtime generations. Model files and results belong in the user's working directory.

## Rebuild

Use `build.sh original-9def.deb install-runtime.py output.deb` in a checked-out packager tree with `dpkg-deb`, Perl with `JSON::PP`, and Git on Debian or Ubuntu. Download the [standalone bootstrapper at revision 0e3064d1](https://raw.githubusercontent.com/JohannesMaierhofer/fieldcraft-ngsolve/0e3064d1bba003574f1cb3e46b0290eedc690080/packaging/debian/install-runtime.py) or use the identical file from a local checkout. The builder requires its SHA-256 to be `95e63a4dc47db5227ff56314f513d5e462dfbc3dd4c9edf77de10d861ed1e345`. The script verifies the source Debian package against the SHA-256 of the original offline package or the preceding verified 9def network package, verifies both native executable hashes, and takes only FieldCraft's own GUI, CLI, notices and desktop icon from it. The external interpreter, wheels and bridge are fetched on the destination machine from the pinned URLs in `runtime-lock.json`. `SOLVER-NOTICES.txt` accompanies the download and is installed under `/opt/fieldcraft-ngsolve`. The package records the desktop, packager, solver bridge and bootstrapper revisions in `BUILD.txt`. [Original 9def build](https://github.com/JohannesMaierhofer/fieldcraft/actions/runs/45060336394/job/137324595799)
