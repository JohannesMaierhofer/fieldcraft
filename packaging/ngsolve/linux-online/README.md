# Linux x86_64 network installer for the 9def desktop

`sudo apt install ./FieldCraft-NGSolve-0.8.1-9def8cf4-linux-x64-network.deb` installs the FieldCraft GUI and CLI, fetches CPython and the solver wheels from their publishers, verifies each SHA-256 and checks a finite-element integral before enabling the runtime. An Internet connection is needed during installation. Failed downloads leave the previous complete runtime active. Run `sudo dpkg --configure fieldcraft-ngsolve-network` to retry configuration.

Choose **FieldCraft with NGSolve** in the applications menu. On a fresh user profile the launcher hides the Donate button. It preserves existing preference files. The GUI and CLI are the verified 9def8cf4 compatibility desktop. The external catalogue tree introduced on later desktop revisions requires a newer GUI.

```sh
work=$(mktemp -d)
cd "$work" && cp /opt/fieldcraft-ngsolve/bridge/samples/* .
fieldcraft-ngsolve-cli run slab.json
fieldcraft-ngsolve case slab.case.json result --gui /usr/bin/fieldcraft-ngsolve-gui
```

The package shares the `fieldcraft-ngsolve` command names with the old offline package and replaces it when installed. Its private runtime lives under `/var/lib/fieldcraft-ngsolve`. Package purge removes its complete, lock-identified runtime generations. Model files and results belong in the user's working directory.

## Rebuild

Use `build.sh original-9def.deb install-runtime.py output.deb` in a checked-out packager tree with `dpkg-deb`, Perl with `JSON::PP`, and Git on Debian or Ubuntu. Download the [standalone bootstrapper at revision 982793ed](https://raw.githubusercontent.com/JohannesMaierhofer/fieldcraft-ngsolve/982793ed5f0bcdd444c3ee41c2d46f37c428c16f/packaging/debian/install-runtime.py) or use the identical file from a local checkout. The builder requires its SHA-256 to be `309d9e570d8b08754eb971b9304393e8b012487d0a7bc26d34a319ae77966e47`. The script verifies the source Debian package against its original SHA-256 and takes only FieldCraft's own GUI, CLI, notices and desktop icon from it. The external interpreter, wheels and bridge are fetched on the destination machine from the pinned URLs in `runtime-lock.json`. `SOLVER-NOTICES.txt` accompanies the download and is installed under `/opt/fieldcraft-ngsolve`. The package records the desktop, packager, solver bridge and bootstrapper revisions in `BUILD.txt`. [Original 9def build](https://github.com/JohannesMaierhofer/fieldcraft/actions/runs/45060336394/job/137324595799)
